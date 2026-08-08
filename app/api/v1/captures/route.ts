import { bindings, consumeRate, ensureSchema, hmacHex, insertCapture, json, listCaptures, readJson, registerWithMediaPlane } from "../../../../lib/control-plane";
import { canonicalize, publicKeyFingerprint, type CreateCaptureRequest, type CreateCaptureResponse, type SessionBinding } from "../../../../lib/protocol";

const idPattern = /^[a-zA-Z0-9_-]{8,96}$/;

export async function GET(request: Request) {
  const url = new URL(request.url);
  return json({ captures: await listCaptures({ status: url.searchParams.get("status") ?? "recent", limit: Number(url.searchParams.get("limit") ?? 48) }) }, 200, { "cache-control": "public, max-age=5, stale-while-revalidate=15" });
}

export async function POST(request: Request) {
  try {
    const input = await readJson<CreateCaptureRequest>(request);
    await ensureSchema();
    if (!idPattern.test(input.deviceFingerprint) || input.streams.length < 1 || input.streams.length > 3) return json({ error: "Invalid device or stream declaration" }, 400);
    if (input.deviceFingerprint !== await publicKeyFingerprint(input.devicePublicKeySpki)) return json({ error: "Device fingerprint does not match its public key" }, 400);
    const [challenge, enrolled] = await Promise.all([
      bindings().DB.prepare("SELECT nonce, expires_at FROM attestation_challenges WHERE nonce = ? AND used_at IS NULL").bind(input.sessionNonce).first<{ nonce: string; expires_at: string }>(),
      bindings().DB.prepare("SELECT assurance_level, public_key_spki FROM devices WHERE fingerprint = ?").bind(input.deviceFingerprint).first<{ assurance_level: string; public_key_spki: string }>(),
    ]);
    if (!challenge || Date.parse(challenge.expires_at) < Date.now()) return json({ error: "Session nonce is invalid or expired" }, 400);
    if (!enrolled || enrolled.public_key_spki !== input.devicePublicKeySpki || enrolled.assurance_level !== input.assuranceLevel) return json({ error: "Device enrollment or assurance level does not match" }, 403);
    const binding: SessionBinding = { protocolVersion: "proofline/2", challenge: input.sessionNonce, deviceFingerprint: input.deviceFingerprint, sessionPublicKeySpki: input.sessionPublicKeySpki, startedAt: input.startedAt, streams: input.streams };
    const publicKey = await crypto.subtle.importKey("spki", base64UrlBytes(input.devicePublicKeySpki), { name: "ECDSA", namedCurve: "P-256" }, false, ["verify"]);
    const signatureValid = await crypto.subtle.verify({ name: "ECDSA", hash: "SHA-256" }, publicKey, base64UrlBytes(input.sessionBindingSignature), new TextEncoder().encode(canonicalize(binding)));
    if (!signatureValid) return json({ error: "Session binding signature is invalid" }, 403);
    const activeDevice = await bindings().DB.prepare("SELECT count(*) AS count FROM captures WHERE device_fingerprint = ? AND status IN ('initializing','live','stalled')").bind(input.deviceFingerprint).first<{ count: number }>();
    if (Number(activeDevice?.count ?? 0) > 0) return json({ error: "Device already has an active capture" }, 409);
    const ip = request.headers.get("cf-connecting-ip") ?? "local-development";
    const internalSecret = bindings().PROOFLINE_INTERNAL_SECRET ?? "local-development-only";
    const originIpHash = await hmacHex(internalSecret, `origin-ip:${ip}`);
    const ipAllowed = await consumeRate(`capture-ip:${originIpHash}`, input.assuranceLevel === "web_key" ? 6 : 12);
    if (!ipAllowed) return json({ error: "Capture creation rate limit exceeded" }, 429);
    const captureId = `cap_${crypto.randomUUID().replace(/-/g, "")}`;
    const uploadBytes = crypto.getRandomValues(new Uint8Array(32));
    const uploadToken = btoa(String.fromCharCode(...uploadBytes)).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");
    const mediaBaseUrl = bindings().MEDIA_PUBLIC_URL ?? "http://localhost:8080";
    await bindings().DB.prepare("UPDATE attestation_challenges SET used_at = ? WHERE nonce = ? AND used_at IS NULL").bind(new Date().toISOString(), input.sessionNonce).run();
    await insertCapture(captureId, input, mediaBaseUrl);
    try {
      await registerWithMediaPlane({ capture_id: captureId, device_fingerprint: input.deviceFingerprint, assurance_level: input.assuranceLevel, session_public_key_spki: input.sessionPublicKeySpki, device_public_key_spki: input.devicePublicKeySpki, session_binding_signature: input.sessionBindingSignature, streams: input.streams.map((stream) => ({ id: stream.id, role: stream.role, mime_type: stream.mimeType, codec: stream.codec })), upload_token: uploadToken, started_at: input.startedAt, origin_ip_hash: originIpHash });
    } catch (error) {
      await bindings().DB.prepare("DELETE FROM captures WHERE id = ?").bind(captureId).run();
      return json({ error: error instanceof Error ? error.message : "Media plane is unavailable" }, 503);
    }
    const response: CreateCaptureResponse = { captureId, uploadToken, mediaBaseUrl, expiresAt: new Date(Date.now() + 75 * 60_000).toISOString(), maxDurationSeconds: 3600, fragmentDurationMs: 2000 };
    return json(response, 201);
  } catch (error) {
    return json({ error: error instanceof Error ? error.message : "Capture creation failed" }, 400);
  }
}

function base64UrlBytes(value: string): ArrayBuffer {
  const normalized = value.replace(/-/g, "+").replace(/_/g, "/");
  const binary = atob(normalized.padEnd(Math.ceil(normalized.length / 4) * 4, "="));
  const buffer = new ArrayBuffer(binary.length);
  const view = new Uint8Array(buffer);
  for (let index = 0; index < binary.length; index += 1) view[index] = binary.charCodeAt(index);
  return buffer;
}
