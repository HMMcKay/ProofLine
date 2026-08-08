# Threat model and claims

## What ProofLine can establish

- The exported bytes match the fragment digests, signed session chain, and durable server receipts in the bundle.
- A displayed start is the first encoded frame in the app-signed chain. This is not a universal sensor-to-signature guarantee.
- A sealed ending covers the declared accepted tracks, or an interrupted ending is the highest contiguous prefix the server accepted before the resume window expired.
- Attestation assurance, if verified, describes the signing key and measured device/app state at enrollment.
- Signed telemetry, receipt time, TSA responses, and public-data correlations can corroborate time and place.

## What remains unknowable or attackable

- A staged scene, coerced operator, camera pointed at a screen, deepfake presented to the lens, or event outside the field of view.
- A rooted or compromised device lying before encoding, sensor spoofing, malicious camera HAL/firmware, stolen unlocked signing keys, or a vulnerable trusted execution environment.
- Frames the sensor produced but the app never encoded, and frames encoded locally after the last fragment the server received.
- Exact clock truth from the phone alone. Wall time is corroborating data; monotonic timestamps and server/TSA observations are reported separately.
- A malicious operator controlling the server and its signing key. Independent downloads, RFC 3161 anchors, public audit replication, and external witnesses reduce but do not eliminate this trust.
- Legal admissibility, authenticity findings, or evidentiary weight. Those are jurisdiction- and case-specific.

## Abuse and privacy

All captures are irrevocably public by product design. Exact location is retained immediately, shown at roughly one-kilometer precision while live, and released 30 minutes after capture ends. That delayed release still creates serious safety risk; the first-launch warning must be explicit.

The media plane accepts only protocol-created capabilities, not arbitrary upload forms. Limits apply to active captures, IP/device concurrency, fragment size, capture duration, and daily bytes. Blocklists, capacity checks, request IDs, audit logs, and fail-closed storage behavior are operator controls.

Tombstoning is for safety, illegality, and abuse response. It does not erase originals or history: playback and downloads are suppressed while the signed offline action, reason, hashes, receipt metadata, and report remain public.
