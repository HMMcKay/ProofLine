# Evidence protocol

The schema authority is `protocol/schema/proofline-v2.schema.json`; the committed vector in `protocol/test-vectors` is executed by TypeScript, Rust, and Kotlin tests. Signed JSON uses RFC 8785-compatible canonical serialization and ES256/P-1363 signatures.

For each exact fragment:

```text
media_digest = SHA256(exact_fragment_bytes)
chain_digest = SHA256(JCS({
  protocol_version, capture_id, stream_id, sequence,
  previous_chain_digest, media_digest, byte_length,
  pts_start_us, pts_end_us, telemetry_root
}))
```

The gateway verifies route identity, capability, session public key, envelope signature, MIME/container shape, size, media digest, chain digest, predecessor, sequence, PTS policy, per-capture/device quotas, and durable object storage before issuing a signed receipt. Duplicate identical fragments return the original receipt; conflicting duplicates are rejected and remain observable in logs.

## Identity levels

- `strongbox`: attestation verified and key is StrongBox-backed.
- `tee`: attestation verified and key is TEE-backed.
- `software_attested`: a valid attestation explicitly reports software security.
- `web_key`: browser-generated key, or a deliberately visible Android development fallback when no attestation verifier is configured.

IMEI, serial number, MAC address, and Android ID are not used as provenance identity. The public identifier is the base32 SHA-256 fingerprint of SPKI. Uninstalling before a signed rotation loses key continuity.

## Media and audio

The protocol binds exact fragment bytes and therefore all tracks present in the BMFF object. Track-specific roots in reports let a verifier additionally distinguish audio, video, and telemetry results. Shared microphone capture is declared once and linked to both camera streams; UI synchronization is not itself evidence.

The Android production target is H.264/AVC plus AAC-LC in two-second CMAF fragments. The current native capture implementation records independently decodable two-second MP4 segments through CameraX while the committed Media3/CMAF and official C2PA fixture proves the desired container path separately. This limitation is reported rather than relabeled as live C2PA.

## Endings and recovery

A valid end manifest names every declared stream, its exact count and last digest, duration, end time, and one of `user_stop`, `duration_limit`, `permission_revoked`, `thermal_shutdown`, or `app_error`. A mismatch yields `gaps_detected`.

After the 15-minute continuation window, the received prefix is finalized as `interrupted`. A later valid next fragment is stored as a signed recovery supplement, emits a new audit event, and regenerates the report without changing the original interrupted assertion or any prior receipt.

## C2PA status

`scripts/c2pa-spike.ps1` pins official `c2patool` 0.26.60, generates AVC and AAC CMAF renditions, authors fragmented-BMFF manifests, validates their asset bindings, and separately confirms strict trust rejection of the bundled development credential. Production trust requires an operator-supplied certificate chain and an independently installed official validator. Proprietary sidecars are never called C2PA.
