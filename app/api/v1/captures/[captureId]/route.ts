import { getCapture, json } from "../../../../../lib/control-plane";

export async function GET(_request: Request, context: { params: Promise<{ captureId: string }> }) {
  const { captureId } = await context.params;
  const detail = await getCapture(captureId);
  return detail ? json(detail, 200, { "cache-control": "public, max-age=3" }) : json({ error: "Capture not found" }, 404);
}
