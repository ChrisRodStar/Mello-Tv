import { useCallback, useRef, useState, type RefObject } from 'react';

export function usePlayerControls(video: RefObject<HTMLVideoElement | null>) {
  const [paused, setPaused] = useState(true);
  const [buffering, setBuffering] = useState(true);
  const [time, setTime] = useState(0);
  const [duration, setDuration] = useState(0);
  const [volume, setVolume] = useState(1);
  const [muted, setMuted] = useState(false);
  const [scrubbing, setScrubbing] = useState(false);
  const scrubTime = useRef<number | null>(null);
  const preferences = useRef({ volume: 1, muted: false });
  const attach = useCallback(
    (element: HTMLVideoElement | null) => {
      video.current = element;
      if (!element) return;
      element.volume = preferences.current.volume;
      element.muted = preferences.current.muted;
      scrubTime.current = null;
      setScrubbing(false);
      setTime(0);
      setDuration(0);
      setPaused(true);
      setBuffering(true);
    },
    [video],
  );
  function sync() {
    const element = video.current;
    if (!element) return;
    setPaused(element.paused);
    const validDuration =
      Number.isFinite(element.duration) && element.duration > 0 ? element.duration : 0;
    if (scrubTime.current === null)
      setTime(Number.isFinite(element.currentTime) ? Math.max(0, element.currentTime) : 0);
    setDuration(validDuration);
    setVolume(Number.isFinite(element.volume) ? Math.max(0, Math.min(1, element.volume)) : 0);
    setMuted(element.muted);
    preferences.current = { volume: element.volume, muted: element.muted };
  }
  async function toggle() {
    const element = video.current;
    if (!element) return;
    if (!element.paused) element.pause();
    else {
      try {
        await element.play();
      } catch {
        setPaused(true);
        setBuffering(false);
      }
    }
  }
  function boundedTime(value: number) {
    const element = video.current;
    if (
      !element ||
      !Number.isFinite(value) ||
      !Number.isFinite(element.duration) ||
      element.duration <= 0
    )
      return null;
    return Math.max(0, Math.min(element.duration, value));
  }
  function seek(value: number) {
    const target = boundedTime(value);
    if (target === null || !video.current) return;
    video.current.currentTime = target;
    setTime(target);
  }
  function beginScrub() {
    const element = video.current;
    if (!element) return;
    const target = boundedTime(element.currentTime);
    if (target !== null) {
      scrubTime.current = target;
      setScrubbing(true);
    }
  }
  function updateScrub(value: number) {
    const target = boundedTime(value);
    if (target === null || scrubTime.current === null) return;
    scrubTime.current = target;
    setTime(target);
  }
  function commitScrub() {
    setScrubbing(false);
    const target = scrubTime.current;
    scrubTime.current = null;
    if (target !== null) seek(target);
  }
  function cancelScrub() {
    setScrubbing(false);
    scrubTime.current = null;
    sync();
  }
  function changeVolume(value: number) {
    if (!video.current || !Number.isFinite(value)) return;
    const target = Math.max(0, Math.min(1, value));
    video.current.volume = target;
    video.current.muted = target === 0;
    sync();
  }
  function toggleMute() {
    if (!video.current) return;
    video.current.muted = !video.current.muted;
    sync();
  }
  return {
    attach,
    paused,
    buffering,
    time,
    duration,
    volume,
    muted,
    sync,
    toggle,
    seek,
    scrubbing,
    beginScrub,
    updateScrub,
    commitScrub,
    cancelScrub,
    changeVolume,
    toggleMute,
    setBuffering,
  };
}

export function formatPlaybackTime(seconds: number) {
  const value = Number.isFinite(seconds) ? Math.max(0, Math.floor(seconds)) : 0;
  const hours = Math.floor(value / 3600);
  const minutes = Math.floor((value % 3600) / 60);
  const remainder = String(value % 60).padStart(2, '0');
  return hours
    ? `${hours}:${String(minutes).padStart(2, '0')}:${remainder}`
    : `${minutes}:${remainder}`;
}
