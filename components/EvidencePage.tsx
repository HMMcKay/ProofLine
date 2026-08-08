"use client";

import { useCallback, useEffect, useState } from "react";
import type { CaptureSummary, VerificationSummary } from "../lib/protocol";
import { HlsPlayer } from "./HlsPlayer";
import { SiteChrome } from "./SiteChrome";

interface Detail { capture: CaptureSummary; streams: Array<Record<string, unknown>>; events: Array<Record<string, unknown>>; }

const checkLabels: Record<keyof VerificationSummary, string> = {
  fragmentChain: "Fragment continuity", deviceSignature: "Device/session signatures", audioBinding: "Audio binding",
  serverReceipts: "Server receipts", timestampAnchor: "Independent time anchor", c2pa: "C2PA asset binding",
};

function formatDuration(ms: number) { return `${Math.floor(ms / 60_000)}:${Math.floor(ms / 1000 % 60).toString().padStart(2, "0")}`; }

export function EvidencePage({ captureId }: { captureId: string }) {
  const [detail, setDetail] = useState<Detail>();
  const [error, setError] = useState("");
  const [liveValues, setLiveValues] = useState<Record<string, unknown>>({});
  const load = useCallback(async () => { const response = await fetch(`/api/v1/captures/${captureId}`, { cache: "no-store" }); if (!response.ok) throw new Error("Capture not found"); setDetail(await response.json() as Detail); }, [captureId]);
  useEffect(() => { const first = window.setTimeout(() => void load().catch((reason) => setError(reason instanceof Error ? reason.message : "Unable to load evidence")), 0); const timer = window.setInterval(() => void load().catch(() => undefined), 5000); return () => { window.clearTimeout(first); window.clearInterval(timer); }; }, [load]);
  const mediaBaseUrl = detail?.capture.mediaBaseUrl;
  const captureStatus = detail?.capture.status;
  useEffect(() => { if (!mediaBaseUrl || !captureStatus || !["initializing", "live", "stalled"].includes(captureStatus)) return; const events = new EventSource(`${mediaBaseUrl.replace(/\/$/, "")}/events/v1/${captureId}`); events.onmessage = (event) => { try { setLiveValues(JSON.parse(event.data) as Record<string, unknown>); } catch { /* malformed live telemetry is ignored, not added to evidence */ } }; return () => events.close(); }, [captureId, mediaBaseUrl, captureStatus]);
  async function share() { const url = window.location.href; if (navigator.share) await navigator.share({ title: detail?.capture.title ?? "ProofLine capture", url }); else await navigator.clipboard.writeText(url); }
  if (error) return <SiteChrome><main className="main" id="main-content"><p className="warning danger-warning">{error}</p></main></SiteChrome>;
  if (!detail) return <SiteChrome><main className="main" id="main-content"><div className="loading-line" role="status" aria-label="Loading evidence" /></main></SiteChrome>;
  const { capture, streams, events } = detail;
  const base = capture.mediaBaseUrl.replace(/\/$/, "");
  const location = capture.latitude == null ? "Unavailable" : `${capture.latitude.toFixed(capture.locationIsCoarse ? 2 : 5)}, ${capture.longitude?.toFixed(capture.locationIsCoarse ? 2 : 5)}${capture.locationIsCoarse ? " (approximately 1 km)" : ""}`;
  return <SiteChrome><main className="main" id="main-content">
    <div className="section-head"><div><p className="eyebrow">Public evidence record · {capture.status}</p><h2>{capture.title}</h2></div><div className="actions"><button className="secondary-button" onClick={share}>Share permalink</button><a className="primary-button" href={`${base}/evidence/v1/${capture.id}/bundle.zip`}>Download evidence</a></div></div>
    {capture.status === "interrupted" && <p className="warning">No valid device-signed ending was received. The shown ending is the highest contiguous fragment durably received by the server, not proof that the device recorded nothing later.</p>}
    {capture.status === "tombstoned" && <p className="warning danger-warning">Playback is unavailable under the documented tombstone process. Hashes, signed receipts, and this audit record remain public. Reason: {capture.tombstoneReason ?? "administrative review"}</p>}
    <div className="detail-grid section">
      <div className="video-stack">{capture.status !== "tombstoned" && streams.filter((stream) => String(stream.role).includes("video")).map((stream, index) => <div className="video-frame" key={String(stream.id)}><span className="video-label">{String(stream.role).replace("_video", " camera")}</span><HlsPlayer source={`${base}/live/v1/${capture.id}/${stream.id}/index.m3u8`} muted={index > 0} label={`${String(stream.role)} evidence video`} /><a className="download-inline" href={`${base}/evidence/v1/${capture.id}/original/${stream.id}`}>Download immutable stream bytes</a></div>)}</div>
      <aside className="panel"><h3>Live receipt state</h3><div className="readings"><div className="reading"><span>Status</span><strong>{capture.status}</strong></div><div className="reading"><span>Completeness</span><strong>{capture.completeness.replaceAll("_", " ")}</strong></div><div className="reading"><span>Duration</span><strong>{formatDuration(capture.durationMs)}</strong></div><div className="reading"><span>Device</span><strong>{capture.deviceFingerprint.slice(0, 16)}</strong></div><div className="reading"><span>Assurance</span><strong>{capture.assuranceLevel.replace("_", " ")}</strong></div><div className="reading"><span>Location</span><strong>{location}</strong></div><div className="reading"><span>Last receipt</span><strong>{capture.lastReceiptAt ? new Date(capture.lastReceiptAt).toLocaleTimeString() : "pending"}</strong></div>{Object.entries(liveValues).slice(0, 5).map(([key, value]) => <div className="reading" key={key}><span>{key}</span><strong>{String(value)}</strong></div>)}</div></aside>
    </div>
    <section className="section"><div className="section-head"><h2>Verification report</h2><p>Checks are independent; no composite “truth score” is calculated.</p></div><p className="warning">Cryptography can show that accepted bytes and signed metadata have not changed. It cannot rule out a staged scene, an off-camera event, screen re-recording, or sensor spoofing by a compromised phone.</p><div className="check-list">{(Object.entries(capture.verification) as Array<[keyof VerificationSummary, VerificationSummary[keyof VerificationSummary]]>).map(([key, result]) => <div className="check" data-result={result} key={key}><span className="check-mark" /><strong>{checkLabels[key]}</strong><code>{result}</code></div>)}</div><div className="actions"><a className="secondary-button" href={`${base}/evidence/v1/${capture.id}/report.pdf`}>Download PDF report</a><a className="secondary-button" href={`${base}/evidence/v1/${capture.id}/report.json`}>Canonical evidence JSON</a><a className="secondary-button" href={`/api/v1/captures/${capture.id}/verification`}>Public ledger projection</a></div><details className="details-raw"><summary>Raw signed ledger events</summary><pre>{JSON.stringify(events, null, 2)}</pre></details></section>
  </main></SiteChrome>;
}
