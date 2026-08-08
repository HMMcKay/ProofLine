ALTER TABLE captures ADD COLUMN IF NOT EXISTS origin_ip_hash TEXT;
ALTER TABLE captures ADD COLUMN IF NOT EXISTS device_public_key_spki TEXT;
CREATE INDEX IF NOT EXISTS idx_captures_origin_status ON captures(origin_ip_hash, status);

CREATE TABLE IF NOT EXISTS blocked_devices (
  device_fingerprint TEXT PRIMARY KEY,
  reason TEXT NOT NULL,
  expires_at TIMESTAMPTZ,
  created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE IF NOT EXISTS blocked_ips (
  ip_hash TEXT PRIMARY KEY,
  reason TEXT NOT NULL,
  expires_at TIMESTAMPTZ,
  created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
