"use client";

import { useCallback, useEffect, useRef, useState, useSyncExternalStore } from "react";
import { buildFragmentEnvelope, createSessionKey, drainUploadQueue, getOrCreateBrowserIdentity, queueFragment, signCanonical, uploadQueuedFragment } from "../lib/browser-evidence";
import { GENESIS_DIGEST, canonicalize, type CreateCaptureResponse, type StreamDeclaration } from "../lib/protocol";
import { SiteChrome } from "./SiteChrome";

type StudioState = "consent" | "preparing" | "recording" | "stopping" | "sealed" | "error";

function subscribeOnline(callback: () => void) {
  window.addEventListener("online", callback); window.addEventListener("offline", callback);
  return () => { window.removeEventListener("online", callback); window.removeEventListener("offline", callback); };
}
const getOnline = () => navigator.onLine;
const getOnlineServer = () => true;

function locationReading(): Promise<GeolocationPosition | null> {
  if (!("geolocation" in navigator)) return Promise.resolve(null);
  return new Promise((resolve) => navigator.geolocation.getCurrentPosition(resolve, () => resolve(null), { enableHighAccuracy: true, timeout: 8000, maximumAge: 0 }));
}

function randomId(prefix: string) { return `${prefix}_${crypto.randomUUID().replace(/-/g, "")}`; }

export function CaptureStudio() {
  const [state, setState] = useState<StudioState>(() => typeof window !== "undefined" && localStorage.getItem("proofline-public-consent-v2") === "accepted" ? "preparing" : "consent");
  const [title, setTitle] = useState("Untitled field capture");
  const [message, setMessage] = useState("Camera permission and a public relay connection are required.");
  const [captureId, setCaptureId] = useState<string>();
  const [elapsed, setElapsed] = useState(0);
  const [acknowledged, setAcknowledged] = useState(0);
  const [queued, setQueued] = useState(0);
  const online = useSyncExternalStore(subscribeOnline, getOnline, getOnlineServer);
  const videoRef = useRef<HTMLVideoElement>(null);
  const recorderRef = useRef<MediaRecorder | undefined>(undefined);
  const mediaRef = useRef<MediaStream | undefined>(undefined);
  const processingRef = useRef<Promise<void>>(Promise.resolve());
  const startedAtRef = useRef<number>(0);
  const sessionRef = useRef<{ response: CreateCaptureResponse; privateKey: CryptoKey; devicePrivateKey: CryptoKey; streamId: string; sequence: number; previousDigest: string; finalDigest: string } | undefined>(undefined);

  const stopTracks = useCallback(() => { mediaRef.current?.getTracks().forEach((track) => track.stop()); mediaRef.current = undefined; }, []);

  const begin = useCallback(async () => {
    try {
      setState("preparing"); setMessage("Creating a device key and requesting camera access…");
      await drainUploadQueue().catch(() => 0);
      const [identity, session, media, location] = await Promise.all([
        getOrCreateBrowserIdentity(), createSessionKey(), navigator.mediaDevices.getUserMedia({ video: { facingMode: { ideal: "environment" }, width: { ideal: 1920 }, height: { ideal: 1080 }, frameRate: { ideal: 30, max: 30 } }, audio: { echoCancellation: false, noiseSuppression: false, autoGainControl: false } }), locationReading(),
      ]);
      mediaRef.current = media;
      if (videoRef.current) { videoRef.current.srcObject = media; await videoRef.current.play(); }
      const challengeResponse = await fetch("/api/v1/devices/attest", { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify({ phase: "challenge" }) });
      if (!challengeResponse.ok) throw new Error("The device challenge endpoint is unavailable");
      const challenge = await challengeResponse.json() as { challenge: string };
      const attestResponse = await fetch("/api/v1/devices/attest", { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify({ phase: "verify", publicKeySpki: identity.spki, challenge: challenge.challenge }) });
      if (!attestResponse.ok) throw new Error("The browser signing key could not be registered");
      const attestation = await attestResponse.json() as { fingerprint: string; assuranceLevel: "web_key" };
      const sessionChallengeResponse = await fetch("/api/v1/devices/attest", { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify({ phase: "challenge" }) });
      if (!sessionChallengeResponse.ok) throw new Error("A fresh capture nonce could not be issued");
      const sessionChallenge = await sessionChallengeResponse.json() as { challenge: string };
      const mimeType = ["video/mp4;codecs=avc1.42E01E,mp4a.40.2", "video/webm;codecs=vp8,opus", "video/webm"].find((type) => MediaRecorder.isTypeSupported(type)) ?? "";
      const settings = media.getVideoTracks()[0]?.getSettings();
      const streamId = randomId("rear");
      const streams: StreamDeclaration[] = [{ id: streamId, role: "rear_video", mimeType: mimeType || "video/webm", codec: mimeType.includes("mp4") ? "avc1+aac" : "vp8+opus", width: settings?.width, height: settings?.height, fps: settings?.frameRate, hasAudio: media.getAudioTracks().length > 0 }];
      const startedAt = new Date().toISOString();
      const binding = { protocolVersion: "proofline/2", challenge: sessionChallenge.challenge, deviceFingerprint: attestation.fingerprint, sessionPublicKeySpki: session.spki, startedAt, streams };
      const sessionBindingSignature = await signCanonical(identity.privateKey, binding);
      const captureResponse = await fetch("/api/v1/captures", { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify({ sessionNonce: sessionChallenge.challenge, deviceFingerprint: attestation.fingerprint, assuranceLevel: attestation.assuranceLevel, devicePublicKeySpki: identity.spki, sessionPublicKeySpki: session.spki, sessionBindingSignature, title, startedAt, streams, location: location ? { latitude: location.coords.latitude, longitude: location.coords.longitude, accuracyM: location.coords.accuracy } : undefined }) });
      if (!captureResponse.ok) { const body = await captureResponse.json().catch(() => ({})) as { error?: string }; throw new Error(body.error ?? "The relay refused to start a capture"); }
      const response = await captureResponse.json() as CreateCaptureResponse;
      sessionRef.current = { response, privateKey: session.privateKey, devicePrivateKey: identity.privateKey, streamId, sequence: 0, previousDigest: GENESIS_DIGEST, finalDigest: GENESIS_DIGEST };
      const known = new Set<string>(JSON.parse(localStorage.getItem("proofline-device-fingerprints") ?? "[]")); known.add(attestation.fingerprint); localStorage.setItem("proofline-device-fingerprints", JSON.stringify([...known]));
      const recorder = new MediaRecorder(media, mimeType ? { mimeType, videoBitsPerSecond: 4_000_000, audioBitsPerSecond: 128_000 } : undefined);
      recorderRef.current = recorder;
      recorder.addEventListener("dataavailable", (event) => {
        if (!event.data.size || !sessionRef.current) return;
        processingRef.current = processingRef.current.then(async () => {
          const current = sessionRef.current!;
          const sequence = current.sequence++;
          const bytes = await event.data.arrayBuffer();
          const endUs = Math.round((performance.now() - startedAtRef.current) * 1000);
          const envelope = await buildFragmentEnvelope({ capture_id: current.response.captureId, stream_id: current.streamId, sequence, previous_chain_digest: current.previousDigest, pts_start_us: Math.max(0, endUs - 2_000_000), pts_end_us: endUs, telemetry_root: GENESIS_DIGEST }, bytes);
          const signature = await signCanonical(current.privateKey, envelope);
          current.previousDigest = envelope.chain_digest; current.finalDigest = envelope.chain_digest;
          const fragment = { id: `${current.response.captureId}/${current.streamId}/${sequence}`, captureId: current.response.captureId, streamId: current.streamId, sequence, mediaBaseUrl: current.response.mediaBaseUrl, uploadToken: current.response.uploadToken, envelope, signature, blob: event.data, queuedAt: new Date().toISOString() };
          await queueFragment(fragment); setQueued((value) => value + 1);
          if (navigator.onLine) { await uploadQueuedFragment(fragment); setQueued((value) => Math.max(0, value - 1)); setAcknowledged(sequence + 1); }
        }).catch((error) => setMessage(`Recording continues locally; relay warning: ${error instanceof Error ? error.message : "upload failed"}`));
      });
      startedAtRef.current = performance.now(); recorder.start(2000); setCaptureId(response.captureId); setState("recording"); setMessage("Fragments are being committed locally before relay upload.");
    } catch (error) { stopTracks(); setState("error"); setMessage(error instanceof Error ? error.message : "Capture could not start"); }
  }, [stopTracks, title]);

  // This is intentionally a mount-only continuation of consent recorded before this render.
  // eslint-disable-next-line react-hooks/exhaustive-deps
  useEffect(() => { if (state !== "preparing" || localStorage.getItem("proofline-public-consent-v2") !== "accepted") return; const timer = window.setTimeout(() => void begin(), 0); return () => window.clearTimeout(timer); }, []);
  // The interval only exists while recording; stop reads the current refs when the cap is reached.
  // eslint-disable-next-line react-hooks/exhaustive-deps
  useEffect(() => { if (state !== "recording") return; const timer = window.setInterval(() => { const next = performance.now() - startedAtRef.current; setElapsed(next); if (next >= 3_600_000) void stop("duration_limit"); }, 1000); return () => window.clearInterval(timer); }, [state]);
  useEffect(() => { if (online) void drainUploadQueue().then((count) => { if (count) { setQueued((value) => Math.max(0, value - count)); setAcknowledged((value) => value + count); } }); }, [online]);
  useEffect(() => stopTracks, [stopTracks]);

  async function acceptAndBegin() { localStorage.setItem("proofline-public-consent-v2", "accepted"); await begin(); }
  async function stop(reason: "user_stop" | "duration_limit" = "user_stop") {
    if (!recorderRef.current || !sessionRef.current || state !== "recording") return;
    setState("stopping"); setMessage("Closing the media tail and waiting for signed uploads…");
    await new Promise<void>((resolve) => { recorderRef.current!.addEventListener("stop", () => resolve(), { once: true }); recorderRef.current!.stop(); });
    await processingRef.current; await drainUploadQueue().catch(() => 0);
    const current = sessionRef.current;
    const endedAt = new Date().toISOString();
    const manifest = { protocolVersion: "proofline/2", captureId: current.response.captureId, endedAt, durationMs: Math.round(elapsed), closeReason: reason, streams: [{ id: current.streamId, sequenceCount: current.sequence, finalChainDigest: current.finalDigest }] };
    const signature = await signCanonical(current.devicePrivateKey, manifest);
    const response = await fetch(`${current.response.mediaBaseUrl.replace(/\/$/, "")}/ingest/v1/${current.response.captureId}/end`, { method: "POST", headers: { authorization: `Bearer ${current.response.uploadToken}`, "content-type": "application/json" }, body: canonicalize({ manifest, signature }) }).catch(() => null);
    stopTracks(); setState(response?.ok ? "sealed" : "error"); setMessage(response?.ok ? "The device-signed ending was accepted. Exact location remains delayed for 30 minutes." : "The final ending was not acknowledged. Received fragments remain independently receipted and the session will be marked interrupted.");
  }

  if (state === "consent") return <SiteChrome><main className="main" id="main-content"><section className="consent"><p className="eyebrow">One-time public-capture notice</p><h1>This camera posts in public.</h1><div className="warning danger-warning"><strong>There is no private mode.</strong> Video and audio begin uploading as they are recorded. A permalink and provenance record remain public afterward, subject only to the documented tombstone process.</div><ul><li>Precise location is cryptographically recorded and becomes public 30 minutes after capture ends.</li><li>Closing the app does not retract fragments already received by the server.</li><li>The browser fallback is signed but not hardware-attested.</li><li>Do not record where doing so is illegal or would endanger someone.</li></ul><div className="form-row"><label htmlFor="capture-title">Public capture title</label><input id="capture-title" value={title} maxLength={100} onChange={(event) => setTitle(event.target.value)} /></div><button className="primary-button" onClick={acceptAndBegin}>Accept and open camera</button></section></main></SiteChrome>;

  return <SiteChrome><main className="main" id="main-content"><div className="section-head"><div><p className="eyebrow">Browser-signed capture</p><h2>{state === "recording" ? "Recording publicly" : state === "sealed" ? "Capture sealed" : "Preparing capture"}</h2></div>{captureId && <p>{captureId}</p>}</div><div className="capture-layout"><div className="camera-stage"><video ref={videoRef} muted playsInline /><div className="camera-overlay">{state === "recording" && <span className="recording-status"><span className="live-dot" />Live public upload</span>}<div className="quality-strip"><span>{online ? "network online" : "offline queue"}</span><span>{Math.floor(elapsed / 60_000).toString().padStart(2, "0")}:{Math.floor(elapsed / 1000 % 60).toString().padStart(2, "0")}</span><span>{acknowledged} fragments acknowledged</span><span>{queued} queued</span></div></div></div><aside><div className="panel"><h3>Capture state</h3><p className={`status-message ${state === "error" ? "error" : ""}`}>{message}</p><div className="readings"><div className="reading"><span>Assurance</span><strong>Browser key</strong></div><div className="reading"><span>Fragment target</span><strong>2 seconds</strong></div><div className="reading"><span>Maximum</span><strong>60 minutes</strong></div><div className="reading"><span>Visibility</span><strong>Public</strong></div></div><div className="actions">{state === "recording" && <button className="primary-button" onClick={() => stop("user_stop")}>Stop and seal</button>}{state === "error" && <button className="secondary-button" onClick={() => location.reload()}>Try again</button>}{state === "sealed" && captureId && <a className="primary-button" href={`/captures/${captureId}`}>View evidence page</a>}</div></div></aside></div></main></SiteChrome>;
}
