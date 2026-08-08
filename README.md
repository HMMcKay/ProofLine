# ProofLine v2

ProofLine is a public, high-assurance video-provenance system. It uploads signed media fragments while a capture is happening, preserves server receipts and telemetry, and makes deliberate endings visibly different from interrupted recordings.

It does **not** prove that a depicted event is objectively true, guarantee admissibility, or make a compromised phone trustworthy. C2PA and the ProofLine chain establish media bindings and signed provenance assertions; evidentiary weight still depends on device state, custody, operator keys, and jurisdiction.

## What is implemented

- Anonymous public Sites/Vinext control plane with live-first browsing, permanent capture and device pages, evidence dashboards, PWA capture, D1 projections, location-delay handling, and no viewer accounts.
- Rust gateway with signed two-second fragment envelopes, independent byte hashing, durable object writes, signed receipts, continuity enforcement, telemetry batches, SSE status, HLS playlists, recovery supplements, quotas, tombstones, and immutable audit events.
- Rust worker with stalled/interrupted state transitions, PostgreSQL jobs/outbox behavior, deterministic receipt Merkle anchors, optional RFC 3161 requests, and signed JSON/PDF/ZIP evidence artifacts.
- Kotlin/Compose Android 11+ client with one-time public warning, foreground capture service, Android Keystore identity/session keys, CameraX single/attempted concurrent capture, microphone, location and motion telemetry, ordered quality fallback, encrypted bounded queue, receipt handling, and 60-minute cutoff.
- Reproducible official `c2patool` 0.26.60 fragmented-BMFF compatibility fixture. The committed fixture passes video/audio asset-binding validation with trust checking disabled and correctly fails strict trust because it uses an untrusted development credential.
- Docker Compose for PostgreSQL, MinIO, Caddy, gateway, and worker; migrations, health checks, persistent volumes, CI, backup/restore tooling, and an SBOM workflow.

## Repository

```text
app, components, lib, worker/    Sites/Vinext web and D1 worker
android/                         Native Android application
crates/proofline-protocol/       Canonical protocol and crypto primitives
services/media/                  Gateway, worker, report builder, admin CLI
protocol/                        JSON Schema and cross-language vectors
examples/c2pa/                   Executed C2PA/CMAF compatibility fixture
docs/                            Architecture, protocol, threat model, operations
openapi/                         Versioned public HTTP contract
scripts/                         Backup, restore, key, C2PA, and SBOM tools
```

## Local development

Requirements: Node.js 22+, Docker Desktop, JDK 17, Android SDK 36, and FFmpeg. On Windows, Vinext's POSIX scripts need Git Bash:

```powershell
npm ci
$env:npm_config_script_shell = "C:\Program Files\Git\bin\bash.exe"
npm run dev
docker compose up -d --build
```

The web app is at `http://localhost:3000`; the media plane is at `http://localhost:8080`. Local Compose defaults are intentionally development-only. Copy `.env.example` and replace every secret before exposing the service.

Android debug build:

```powershell
cd android
.\gradlew.bat testDebugUnitTest lintDebug assembleDebug
```

To create the deliverable debug APK plus development-signed release APK/AAB and run the clean-payload reproducibility check:

```powershell
.\scripts\build-android-artifacts.ps1
```

Release signing is environment-provided and never committed:

```powershell
$env:PROOFLINE_KEYSTORE_PATH = "C:\secure\proofline-release.jks"
$env:PROOFLINE_KEYSTORE_PASSWORD = "..."
$env:PROOFLINE_KEY_ALIAS = "proofline"
$env:PROOFLINE_KEY_PASSWORD = "..."
.\gradlew.bat assembleRelease bundleRelease
```

## Validation

```powershell
npm run lint
npm run typecheck
npm test
npm run test:built
docker compose -f docker-compose.yml -f docker-compose.test.yml up -d --build
npm run test:media
.\scripts\c2pa-spike.ps1
```

Rust validation runs in the pinned build image or on Rust 1.88:

```powershell
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo build --workspace --release
```

See [validation](docs/VALIDATION.md), [operations](docs/OPERATIONS.md), [protocol](docs/PROTOCOL.md), [architecture](docs/ARCHITECTURE.md), and [threat model](docs/THREAT_MODEL.md). `openapi/proofline-v1.yaml` is the HTTP contract.

## Deployment boundary

The Sites source is bound to the existing ProofLine project in `.openai/hosting.json`. Do not create a duplicate project. The prior private deployment remains the rollback version until this source is validated. Changing Sites access to public and enabling anonymous production capture is a separate, explicit confirmation step.
