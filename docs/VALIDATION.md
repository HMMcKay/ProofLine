# Validation record

This file separates executable evidence from work that still needs production infrastructure or physical hardware. A green emulator, synthetic fixture, or local container is never represented as a physical-device result.

## Passed locally on 2026-08-07/08

- Sites/Vinext: ESLint, TypeScript `--noEmit`, Node protocol vectors, server-rendered route assertions, and production build.
- Browser: public homepage, protocol boundary, first-launch capture warning, desktop layout, and 390 x 844 responsive layout in the in-app browser. The local evidence ledger was intentionally empty; synthetic captures were not shown as public evidence.
- Rust 1.88: formatting check, Clippy for all targets with warnings denied, workspace unit/doc tests, and optimized workspace build.
- Containers: digest-pinned gateway/worker build, migrations, health checks, PostgreSQL, MinIO, Caddy, and the media-plane end-to-end scenario.
- Media-plane scenario: forged signature, reordered sequence, mutated bytes, duplicate retry, session-key-signed ending, interrupted prefix, late recovery supplement, invalid tombstone signature, valid offline-admin tombstone, retained report, and hidden tombstoned media.
- C2PA spike: official `c2patool` 0.26.60 authored and validated separate AVC and AAC CMAF fragment sets. Asset binding passes with trust disabled; strict validation correctly reports the bundled development credential as untrusted.
- Android: JVM protocol-vector test, lint, debug APK, and compiled Compose instrumentation-test APK. Release APK/AAB packaging uses an ephemeral development key and compares two clean runtime payloads; signing containers and R8's `buildTimeNs` diagnostic are reported separately because they are not byte-stable.
- PDF: an interrupted-capture report was rendered with Poppler and visually inspected. Long keys/hashes are wrapped, critical warnings appear first, and the QR permalink remains clear.
- Operations: a CycloneDX SBOM was generated (362 components), then the disposable PostgreSQL and MinIO state was backed up, restored over the running local stack, and returned to healthy service state.

## Not verified here

- StrongBox/TEE attestation chain, Google revocation status, verified boot/root-of-trust fields, and production package-signature allowlisting. The media service fails closed for certificate-bearing Android enrollment unless `PROOFLINE_ANDROID_ATTESTATION_VERIFIER_URL` points to an operator-supplied verifier; its absence is never promoted to a hardware assurance label.
- A real phone's camera, microphone, GNSS, barometer, inertial/camera timebase calibration, thermal shutdown behavior, process death, storage pressure, or concurrent front/rear cameras.
- Execution of the Compose instrumentation test on an emulator or phone. The test APK compiled; it was not run on a target in this session.
- Continuous Media3-authored CMAF from Android. The current CameraX implementation emits independently playable, signed two-second ISO BMFF assets. Accordingly the public report says C2PA live binding and final-asset C2PA generation are unsupported for current capture output.
- Server-generated poster/sprite extraction and hover/stylus scrubbing. Capture cards support static posters from the ledger, but the current worker does not create preview sprites.
- Production TSA validation, production C2PA trust chain, weather correlation adapters, KMS/HSM/PKCS#11, or a hostile-server independent witness.
- VPS deployment, public DNS/TLS, real production quotas/capacity alerts, and backup restore on a separate host.
- Public Sites access or anonymous production capture. The bound project remains private pending the user's explicit go-live confirmation.

## Physical-device acceptance matrix

For each target device, record model, OS build, security patch, camera IDs, reported concurrent combinations, keystore security level, attestation validation result, sensor availability, 15/30/60 minute thermal behavior, offline/reconnect behavior, and hashes of exported evidence bundles. Test at least one StrongBox device, one TEE-only device, and one device without concurrent-camera support before treating native capture as production-ready.

## Commands

```powershell
$env:npm_config_script_shell = "C:\Program Files\Git\bin\bash.exe"
npm run lint
npm run typecheck
npm test
npm run test:built

docker run --rm -v "${PWD}:/work" -w /work rust:1.88.0-bookworm `
  sh -c "cargo fmt --all -- --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace && cargo build --workspace --release"

docker compose -f docker-compose.yml -f docker-compose.test.yml up -d --build --wait
npm run test:media
.\scripts\c2pa-spike.ps1
.\scripts\build-android-artifacts.ps1
```
