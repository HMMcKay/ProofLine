import type { Metadata } from "next";
import { DeviceCaptures } from "../../components/DeviceCaptures";
export const metadata: Metadata = { title: "My videos" };
export default function MyPage() { return <DeviceCaptures fingerprints={[]} />; }
