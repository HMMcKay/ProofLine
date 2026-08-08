import assert from "node:assert/strict";
import crypto from "node:crypto";
import { readFile } from "node:fs/promises";
import test from "node:test";

function canonical(value) {
  if (value === null || typeof value !== "object") return JSON.stringify(value);
  if (Array.isArray(value)) return `[${value.map(canonical).join(",")}]`;
  return `{${Object.keys(value).sort().map((key) => `${JSON.stringify(key)}:${canonical(value[key])}`).join(",")}}`;
}

test("shared fragment chain vector is stable", async () => {
  const fixture = JSON.parse(await readFile(new URL("../protocol/test-vectors/fragment-chain-v2.json", import.meta.url)));
  assert.equal(canonical(fixture.chain_input), fixture.canonical);
  assert.equal(crypto.createHash("sha256").update(fixture.canonical).digest("hex"), fixture.chain_digest);
  assert.equal(crypto.createHash("sha256").update(Buffer.from(fixture.media_base64url, "base64url")).digest("hex"), fixture.media_digest);
});
