"use client";

import { useCallback, useEffect, useState } from "react";
import type { CaptureSummary } from "../lib/protocol";
import { CaptureCard } from "./CaptureCard";
import { SiteChrome } from "./SiteChrome";

async function load(status: string) {
  const response = await fetch(`/api/v1/captures?status=${status}`, { cache: "no-store" });
  if (!response.ok) throw new Error("The public ledger could not be loaded");
  return (await response.json() as { captures: CaptureSummary[] }).captures;
}

export function LedgerHome() {
  const [live, setLive] = useState<CaptureSummary[]>([]);
  const [recent, setRecent] = useState<CaptureSummary[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState("");
  const refresh = useCallback(async () => {
    try { const [liveCaptures, recentCaptures] = await Promise.all([load("live"), load("recent")]); setLive(liveCaptures); setRecent(recentCaptures.filter((item) => !liveCaptures.some((liveItem) => liveItem.id === item.id))); setError(""); }
    catch (reason) { setError(reason instanceof Error ? reason.message : "Ledger unavailable"); }
    finally { setLoading(false); }
  }, []);
  useEffect(() => { const first = window.setTimeout(() => void refresh(), 0); const timer = window.setInterval(refresh, 5000); return () => { window.clearTimeout(first); window.clearInterval(timer); }; }, [refresh]);

  return <SiteChrome><main className="main" id="main-content">
    <section className="hero">
      <div><p className="eyebrow">Public video provenance</p><h1>Every received frame leaves a <span>receipt.</span></h1></div>
      <div className="hero-copy"><p>ProofLine uploads video while the camera is still running, then binds the exact received bytes, audio, timing, device key and sensor context into an inspectable public record.</p><p><strong>It proves continuity and provenance claims—not that a scene could not have been staged.</strong></p><a className="link-arrow" href="/protocol">Read the evidence protocol →</a></div>
    </section>
    {loading && <div className="loading-line" role="status" aria-label="Loading public evidence ledger" />}
    {error && <p className="warning danger-warning">{error}</p>}
    <section className="section" aria-labelledby="live-heading">
      <div className="section-head"><h2 id="live-heading"><span className="live-dot" />Live now</h2><p>{live.length ? `${live.length} public capture${live.length === 1 ? "" : "s"} receiving fragments` : "No active captures"}</p></div>
      {live.length ? <div className="capture-grid">{live.map((capture) => <CaptureCard capture={capture} key={capture.id} />)}</div> : <div className="empty-state"><h3>The relay is quiet</h3><p>When a device starts recording, the first durably received fragments appear here. No sign-in is required to watch.</p><a className="primary-button" href="/capture">Start a public capture</a></div>}
    </section>
    <section className="section" aria-labelledby="recent-heading">
      <div className="section-head"><h2 id="recent-heading">Recent record</h2><p>Newest public evidence first</p></div>
      {recent.length ? <div className="capture-grid">{recent.map((capture) => <CaptureCard capture={capture} key={capture.id} />)}</div> : <div className="empty-state"><h3>No archived captures yet</h3><p>The ledger begins empty. Synthetic fixtures are never presented as real evidence.</p></div>}
    </section>
  </main></SiteChrome>;
}
