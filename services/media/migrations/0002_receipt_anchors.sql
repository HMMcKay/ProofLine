CREATE TABLE IF NOT EXISTS receipt_anchors (
  id UUID PRIMARY KEY,
  created_at TIMESTAMPTZ NOT NULL,
  merkle_root TEXT NOT NULL,
  leaf_count INTEGER NOT NULL CHECK (leaf_count > 0),
  leaf_set JSONB NOT NULL,
  server_signature TEXT NOT NULL,
  tsa_url TEXT,
  tsa_status TEXT NOT NULL DEFAULT 'not_configured',
  tsa_response_object_key TEXT,
  tsa_error TEXT
);

ALTER TABLE fragments
  ADD COLUMN IF NOT EXISTS anchor_id UUID REFERENCES receipt_anchors(id);

CREATE INDEX IF NOT EXISTS idx_fragments_unanchored
  ON fragments(server_received_at)
  WHERE anchor_id IS NULL;
