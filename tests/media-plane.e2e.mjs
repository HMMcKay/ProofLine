import assert from "node:assert/strict";
import crypto from "node:crypto";
import { mkdtemp, readFile, rm } from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { spawnSync } from "node:child_process";

const gateway = process.env.PROOFLINE_MEDIA_URL ?? "http://localhost:8080";
const internalSecret = process.env.PROOFLINE_INTERNAL_SECRET ?? "proofline-development-secret";
const genesis = "0".repeat(64);

function canonical(value) {
  if (value === null || typeof value !== "object") return JSON.stringify(value);
  if (Array.isArray(value)) return `[${value.map(canonical).join(",")}]`;
  return `{${Object.keys(value).sort().map((key) => `${JSON.stringify(key)}:${canonical(value[key])}`).join(",")}}`;
}
const sha256 = (value) => crypto.createHash("sha256").update(value).digest("hex");
const sign = (key, value) => crypto.sign("sha256", Buffer.from(canonical(value)), { key, dsaEncoding: "ieee-p1363" }).toString("base64url");

async function expectStatus(response, expected) {
  const body = await response.text();
  assert.equal(response.status, expected, `${response.url}: ${body}`);
  return body ? JSON.parse(body) : {};
}

async function waitForReport(captureId, predicate, timeoutMs = 30_000) {
  const started = Date.now();
  while (Date.now() - started < timeoutMs) {
    const response = await fetch(`${gateway}/evidence/v1/${captureId}/report.json`);
    if (response.ok) {
      const report = await response.json();
      if (predicate(report)) return report;
    }
    await new Promise((resolve) => setTimeout(resolve, 1000));
  }
  throw new Error(`Timed out waiting for evidence report ${captureId}`);
}

const directory = await mkdtemp(path.join(os.tmpdir(), "proofline-e2e-"));
try {
  const mediaPath = path.join(directory, "fixture.mp4");
  const ffmpeg = spawnSync("ffmpeg", ["-loglevel", "error", "-f", "lavfi", "-i", "color=c=black:s=320x240:r=30", "-f", "lavfi", "-i", "anullsrc=r=48000:cl=mono", "-t", "2", "-c:v", "libx264", "-pix_fmt", "yuv420p", "-c:a", "aac", "-movflags", "+frag_keyframe+empty_moov+default_base_moof", "-frag_duration", "2000000", "-y", mediaPath], { encoding: "utf8" });
  assert.equal(ffmpeg.status, 0, ffmpeg.stderr);
  const media = await readFile(mediaPath);
  const { publicKey, privateKey } = crypto.generateKeyPairSync("ec", { namedCurve: "prime256v1" });
  const { publicKey: devicePublicKey, privateKey: devicePrivateKey } = crypto.generateKeyPairSync("ec", { namedCurve: "prime256v1" });
  const spki = publicKey.export({ type: "spki", format: "der" }).toString("base64url");
  const deviceSpki = devicePublicKey.export({ type: "spki", format: "der" }).toString("base64url");
  const captureId = `cap_e2e_${crypto.randomUUID().replaceAll("-", "")}`;
  const streamId = `rear_e2e_${crypto.randomUUID().replaceAll("-", "")}`;
  const deviceFingerprint = `e2e-device-${crypto.randomUUID().replaceAll("-", "")}`;
  const token = crypto.randomBytes(32).toString("base64url");
  await expectStatus(await fetch(`${gateway}/internal/v1/captures`, { method: "POST", headers: { "content-type": "application/json", "x-proofline-internal-secret": internalSecret }, body: JSON.stringify({ capture_id: captureId, device_fingerprint: deviceFingerprint, assurance_level: "web_key", session_public_key_spki: spki, device_public_key_spki: deviceSpki, session_binding_signature: "verified-by-control-plane-test-fixture", upload_token: token, started_at: new Date().toISOString(), streams: [{ id: streamId, role: "rear_video", mime_type: "video/mp4", codec: "avc1+aac" }] }) }), 200);

  const makeEnvelope = (sequence, previous) => {
    const input = { protocol_version: "proofline/2", capture_id: captureId, stream_id: streamId, sequence, previous_chain_digest: previous, media_digest: sha256(media), byte_length: media.length, pts_start_us: sequence * 2_000_000, pts_end_us: (sequence + 1) * 2_000_000, telemetry_root: genesis };
    return { ...input, chain_digest: sha256(canonical(input)) };
  };
  const upload = (envelope, bytes = media, signature = sign(privateKey, envelope)) => fetch(`${gateway}/ingest/v1/${captureId}/${streamId}/${envelope.sequence}`, { method: "PUT", headers: { authorization: `Bearer ${token}`, "content-type": "video/mp4", "x-proofline-envelope": Buffer.from(canonical(envelope)).toString("base64url"), "x-proofline-signature": signature }, body: bytes });

  const first = makeEnvelope(0, genesis);
  await expectStatus(await upload(first, media, sign(privateKey, { ...first, sequence: 9 })), 422);
  const firstReceipt = await expectStatus(await upload(first), 200);
  const duplicateReceipt = await expectStatus(await upload(first), 200);
  assert.deepEqual(duplicateReceipt, firstReceipt);

  const reordered = makeEnvelope(2, first.chain_digest);
  await expectStatus(await upload(reordered), 409);
  const second = makeEnvelope(1, first.chain_digest);
  const mutated = Buffer.from(media); mutated[mutated.length - 1] ^= 1;
  await expectStatus(await upload(second, mutated), 422);
  await expectStatus(await upload(second), 200);

  const manifest = { protocolVersion: "proofline/2", captureId, endedAt: new Date().toISOString(), durationMs: 4000, closeReason: "user_stop", streams: [{ id: streamId, sequenceCount: 2, finalChainDigest: second.chain_digest }] };
  await expectStatus(await fetch(`${gateway}/ingest/v1/${captureId}/end`, { method: "POST", headers: { authorization: `Bearer ${token}`, "content-type": "application/json" }, body: JSON.stringify({ manifest, signature: sign(privateKey, manifest) }) }), 422);
  const ending = await expectStatus(await fetch(`${gateway}/ingest/v1/${captureId}/end`, { method: "POST", headers: { authorization: `Bearer ${token}`, "content-type": "application/json" }, body: JSON.stringify({ manifest, signature: sign(devicePrivateKey, manifest) }) }), 200);
  assert.equal(ending.completeness, "complete_with_signed_end");
  const report = await expectStatus(await fetch(`${gateway}/evidence/v1/${captureId}/report.json`), 200);
  assert.equal(report.fragment_count, 2);
  assert.equal(report.completeness, "complete_with_signed_end");
  assert.equal((await fetch(`${gateway}/evidence/v1/${captureId}/bundle.zip`)).status, 200);
  const playlist = await (await fetch(`${gateway}/live/v1/${captureId}/${streamId}/index.m3u8`)).text();
  assert.match(playlist, /#EXT-X-ENDLIST/);

  const interruptedCaptureId = `cap_interrupted_${crypto.randomUUID().replaceAll("-", "")}`;
  const interruptedStreamId = `rear_interrupted_${crypto.randomUUID().replaceAll("-", "")}`;
  const interruptedToken = crypto.randomBytes(32).toString("base64url");
  await expectStatus(await fetch(`${gateway}/internal/v1/captures`, { method: "POST", headers: { "content-type": "application/json", "x-proofline-internal-secret": internalSecret }, body: JSON.stringify({ capture_id: interruptedCaptureId, device_fingerprint: `interrupted-device-${crypto.randomUUID()}`, assurance_level: "web_key", session_public_key_spki: spki, session_binding_signature: "verified-by-control-plane-test-fixture", upload_token: interruptedToken, started_at: new Date().toISOString(), streams: [{ id: interruptedStreamId, role: "rear_video", mime_type: "video/mp4", codec: "avc1+aac" }] }) }), 200);
  const makeInterruptedEnvelope = (sequence, previous) => {
    const input = { protocol_version: "proofline/2", capture_id: interruptedCaptureId, stream_id: interruptedStreamId, sequence, previous_chain_digest: previous, media_digest: sha256(media), byte_length: media.length, pts_start_us: sequence * 2_000_000, pts_end_us: (sequence + 1) * 2_000_000, telemetry_root: genesis };
    return { ...input, chain_digest: sha256(canonical(input)) };
  };
  const uploadInterrupted = (envelope) => fetch(`${gateway}/ingest/v1/${interruptedCaptureId}/${interruptedStreamId}/${envelope.sequence}`, { method: "PUT", headers: { authorization: `Bearer ${interruptedToken}`, "content-type": "video/mp4", "x-proofline-envelope": Buffer.from(canonical(envelope)).toString("base64url"), "x-proofline-signature": sign(privateKey, envelope) }, body: media });
  const interruptedFirst = makeInterruptedEnvelope(0, genesis);
  await expectStatus(await uploadInterrupted(interruptedFirst), 200);
  const age = spawnSync("docker", ["compose", "exec", "-T", "postgres", "psql", "-U", "proofline", "-d", "proofline", "-c", `UPDATE fragments SET server_received_at=now()-interval '16 minutes' WHERE capture_id='${interruptedCaptureId}'; UPDATE captures SET started_at=now()-interval '16 minutes',updated_at=now()-interval '16 minutes' WHERE id='${interruptedCaptureId}'`], { encoding: "utf8" });
  assert.equal(age.status, 0, age.stderr);
  const interruptedReport = await waitForReport(interruptedCaptureId, (value) => value.status === "interrupted");
  assert.equal(interruptedReport.completeness, "complete_as_server_received");
  assert.equal(interruptedReport.fragment_count, 1);
  assert.ok(interruptedReport.audit_events.some((event) => event.event_type === "capture.interrupted"));

  const interruptedSecond = makeInterruptedEnvelope(1, interruptedFirst.chain_digest);
  await expectStatus(await uploadInterrupted(interruptedSecond), 200);
  const recoveredReport = await waitForReport(interruptedCaptureId, (value) => value.fragment_count === 2 && value.audit_events.some((event) => event.event_type === "capture.recovery"));
  assert.equal(recoveredReport.status, "interrupted");
  assert.equal(recoveredReport.completeness, "complete_as_server_received");

  const adminFixture = JSON.parse(await readFile(new URL("./fixtures/admin-test-key.json", import.meta.url), "utf8"));
  const adminPrivateKey = crypto.createPrivateKey({ key: Buffer.from(adminFixture.private_key_pkcs8_b64url, "base64url"), type: "pkcs8", format: "der" });
  const tombstoneAction = { captureId, reason: "Automated test: public playback suppression", issuedAt: new Date().toISOString(), nonce: crypto.randomUUID() };
  const tombstoneUrl = `${gateway}/internal/v1/tombstones`;
  await expectStatus(await fetch(tombstoneUrl, { method: "POST", headers: { "content-type": "application/json", "x-proofline-internal-secret": internalSecret, "x-proofline-admin-signature": sign(privateKey, tombstoneAction) }, body: JSON.stringify(tombstoneAction) }), 401);
  await expectStatus(await fetch(tombstoneUrl, { method: "POST", headers: { "content-type": "application/json", "x-proofline-internal-secret": internalSecret, "x-proofline-admin-signature": sign(adminPrivateKey, tombstoneAction) }, body: JSON.stringify(tombstoneAction) }), 200);
  assert.equal((await fetch(`${gateway}/live/v1/${captureId}/${streamId}/index.m3u8`)).status, 410);
  assert.equal((await fetch(`${gateway}/evidence/v1/${captureId}/original/${streamId}`)).status, 410);
  assert.equal((await fetch(`${gateway}/evidence/v1/${captureId}/bundle.zip`)).status, 410);
  const tombstoneReport = await expectStatus(await fetch(`${gateway}/evidence/v1/${captureId}/report.json`), 200);
  assert.equal(tombstoneReport.status, "tombstoned");
  assert.equal(tombstoneReport.tombstone.reason, tombstoneAction.reason);

  console.log(JSON.stringify({ captureId, streamId, fragments: 2, forgedRejected: true, reorderingRejected: true, mutationRejected: true, sessionSignedEndRejected: true, completeness: ending.completeness, interruptedCaptureId, interruptedRecovery: true, tombstoneRetainedReport: true }));
} finally {
  await rm(directory, { recursive: true, force: true });
}
