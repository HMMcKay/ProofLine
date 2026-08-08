"use client";

import { useEffect, useState } from "react";
import type { CaptureSummary } from "../lib/protocol";
import { CaptureCard } from "./CaptureCard";
import { SiteChrome } from "./SiteChrome";

export function DeviceCaptures({ fingerprints }: { fingerprints: string[] }) {
  const [captures, setCaptures] = useState<CaptureSummary[]>([]);
  const [known, setKnown] = useState(fingerprints);
  useEffect(() => { const timer = window.setTimeout(() => { const local = JSON.parse(localStorage.getItem("proofline-device-fingerprints") ?? "[]") as string[]; const all = [...new Set([...fingerprints, ...local])]; setKnown(all); void Promise.all(all.map(async (fingerprint) => { const response = await fetch(`/api/v1/devices/${encodeURIComponent(fingerprint)}/captures`); return response.ok ? (await response.json() as { captures: CaptureSummary[] }).captures : []; })).then((groups) => setCaptures(groups.flat().sort((a, b) => b.startedAt.localeCompare(a.startedAt)))); }, 0); return () => window.clearTimeout(timer); }, [fingerprints]);
  return <SiteChrome><main className="main" id="main-content"><section className="hero"><div><p className="eyebrow">Local convenience, public records</p><h1>My <span>videos.</span></h1></div><div className="hero-copy"><p>This browser remembers public device fingerprints; it does not authenticate ownership. Anyone with a fingerprint can view the same public device history.</p><p><strong>{known.length} known device{known.length === 1 ? "" : "s"}</strong></p></div></section><section className="section">{captures.length ? <div className="capture-grid">{captures.map((capture) => <CaptureCard capture={capture} key={capture.id} />)}</div> : <div className="empty-state"><h3>No local device history</h3><p>Record from this browser or open the verified link from the Android app to remember its public fingerprint here.</p><a className="primary-button" href="/capture">Start a public capture</a></div>}</section></main></SiteChrome>;
}
