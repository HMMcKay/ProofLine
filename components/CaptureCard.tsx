import Link from "next/link";
import Image from "next/image";
import type { CaptureSummary } from "../lib/protocol";

const elapsed = (capture: CaptureSummary) => capture.durationMs ? `${Math.floor(capture.durationMs / 60_000)}m ${Math.floor(capture.durationMs / 1000) % 60}s` : capture.status === "live" ? "ongoing" : "—";

export function CaptureCard({ capture }: { capture: CaptureSummary }) {
  const location = capture.latitude == null ? "Location unavailable" : `${capture.latitude.toFixed(capture.locationIsCoarse ? 2 : 5)}, ${capture.longitude?.toFixed(capture.locationIsCoarse ? 2 : 5)}${capture.locationIsCoarse ? " approx." : ""}`;
  return <Link className="capture-card" href={`/captures/${capture.id}`}>
    <div className="capture-preview">
      {capture.posterUrl ? <Image src={capture.posterUrl} alt="Video preview frame" fill sizes="(max-width: 640px) 100vw, 33vw" unoptimized /> : <span className="capture-preview-placeholder" aria-hidden="true" />}
      <span className="capture-badge" data-status={capture.status}>{capture.status}</span>
    </div>
    <div className="capture-card-body">
      <h3>{capture.title}</h3>
      <div className="capture-meta">
        <span>Started<br /><strong>{new Date(capture.startedAt).toLocaleString()}</strong></span>
        <span>Duration<br /><strong>{elapsed(capture)}</strong></span>
        <span>Device<br /><strong>{capture.deviceFingerprint.slice(0, 12)}</strong></span>
        <span>Assurance<br /><strong>{capture.assuranceLevel.replace("_", " ")}</strong></span>
        <span style={{ gridColumn: "1 / -1" }}>Place<br /><strong>{location}</strong></span>
      </div>
    </div>
  </Link>;
}
