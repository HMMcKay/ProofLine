import { index, integer, real, sqliteTable, text, uniqueIndex } from "drizzle-orm/sqlite-core";

/**
 * D1 is the public, searchable read model. Original media and high-volume
 * receipts remain in the media plane; only signed projections live here.
 */
export const devices = sqliteTable("devices", {
  fingerprint: text("fingerprint").primaryKey(),
  assuranceLevel: text("assurance_level").notNull(),
  publicKeySpki: text("public_key_spki").notNull(),
  attestationSummary: text("attestation_summary").notNull().default("{}"),
  createdAt: text("created_at").notNull(),
  rotatedFrom: text("rotated_from"),
});

export const captures = sqliteTable(
  "captures",
  {
    id: text("id").primaryKey(),
    deviceFingerprint: text("device_fingerprint").notNull(),
    title: text("title").notNull(),
    status: text("status").notNull(),
    completeness: text("completeness").notNull(),
    assuranceLevel: text("assurance_level").notNull(),
    startedAt: text("started_at").notNull(),
    endedAt: text("ended_at"),
    lastReceiptAt: text("last_receipt_at"),
    exactLocationReleaseAt: text("exact_location_release_at"),
    coarseLatitude: real("coarse_latitude"),
    coarseLongitude: real("coarse_longitude"),
    exactLatitude: real("exact_latitude"),
    exactLongitude: real("exact_longitude"),
    locationAccuracyM: real("location_accuracy_m"),
    streamCount: integer("stream_count").notNull().default(1),
    durationMs: integer("duration_ms").notNull().default(0),
    closeReason: text("close_reason"),
    verificationSummary: text("verification_summary").notNull().default("{}"),
    mediaBaseUrl: text("media_base_url").notNull(),
    posterUrl: text("poster_url"),
    tombstoneReason: text("tombstone_reason"),
    createdAt: text("created_at").notNull(),
    updatedAt: text("updated_at").notNull(),
  },
  (table) => [
    index("idx_captures_started_at").on(table.startedAt),
    index("idx_captures_status_started_at").on(table.status, table.startedAt),
    index("idx_captures_device_started_at").on(table.deviceFingerprint, table.startedAt),
  ],
);

export const captureStreams = sqliteTable(
  "capture_streams",
  {
    id: text("id").primaryKey(),
    captureId: text("capture_id").notNull(),
    role: text("role").notNull(),
    mimeType: text("mime_type").notNull(),
    codec: text("codec").notNull(),
    width: integer("width"),
    height: integer("height"),
    fps: real("fps"),
    hasAudio: integer("has_audio", { mode: "boolean" }).notNull().default(false),
    sequenceCount: integer("sequence_count").notNull().default(0),
    finalChainDigest: text("final_chain_digest"),
  },
  (table) => [
    uniqueIndex("uq_capture_stream_role").on(table.captureId, table.role),
    index("idx_capture_stream_capture").on(table.captureId),
  ],
);

export const ledgerEvents = sqliteTable(
  "ledger_events",
  {
    id: text("id").primaryKey(),
    captureId: text("capture_id").notNull(),
    type: text("type").notNull(),
    occurredAt: text("occurred_at").notNull(),
    payloadJson: text("payload_json").notNull(),
    signature: text("signature").notNull(),
    receivedAt: text("received_at").notNull(),
  },
  (table) => [index("idx_ledger_capture_time").on(table.captureId, table.occurredAt)],
);

export const attestationChallenges = sqliteTable("attestation_challenges", {
  nonce: text("nonce").primaryKey(),
  createdAt: text("created_at").notNull(),
  expiresAt: text("expires_at").notNull(),
  usedAt: text("used_at"),
});

export const rateCounters = sqliteTable(
  "rate_counters",
  {
    key: text("key").notNull(),
    windowStart: text("window_start").notNull(),
    count: integer("count").notNull().default(0),
    bytes: integer("bytes").notNull().default(0),
  },
  (table) => [uniqueIndex("uq_rate_counter_window").on(table.key, table.windowStart)],
);
