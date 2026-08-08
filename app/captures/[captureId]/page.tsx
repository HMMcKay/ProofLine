import type { Metadata } from "next";
import { EvidencePage } from "../../../components/EvidencePage";

export const metadata: Metadata = { title: "Evidence record" };
export default async function CapturePage({ params }: { params: Promise<{ captureId: string }> }) { const { captureId } = await params; return <EvidencePage captureId={captureId} />; }
