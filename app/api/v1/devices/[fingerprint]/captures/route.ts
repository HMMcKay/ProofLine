import { json, listCaptures } from "../../../../../../lib/control-plane";

export async function GET(_request: Request, context: { params: Promise<{ fingerprint: string }> }) {
  const { fingerprint } = await context.params;
  return json({ fingerprint, captures: await listCaptures({ device: fingerprint, limit: 100 }) }, 200, { "cache-control": "public, max-age=10" });
}
