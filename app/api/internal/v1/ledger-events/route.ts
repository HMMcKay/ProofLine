import { applyLedgerEvent, bindings, constantTimeEqual, hmacHex, json } from "../../../../../lib/control-plane";
import type { LedgerEvent } from "../../../../../lib/protocol";

export async function POST(request: Request) {
  const secret = bindings().PROOFLINE_INTERNAL_SECRET;
  if (!secret) return json({ error: "Internal event secret is not configured" }, 503);
  const body = await request.text();
  const expected = await hmacHex(secret, body);
  if (!constantTimeEqual(expected, request.headers.get("x-proofline-hmac") ?? "")) return json({ error: "Invalid event signature" }, 401);
  try {
    const event = JSON.parse(body) as LedgerEvent;
    await applyLedgerEvent(event);
    return json({ accepted: true });
  } catch (error) {
    return json({ error: error instanceof Error ? error.message : "Invalid event" }, 400);
  }
}
