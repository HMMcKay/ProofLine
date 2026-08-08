CREATE TABLE IF NOT EXISTS captures (
  id TEXT PRIMARY KEY,
  device_fingerprint TEXT NOT NULL,
  assurance_level TEXT NOT NULL,
  session_public_key_spki TEXT NOT NULL,
  session_binding_signature TEXT NOT NULL,
  upload_token_hash TEXT NOT NULL,
  started_at TIMESTAMPTZ NOT NULL,
  ended_at TIMESTAMPTZ,
  status TEXT NOT NULL DEFAULT 'initializing',
  completeness TEXT NOT NULL DEFAULT 'pending',
  close_reason TEXT,
  final_manifest JSONB,
  final_signature TEXT,
  finalized_at TIMESTAMPTZ,
  tombstone_reason TEXT,
  created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
  updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS idx_media_capture_status_updated ON captures(status, updated_at);

CREATE TABLE IF NOT EXISTS streams (
  id TEXT PRIMARY KEY,
  capture_id TEXT NOT NULL REFERENCES captures(id),
  role TEXT NOT NULL,
  mime_type TEXT NOT NULL,
  codec TEXT NOT NULL,
  last_sequence BIGINT NOT NULL DEFAULT -1,
  last_chain_digest TEXT NOT NULL DEFAULT repeat('0', 64),
  byte_length BIGINT NOT NULL DEFAULT 0,
  UNIQUE(capture_id, role)
);
CREATE INDEX IF NOT EXISTS idx_media_stream_capture ON streams(capture_id);

CREATE TABLE IF NOT EXISTS fragments (
  capture_id TEXT NOT NULL REFERENCES captures(id),
  stream_id TEXT NOT NULL REFERENCES streams(id),
  sequence BIGINT NOT NULL,
  previous_chain_digest TEXT NOT NULL,
  media_digest TEXT NOT NULL,
  chain_digest TEXT NOT NULL,
  byte_length BIGINT NOT NULL,
  pts_start_us BIGINT NOT NULL,
  pts_end_us BIGINT NOT NULL,
  telemetry_root TEXT NOT NULL,
  device_signature TEXT NOT NULL,
  object_key TEXT NOT NULL,
  object_version TEXT NOT NULL,
  server_received_at TIMESTAMPTZ NOT NULL,
  receipt JSONB NOT NULL,
  receipt_signature TEXT NOT NULL,
  PRIMARY KEY(capture_id, stream_id, sequence)
);
CREATE INDEX IF NOT EXISTS idx_fragments_capture_stream ON fragments(capture_id, stream_id, sequence);

CREATE TABLE IF NOT EXISTS telemetry_batches (
  capture_id TEXT NOT NULL REFERENCES captures(id),
  sequence BIGINT NOT NULL,
  digest TEXT NOT NULL,
  previous_digest TEXT NOT NULL,
  device_signature TEXT NOT NULL,
  payload JSONB NOT NULL,
  received_at TIMESTAMPTZ NOT NULL DEFAULT now(),
  PRIMARY KEY(capture_id, sequence)
);

CREATE TABLE IF NOT EXISTS evidence_events (
  id UUID PRIMARY KEY,
  capture_id TEXT NOT NULL REFERENCES captures(id),
  event_type TEXT NOT NULL,
  occurred_at TIMESTAMPTZ NOT NULL,
  payload JSONB NOT NULL,
  server_signature TEXT NOT NULL,
  delivered_at TIMESTAMPTZ
);
CREATE INDEX IF NOT EXISTS idx_evidence_events_outbox ON evidence_events(delivered_at, occurred_at);

CREATE TABLE IF NOT EXISTS tombstones (
  capture_id TEXT PRIMARY KEY REFERENCES captures(id),
  reason TEXT NOT NULL,
  action_json JSONB NOT NULL,
  admin_signature TEXT NOT NULL,
  created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

