"use client";

import Hls from "hls.js";
import { useEffect, useRef } from "react";

export function HlsPlayer({ source, muted = false, label }: { source: string; muted?: boolean; label: string }) {
  const ref = useRef<HTMLVideoElement>(null);
  useEffect(() => {
    const video = ref.current;
    if (!video) return;
    if (video.canPlayType("application/vnd.apple.mpegurl")) { video.src = source; return; }
    if (!Hls.isSupported()) return;
    const hls = new Hls({ liveSyncDurationCount: 2, maxLiveSyncPlaybackRate: 1.25 });
    hls.loadSource(source); hls.attachMedia(video);
    return () => hls.destroy();
  }, [source]);
  // User-generated evidence does not have a trustworthy caption track unless one is explicitly derived later.
  // eslint-disable-next-line jsx-a11y/media-has-caption
  return <video ref={ref} controls muted={muted} playsInline aria-label={label} />;
}
