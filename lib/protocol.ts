export const PROTOCOL_VERSION = "proofline/2" as const;
export const GENESIS_DIGEST = "0".repeat(64);

export type AssuranceLevel = "strongbox" | "tee" | "software_attested" | "web_key";
export type CaptureStatus = "initializing" | "live" | "stalled" | "sealed" | "interrupted" | "tombstoned";
export type CompletenessClaim = "complete_with_signed_end" | "complete_as_server_received" | "gaps_detected" | "pending";
export type StreamRole = "rear_video" | "front_video" | "audio" | "telemetry";
export type CloseReason = "user_stop" | "duration_limit" | "permission_revoked" | "thermal_shutdown" | "app_error" | "server_timeout";

export interface StreamDeclaration {
  id: string;
  role: StreamRole;
  mimeType: string;
  codec: string;
  width?: number;
  height?: number;
  fps?: number;
  hasAudio: boolean;
}

export interface CaptureSummary {
  id: string;
  deviceFingerprint: string;
  title: string;
  status: CaptureStatus;
  completeness: CompletenessClaim;
  assuranceLevel: AssuranceLevel;
  startedAt: string;
  endedAt?: string | null;
  lastReceiptAt?: string | null;
  exactLocationReleaseAt?: string | null;
  latitude?: number | null;
  longitude?: number | null;
  locationIsCoarse: boolean;
  streamCount: number;
  durationMs: number;
  closeReason?: CloseReason | null;
  verification: VerificationSummary;
  mediaBaseUrl: string;
  posterUrl?: string | null;
  tombstoneReason?: string | null;
}

export interface VerificationSummary {
  fragmentChain: "pass" | "warn" | "fail" | "pending";
  deviceSignature: "pass" | "warn" | "fail" | "pending";
  audioBinding: "pass" | "warn" | "fail" | "pending";
  serverReceipts: "pass" | "warn" | "fail" | "pending";
  timestampAnchor: "pass" | "warn" | "fail" | "pending";
  c2pa: "pass" | "warn" | "fail" | "unsupported" | "pending";
}

export interface CreateCaptureRequest {
  sessionNonce: string;
  deviceFingerprint: string;
  assuranceLevel: AssuranceLevel;
  devicePublicKeySpki: string;
  sessionPublicKeySpki: string;
  sessionBindingSignature: string;
  title?: string;
  startedAt: string;
  streams: StreamDeclaration[];
  location?: { latitude: number; longitude: number; accuracyM: number };
}

export interface SessionBinding {
  protocolVersion: typeof PROTOCOL_VERSION;
  challenge: string;
  deviceFingerprint: string;
  sessionPublicKeySpki: string;
  startedAt: string;
  streams: StreamDeclaration[];
}

export interface CreateCaptureResponse {
  captureId: string;
  uploadToken: string;
  mediaBaseUrl: string;
  expiresAt: string;
  maxDurationSeconds: 3600;
  fragmentDurationMs: 2000;
}

export interface FragmentEnvelope {
  protocol_version: typeof PROTOCOL_VERSION;
  capture_id: string;
  stream_id: string;
  sequence: number;
  previous_chain_digest: string;
  media_digest: string;
  chain_digest: string;
  byte_length: number;
  pts_start_us: number;
  pts_end_us: number;
  telemetry_root: string;
}

export interface LedgerEvent {
  id: string;
  captureId: string;
  type: "capture.live" | "capture.receipt" | "capture.stalled" | "capture.sealed" | "capture.interrupted" | "capture.tombstoned" | "capture.recovery";
  occurredAt: string;
  payload: Record<string, unknown>;
  signature: string;
}

/** RFC 8785-compatible canonicalization for the JSON values used by ProofLine. */
export function canonicalize(value: unknown): string {
  if (value === null || typeof value !== "object") return JSON.stringify(value);
  if (Array.isArray(value)) return `[${value.map(canonicalize).join(",")}]`;
  const object = value as Record<string, unknown>;
  return `{${Object.keys(object).sort().filter((key) => object[key] !== undefined).map((key) => `${JSON.stringify(key)}:${canonicalize(object[key])}`).join(",")}}`;
}

export async function sha256Hex(value: ArrayBuffer | Uint8Array | string): Promise<string> {
  const bytes = typeof value === "string" ? new TextEncoder().encode(value) : value instanceof Uint8Array ? value : new Uint8Array(value);
  const digest = await crypto.subtle.digest("SHA-256", Uint8Array.from(bytes));
  return [...new Uint8Array(digest)].map((byte) => byte.toString(16).padStart(2, "0")).join("");
}

export function base32(bytes: Uint8Array): string {
  const alphabet = "ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";
  let bits = 0;
  let buffer = 0;
  let output = "";
  for (const byte of bytes) {
    buffer = (buffer << 8) | byte;
    bits += 8;
    while (bits >= 5) {
      output += alphabet[(buffer >>> (bits - 5)) & 31];
      bits -= 5;
    }
  }
  if (bits > 0) output += alphabet[(buffer << (5 - bits)) & 31];
  return output.toLowerCase();
}

export async function publicKeyFingerprint(spkiBase64: string): Promise<string> {
  const normalized = spkiBase64.replace(/-/g, "+").replace(/_/g, "/");
  const padded = normalized.padEnd(Math.ceil(normalized.length / 4) * 4, "=");
  const bytes = Uint8Array.from(atob(padded), (character) => character.charCodeAt(0));
  return base32(new Uint8Array(await crypto.subtle.digest("SHA-256", bytes)));
}
