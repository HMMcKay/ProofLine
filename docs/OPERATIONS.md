# Operations

## Production topology

Use one HTTPS VPS with Caddy, PostgreSQL, S3-compatible object storage, gateway, and worker. Point the Sites bindings at `MEDIA_CONTROL_URL`, `MEDIA_PUBLIC_URL`, and a matching `PROOFLINE_INTERNAL_SECRET`. Restrict `/internal/` at the network layer in addition to HMAC/capability checks.

Copy `.env.example`; replace every value. Generate the ES256 receipt key and offline admin key on an encrypted administrator workstation. Mount the receipt private key read-only or inject its PKCS#8 base64url value through the host secret manager. Configure only the admin **public** SPKI on the VPS. Never place either private key in D1 or PostgreSQL.

Production C2PA requires an operator certificate and chain accepted by the intended trust list. The committed development credential is deliberately untrusted. Prefer an HSM/KMS subprocess signer; private keys in C2PA settings are for development only.

## TLS and proxying

Replace `:80` in `Caddyfile` with the media hostname and email, then expose only 80/443. Caddy is the sole trusted proxy. Do not honor client-supplied forwarding headers from any other source. Set object storage and PostgreSQL ports to internal Docker networks only.

## Start and observe

```powershell
docker compose config
docker compose up -d --build --wait
Invoke-RestMethod https://media.example/healthz
Invoke-RestMethod https://media.example/readyz
Invoke-WebRequest https://media.example/metrics
docker compose logs -f gateway worker
```

For local development, `scripts/start-local.ps1` calls `scripts/prepare-local-media-plane.ps1` before starting the full stack. The helper retains a development receipt key under ignored `private/` storage and reconciles the existing PostgreSQL role with the current Compose password through the trusted container-local socket. This addresses password rotation without deleting the persistent volume. Production deployments must instead use their secret manager and an explicit, audited database credential-rotation procedure.

Alert on readiness failure, object-write failure, PostgreSQL saturation, disk/object capacity, certificate expiry, repeated signature failures, outbox backlog, TSA failure, and captures stuck beyond the resume window. Metrics are Prometheus text; logs are structured JSON with request IDs.

## Offline tombstone

Run the admin CLI from an offline-controlled workstation or a short-lived administration host. A key generation pattern with a mounted encrypted directory is:

```powershell
docker run --rm -v "C:\encrypted\proofline-keys:/keys" --entrypoint proofline-admin proofline-gateway keygen --private-key-out /keys/admin-private-key.txt
```

Configure the printed public SPKI on the gateway, then tombstone:

```powershell
$env:PROOFLINE_INTERNAL_SECRET = "..."
docker run --rm -v "C:\encrypted\proofline-keys:/keys:ro" --network host --entrypoint proofline-admin proofline-gateway tombstone --gateway https://media.example --capture cap_... --reason "Published safety policy reason" --private-key /keys/admin-private-key.txt
```

The action is canonicalized and signed offline. Playback and raw downloads become HTTP 410; report metadata and the signed action remain.

## Backup and restore

`scripts/backup.ps1 -OutputDirectory D:\proofline-backups` captures PostgreSQL and MinIO. Separately back up receipt/C2PA/TLS keys and the offline admin key from their encrypted sources. Keep at least one encrypted off-site copy.

Test restore on a disconnected clone:

```powershell
.\scripts\restore.ps1 -BackupDirectory D:\proofline-backups\proofline-YYYYMMDD-HHMMSS -ConfirmRestore RESTORE-PROOFLINE
npm run test:media
```

Restore deliberately stops ingest and replaces the exact `/data` MinIO target. Do not run it against a live public service.

## Sites release

`.openai/hosting.json` is bound to the existing ProofLine project. Build and validate locally, save the current private version as rollback, then deploy this source to that project. Do not change access while validating. The final transition to public access and anonymous capture requires explicit user confirmation immediately before the switch.

## Known validation boundary

Emulator checks do not validate StrongBox/TEE attestation, real thermal behavior, concurrent physical cameras, microphone timing, GNSS, barometer availability, or camera/realtime timestamp compatibility. Those require the physical-device matrix in `docs/VALIDATION.md` and must remain marked unverified until run.
