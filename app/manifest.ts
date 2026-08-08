import type { MetadataRoute } from "next";
export default function manifest(): MetadataRoute.Manifest { return { name: "ProofLine", short_name: "ProofLine", description: "Public, device-signed field video with inspectable server receipts.", start_url: "/", display: "standalone", background_color: "#090a0a", theme_color: "#090a0a", icons: [] }; }
