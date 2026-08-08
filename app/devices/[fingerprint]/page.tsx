import type { Metadata } from "next";
import { DeviceCaptures } from "../../../components/DeviceCaptures";
export const metadata: Metadata = { title: "Device record" };
export default async function DevicePage({ params }: { params: Promise<{ fingerprint: string }> }) { const { fingerprint } = await params; return <DeviceCaptures fingerprints={[fingerprint]} />; }
