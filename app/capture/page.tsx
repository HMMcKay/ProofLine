import type { Metadata } from "next";
import { CaptureStudio } from "../../components/CaptureStudio";

export const metadata: Metadata = { title: "Record publicly", description: "Start a browser-signed public ProofLine capture." };
export default function CapturePage() { return <CaptureStudio />; }
