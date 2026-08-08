import { getCapture, json } from "../../../../../../lib/control-plane";

export async function GET(_request: Request, context: { params: Promise<{ captureId: string }> }) {
  const { captureId } = await context.params;
  const detail = await getCapture(captureId);
  if (!detail) return json({ error: "Capture not found" }, 404);
  return json({ captureId, status: detail.capture.status, completeness: detail.capture.completeness, assuranceLevel: detail.capture.assuranceLevel, checks: detail.capture.verification, streams: detail.streams, events: detail.events, caveat: "ProofLine verifies recorded bytes and signed provenance claims. It cannot establish that a depicted scene was not staged or that a compromised device reported truthful sensors." }, 200, { "cache-control": "public, max-age=10" });
}
