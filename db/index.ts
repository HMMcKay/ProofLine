import { drizzle } from "drizzle-orm/d1";
import * as schema from "./schema";

export function getDb() {
  const runtime = (globalThis as typeof globalThis & { __PROOFLINE_ENV__?: { DB?: D1Database } }).__PROOFLINE_ENV__;
  if (!runtime?.DB) {
    throw new Error(
      "Cloudflare D1 binding `DB` is unavailable. Set the `d1` field in .openai/hosting.json to `DB` or let your control plane inject the real binding values before using the database."
    );
  }

  return drizzle(runtime.DB, { schema });
}
