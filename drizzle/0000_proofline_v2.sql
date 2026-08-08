CREATE TABLE IF NOT EXISTS devices (
  fingerprint TEXT PRIMARY KEY NOT NULL,
  assurance_level TEXT NOT NULL,
  public_key_spki TEXT NOT NULL,
  attestation_summary TEXT NOT NULL DEFAULT '{}',
  created_at TEXT NOT NULL,
  rotated_from TEXT
);
CREATE TABLE IF NOT EXISTS captures (
  id TEXT PRIMARY KEY NOT NULL,
  device_fingerprint TEXT NOT NULL,
  title TEXT NOT NULL,
  status TEXT NOT NULL,
  completeness TEXT NOT NULL,
  assurance_level TEXT NOT NULL,
  started_at TEXT NOT NULL,
  ended_at TEXT,
  last_receipt_at TEXT,
  exact_location_release_at TEXT,
  coarse_latitude REAL,
  coarse_longitude REAL,
  exact_latitude REAL,
  exact_longitude REAL,
  location_accuracy_m REAL,
  stream_count INTEGER NOT NULL DEFAULT 1,
  duration_ms INTEGER NOT NULL DEFAULT 0,
  close_reason TEXT,
  verification_summary TEXT NOT NULL DEFAULT '{}',
  media_base_url TEXT NOT NULL,
  poster_url TEXT,
  tombstone_reason TEXT,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_captures_started_at ON captures(started_at);
CREATE INDEX IF NOT EXISTS idx_captures_status_started_at ON captures(status, started_at);
CREATE INDEX IF NOT EXISTS idx_captures_device_started_at ON captures(device_fingerprint, started_at);
CREATE TABLE IF NOT EXISTS capture_streams (
  id TEXT PRIMARY KEY NOT NULL,
  capture_id TEXT NOT NULL,
  role TEXT NOT NULL,
  mime_type TEXT NOT NULL,
  codec TEXT NOT NULL,
  width INTEGER,
  height INTEGER,
  fps REAL,
  has_audio INTEGER NOT NULL DEFAULT 0,
  sequence_count INTEGER NOT NULL DEFAULT 0,
  final_chain_digest TEXT
);
CREATE UNIQUE INDEX IF NOT EXISTS uq_capture_stream_role ON capture_streams(capture_id, role);
CREATE INDEX IF NOT EXISTS idx_capture_stream_capture ON capture_streams(capture_id);
CREATE TABLE IF NOT EXISTS ledger_events (
  id TEXT PRIMARY KEY NOT NULL,
  capture_id TEXT NOT NULL,
  type TEXT NOT NULL,
  occurred_at TEXT NOT NULL,
  payload_json TEXT NOT NULL,
  signature TEXT NOT NULL,
  received_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_ledger_capture_time ON ledger_events(capture_id, occurred_at);
CREATE TABLE IF NOT EXISTS attestation_challenges (
  nonce TEXT PRIMARY KEY NOT NULL,
  created_at TEXT NOT NULL,
  expires_at TEXT NOT NULL,
  used_at TEXT
);
CREATE TABLE IF NOT EXISTS rate_counters (
  key TEXT NOT NULL,
  window_start TEXT NOT NULL,
  count INTEGER NOT NULL DEFAULT 0,
  bytes INTEGER NOT NULL DEFAULT 0
);
CREATE UNIQUE INDEX IF NOT EXISTS uq_rate_counter_window ON rate_counters(key, window_start);
