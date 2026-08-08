import assert from "node:assert/strict";
import test from "node:test";

async function render(path = "/") {
  const workerUrl = new URL("../dist/server/index.js", import.meta.url);
  workerUrl.searchParams.set("test", `${process.pid}-${Date.now()}`);
  const { default: worker } = await import(workerUrl.href);
  return worker.fetch(new Request(`http://localhost${path}`, { headers: { accept: "text/html" } }), { ASSETS: { fetch: async () => new Response("Not found", { status: 404 }) } }, { waitUntil() {}, passThroughOnException() {} });
}

test("server renders the ProofLine public ledger", async () => {
  const response = await render();
  assert.equal(response.status, 200);
  const html = await response.text();
  assert.match(html, /ProofLine/);
  assert.match(html, /Every received frame/);
  assert.match(html, /No sign-in is required/);
  assert.doesNotMatch(html, /court certified|ironclad/i);
});

test("protocol and threat-model pages state the evidentiary boundary", async () => {
  const [protocol, threat] = await Promise.all([(await render("/protocol")).text(), (await render("/threat-model")).text()]);
  assert.match(protocol, /highest contiguous (fragment|prefix|receipt)/i);
  assert.match(protocol, /C2PA/i);
  assert.match(threat, /staged/i);
  assert.match(threat, /compromised/i);
});
