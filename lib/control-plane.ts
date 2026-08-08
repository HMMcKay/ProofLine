import {
  type AssuranceLevel,
  type CaptureStatus,
  type CaptureSummary,
  type CompletenessClaim,
  type CreateCaptureRequest,
  type LedgerEvent,
  type VerificationSummary,
} from "./protocol";

interface Bindings {
  DB: D1Database;
  MEDIA_CONTROL_URL?: string;
  MEDIA_PUBLIC_URL?: string;
  PROOFLINE_INTERNAL_SECRET?: string;
}

const schema = [
  `CREATE TABLE IF NOT EXISTS devices (fingerprint TEXT PRIMARY KEY NOT NULL, assurance_level TEXT NOT NULL, public_key_spki TEXT NOT NULL, attestation_summary TEXT NOT NULL DEFAULT '{}', created_at TEXT NOT NULL, rotated_from TEXT)`,
  `CREATE TABLE IF NOT EXISTS captures (id TEXT PRIMARY KEY NOT NULL, device_fingerprint TEXT NOT NULL, title TEXT NOT NULL, status TEXT NOT NULL, completeness TEXT NOT NULL, assurance_level TEXT NOT NULL, started_at TEXT NOT NULL, ended_at TEXT, last_receipt_at TEXT, exact_location_release_at TEXT, coarse_latitude REAL, coarse_longitude REAL, exact_latitude REAL, exact_longitude REAL, location_accuracy_m REAL, stream_count INTEGER NOT NULL DEFAULT 1, duration_ms INTEGER NOT NULL DEFAULT 0, close_reason TEXT, verification_summary TEXT NOT NULL DEFAULT '{}', media_base_url TEXT NOT NULL, poster_url TEXT, tombstone_reason TEXT, created_at TEXT NOT NULL, updated_at TEXT NOT NULL)`,
  `CREATE INDEX IF NOT EXISTS idx_captures_started_at ON captures(started_at)`,
  `CREATE INDEX IF NOT EXISTS idx_captures_status_started_at ON captures(status, started_at)`,
  `CREATE INDEX IF NOT EXISTS idx_captures_device_started_at ON captures(device_fingerprint, started_at)`,
  `CREATE TABLE IF NOT EXISTS capture_streams (id TEXT PRIMARY KEY NOT NULL, capture_id TEXT NOT NULL, role TEXT NOT NULL, mime_type TEXT NOT NULL, codec TEXT NOT NULL, width INTEGER, height INTEGER, fps REAL, has_audio INTEGER NOT NULL DEFAULT 0, sequence_count INTEGER NOT NULL DEFAULT 0, final_chain_digest TEXT)`,
  `CREATE UNIQUE INDEX IF NOT EXISTS uq_capture_stream_role ON capture_streams(capture_id, role)`,
  `CREATE TABLE IF NOT EXISTS ledger_events (id TEXT PRIMARY KEY NOT NULL, capture_id TEXT NOT NULL, type TEXT NOT NULL, occurred_at TEXT NOT NULL, payload_json TEXT NOT NULL, signature TEXT NOT NULL, received_at TEXT NOT NULL)`,
  `CREATE INDEX IF NOT EXISTS idx_ledger_capture_time ON ledger_events(capture_id, occurred_at)`,
  `CREATE TABLE IF NOT EXISTS attestation_challenges (nonce TEXT PRIMARY KEY NOT NULL, created_at TEXT NOT NULL, expires_at TEXT NOT NULL, used_at TEXT)`,
  `CREATE TABLE IF NOT EXISTS rate_counters (key TEXT NOT NULL, window_start TEXT NOT NULL, count INTEGER NOT NULL DEFAULT 0, bytes INTEGER NOT NULL DEFAULT 0)`,
  `CREATE UNIQUE INDEX IF NOT EXISTS uq_rate_counter_window ON rate_counters(key, window_start)`,
];

let schemaReady: Promise<void> | undefined;

export function bindings(): Bindings {
  const runtime = (globalThis as typeof globalThis & { __PROOFLINE_ENV__?: Partial<Bindings> }).__PROOFLINE_ENV__;
  if (!runtime?.DB) throw new Error("ProofLine requires the Sites D1 binding named DB");
  return runtime as Bindings;
}

export async function ensureSchema(): Promise<void> {
  if (!schemaReady) {
    schemaReady = bindings().DB.batch(schema.map((statement) => bindings().DB.prepare(statement))).then(() => undefined).catch((error) => {
      schemaReady = undefined;
      throw error;
    });
  }
  await schemaReady;
}

export function json(value: unknown, status = 200, headers: HeadersInit = {}): Response {
  return new Response(JSON.stringify(value), {
    status,
    headers: { "content-type": "application/json; charset=utf-8", "cache-control": "no-store", ...headers },
  });
}

export async function readJson<T>(request: Request): Promise<T> {
  const contentType = request.headers.get("content-type") ?? "";
  if (!contentType.includes("application/json")) throw new Error("Expected application/json");
  return request.json() as Promise<T>;
}

function parseJson<T>(value: unknown, fallback: T): T {
  if (typeof value !== "string") return fallback;
  try { return JSON.parse(value) as T; } catch { return fallback; }
}

const pendingVerification: VerificationSummary = {
  fragmentChain: "pending",
  deviceSignature: "pending",
  audioBinding: "pending",
  serverReceipts: "pending",
  timestampAnchor: "pending",
  c2pa: "pending",
};

type CaptureRow = Record<string, unknown>;

function numberOrNull(value: unknown): number | null {
  return typeof value === "number" ? value : value == null ? null : Number(value);
}

export function rowToCapture(row: CaptureRow, now = Date.now()): CaptureSummary {
  const releaseAt = typeof row.exact_location_release_at === "string" ? Date.parse(row.exact_location_release_at) : Number.POSITIVE_INFINITY;
  const exactReleased = now >= releaseAt;
  return {
    id: String(row.id),
    deviceFingerprint: String(row.device_fingerprint),
    title: String(row.title),
    status: String(row.status) as CaptureStatus,
    completeness: String(row.completeness) as CompletenessClaim,
    assuranceLevel: String(row.assurance_level) as AssuranceLevel,
    startedAt: String(row.started_at),
    endedAt: row.ended_at ? String(row.ended_at) : null,
    lastReceiptAt: row.last_receipt_at ? String(row.last_receipt_at) : null,
    exactLocationReleaseAt: row.exact_location_release_at ? String(row.exact_location_release_at) : null,
    latitude: exactReleased ? numberOrNull(row.exact_latitude) : numberOrNull(row.coarse_latitude),
    longitude: exactReleased ? numberOrNull(row.exact_longitude) : numberOrNull(row.coarse_longitude),
    locationIsCoarse: !exactReleased,
    streamCount: Number(row.stream_count ?? 1),
    durationMs: Number(row.duration_ms ?? 0),
    closeReason: row.close_reason ? String(row.close_reason) as CaptureSummary["closeReason"] : null,
    verification: parseJson(row.verification_summary, pendingVerification),
    mediaBaseUrl: String(row.media_base_url),
    posterUrl: row.poster_url ? String(row.poster_url) : null,
    tombstoneReason: row.tombstone_reason ? String(row.tombstone_reason) : null,
  };
}

export async function listCaptures(options: { status?: string; device?: string; limit?: number } = {}): Promise<CaptureSummary[]> {
  await ensureSchema();
  const filters: string[] = [];
  const values: unknown[] = [];
  if (options.status === "live") {
    filters.push("status IN ('initializing','live','stalled')");
  } else if (options.status && options.status !== "recent") {
    filters.push("status = ?"); values.push(options.status);
  }
  if (options.device) { filters.push("device_fingerprint = ?"); values.push(options.device); }
  const where = filters.length ? `WHERE ${filters.join(" AND ")}` : "";
  const limit = Math.min(Math.max(options.limit ?? 48, 1), 100);
  const result = await bindings().DB.prepare(`SELECT * FROM captures ${where} ORDER BY started_at DESC LIMIT ?`).bind(...values, limit).all<CaptureRow>();
  return result.results.map((row) => rowToCapture(row));
}

export async function getCapture(id: string): Promise<{ capture: CaptureSummary; streams: CaptureRow[]; events: CaptureRow[] } | null> {
  await ensureSchema();
  const [row, streams, events] = await Promise.all([
    bindings().DB.prepare("SELECT * FROM captures WHERE id = ?").bind(id).first<CaptureRow>(),
    bindings().DB.prepare("SELECT * FROM capture_streams WHERE capture_id = ? ORDER BY role").bind(id).all<CaptureRow>(),
    bindings().DB.prepare("SELECT * FROM ledger_events WHERE capture_id = ? ORDER BY occurred_at DESC LIMIT 200").bind(id).all<CaptureRow>(),
  ]);
  return row ? { capture: rowToCapture(row), streams: streams.results, events: events.results.map((event) => ({ ...event, payload: parseJson(event.payload_json, {}) })) } : null;
}

export async function consumeRate(key: string, limit: number): Promise<boolean> {
  await ensureSchema();
  const hour = new Date(); hour.setUTCMinutes(0, 0, 0);
  const windowStart = hour.toISOString();
  await bindings().DB.prepare(`INSERT INTO rate_counters(key, window_start, count, bytes) VALUES(?, ?, 1, 0) ON CONFLICT(key, window_start) DO UPDATE SET count = count + 1`).bind(key, windowStart).run();
  const row = await bindings().DB.prepare("SELECT count FROM rate_counters WHERE key = ? AND window_start = ?").bind(key, windowStart).first<{ count: number }>();
  return Number(row?.count ?? 0) <= limit;
}

export async function insertCapture(id: string, input: CreateCaptureRequest, mediaBaseUrl: string): Promise<void> {
  await ensureSchema();
  const now = new Date().toISOString();
  const coarseLatitude = input.location ? Math.round(input.location.latitude * 100) / 100 : null;
  const coarseLongitude = input.location ? Math.round(input.location.longitude * 100) / 100 : null;
  const verification = JSON.stringify({ ...pendingVerification, deviceSignature: input.assuranceLevel === "web_key" ? "warn" : "pending" });
  await bindings().DB.batch([
    bindings().DB.prepare(`INSERT INTO devices(fingerprint, assurance_level, public_key_spki, attestation_summary, created_at) VALUES(?, ?, ?, '{}', ?) ON CONFLICT(fingerprint) DO UPDATE SET assurance_level = excluded.assurance_level, public_key_spki = excluded.public_key_spki`).bind(input.deviceFingerprint, input.assuranceLevel, input.devicePublicKeySpki, now),
    bindings().DB.prepare(`INSERT INTO captures(id, device_fingerprint, title, status, completeness, assurance_level, started_at, coarse_latitude, coarse_longitude, exact_latitude, exact_longitude, location_accuracy_m, stream_count, verification_summary, media_base_url, created_at, updated_at) VALUES(?, ?, ?, 'initializing', 'pending', ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)`).bind(id, input.deviceFingerprint, input.title?.trim().slice(0, 100) || "Untitled field capture", input.assuranceLevel, input.startedAt, coarseLatitude, coarseLongitude, input.location?.latitude ?? null, input.location?.longitude ?? null, input.location?.accuracyM ?? null, input.streams.length, verification, mediaBaseUrl, now, now),
    ...input.streams.map((stream) => bindings().DB.prepare(`INSERT INTO capture_streams(id, capture_id, role, mime_type, codec, width, height, fps, has_audio) VALUES(?, ?, ?, ?, ?, ?, ?, ?, ?)`).bind(stream.id, id, stream.role, stream.mimeType, stream.codec, stream.width ?? null, stream.height ?? null, stream.fps ?? null, stream.hasAudio ? 1 : 0)),
  ]);
}

export async function registerWithMediaPlane(input: Record<string, unknown>): Promise<void> {
  const { MEDIA_CONTROL_URL, PROOFLINE_INTERNAL_SECRET } = bindings();
  if (!MEDIA_CONTROL_URL) return;
  const response = await fetch(`${MEDIA_CONTROL_URL.replace(/\/$/, "")}/internal/v1/captures`, {
    method: "POST",
    headers: { "content-type": "application/json", "x-proofline-internal-secret": PROOFLINE_INTERNAL_SECRET ?? "" },
    body: JSON.stringify(input),
  });
  if (!response.ok) throw new Error(`Media plane rejected capture registration (${response.status})`);
}

export async function applyLedgerEvent(event: LedgerEvent): Promise<void> {
  await ensureSchema();
  const payload = event.payload;
  const statusByType: Partial<Record<LedgerEvent["type"], CaptureStatus>> = {
    "capture.live": "live", "capture.stalled": "stalled", "capture.sealed": "sealed",
    "capture.interrupted": "interrupted", "capture.tombstoned": "tombstoned",
  };
  const status = statusByType[event.type];
  const completeness = typeof payload.completeness === "string" ? payload.completeness : null;
  const endedAt = typeof payload.ended_at === "string" ? payload.ended_at : null;
  const releaseAt = endedAt ? new Date(Date.parse(endedAt) + 30 * 60_000).toISOString() : null;
  const now = new Date().toISOString();
  await bindings().DB.batch([
    bindings().DB.prepare(`INSERT OR IGNORE INTO ledger_events(id, capture_id, type, occurred_at, payload_json, signature, received_at) VALUES(?, ?, ?, ?, ?, ?, ?)`).bind(event.id, event.captureId, event.type, event.occurredAt, JSON.stringify(payload), event.signature, now),
    bindings().DB.prepare(`UPDATE captures SET status = COALESCE(?, status), completeness = COALESCE(?, completeness), ended_at = COALESCE(?, ended_at), exact_location_release_at = COALESCE(?, exact_location_release_at), last_receipt_at = CASE WHEN ? = 'capture.receipt' THEN ? ELSE last_receipt_at END, duration_ms = COALESCE(?, duration_ms), close_reason = COALESCE(?, close_reason), verification_summary = COALESCE(?, verification_summary), poster_url = COALESCE(?, poster_url), tombstone_reason = COALESCE(?, tombstone_reason), updated_at = ? WHERE id = ?`).bind(status ?? null, completeness, endedAt, releaseAt, event.type, event.occurredAt, payload.duration_ms ?? null, payload.close_reason ?? null, payload.verification ? JSON.stringify(payload.verification) : null, payload.poster_url ?? null, payload.reason ?? null, now, event.captureId),
  ]);
}

export async function hmacHex(secret: string, body: string): Promise<string> {
  const key = await crypto.subtle.importKey("raw", new TextEncoder().encode(secret), { name: "HMAC", hash: "SHA-256" }, false, ["sign"]);
  const signature = await crypto.subtle.sign("HMAC", key, new TextEncoder().encode(body));
  return [...new Uint8Array(signature)].map((byte) => byte.toString(16).padStart(2, "0")).join("");
}

export function constantTimeEqual(a: string, b: string): boolean {
  if (a.length !== b.length) return false;
  let mismatch = 0;
  for (let index = 0; index < a.length; index += 1) mismatch |= a.charCodeAt(index) ^ b.charCodeAt(index);
  return mismatch === 0;
}
