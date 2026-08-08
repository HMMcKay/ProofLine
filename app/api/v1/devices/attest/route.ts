import { bindings, consumeRate, ensureSchema, json, readJson } from "../../../../../lib/control-plane";
import { publicKeyFingerprint, type AssuranceLevel } from "../../../../../lib/protocol";

type AttestRequest = { phase: "challenge" } | { phase: "verify"; publicKeySpki: string; certificateChain?: string[]; challenge: string; claimedAssurance?: AssuranceLevel };

export async function POST(request: Request) {
  try {
    const ip = request.headers.get("cf-connecting-ip") ?? "local";
    if (!(await consumeRate(`attest:${ip}`, 20))) return json({ error: "Attestation rate limit exceeded" }, 429);
    const input = await readJson<AttestRequest>(request);
    await ensureSchema();
    if (input.phase === "challenge") {
      const nonceBytes = crypto.getRandomValues(new Uint8Array(32));
      const nonce = btoa(String.fromCharCode(...nonceBytes)).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");
      const createdAt = new Date();
      const expiresAt = new Date(createdAt.getTime() + 5 * 60_000);
      await bindings().DB.prepare("INSERT INTO attestation_challenges(nonce, created_at, expires_at) VALUES(?, ?, ?)").bind(nonce, createdAt.toISOString(), expiresAt.toISOString()).run();
      return json({ challenge: nonce, expiresAt: expiresAt.toISOString() }, 201);
    }
    const challenge = await bindings().DB.prepare("SELECT * FROM attestation_challenges WHERE nonce = ? AND used_at IS NULL").bind(input.challenge).first<Record<string, unknown>>();
    if (!challenge || Date.parse(String(challenge.expires_at)) < Date.now()) return json({ error: "Challenge is invalid or expired" }, 400);
    const fingerprint = await publicKeyFingerprint(input.publicKeySpki);
    let assurance: AssuranceLevel = "web_key";
    let summary: Record<string, unknown> = { verified: false, reason: "Browser keys have no Android hardware attestation" };
    if (input.certificateChain?.length) {
      const controlUrl = bindings().MEDIA_CONTROL_URL;
      if (!controlUrl) return json({ error: "Hardware attestation verifier is unavailable" }, 503);
      const response = await fetch(`${controlUrl.replace(/\/$/, "")}/internal/v1/attest`, { method: "POST", headers: { "content-type": "application/json", "x-proofline-internal-secret": bindings().PROOFLINE_INTERNAL_SECRET ?? "" }, body: JSON.stringify(input) });
      if (!response.ok) return json({ error: "Hardware attestation was rejected" }, 400);
      const result = await response.json() as { assuranceLevel: AssuranceLevel; summary: Record<string, unknown> };
      assurance = result.assuranceLevel; summary = result.summary;
    }
    await bindings().DB.prepare("UPDATE attestation_challenges SET used_at = ? WHERE nonce = ?").bind(new Date().toISOString(), input.challenge).run();
    await bindings().DB.prepare(`INSERT INTO devices(fingerprint, assurance_level, public_key_spki, attestation_summary, created_at) VALUES(?, ?, ?, ?, ?) ON CONFLICT(fingerprint) DO UPDATE SET assurance_level = excluded.assurance_level, attestation_summary = excluded.attestation_summary`).bind(fingerprint, assurance, input.publicKeySpki, JSON.stringify(summary), new Date().toISOString()).run();
    return json({ fingerprint, assuranceLevel: assurance, summary });
  } catch (error) {
    return json({ error: error instanceof Error ? error.message : "Attestation failed" }, 400);
  }
}
