import { useState, type RefObject } from 'react';

export function usePlayerControls(video: RefObject<HTMLVideoElement | null>) {
  const [paused, setPaused] = useState(true);
  const [buffering, setBuffering] = useState(true);
  const [time, setTime] = useState(0);
  const [duration, setDuration] = useState(0);
  const [volume, setVolume] = useState(1);
  const [muted, setMuted] = useState(false);
  function sync() {
    const element = video.current;
    if (!element) return;
    setPaused(element.paused);
    setTime(element.currentTime);
    setDuration(Number.isFinite(element.duration) ? element.duration : 0);
    setVolume(element.volume);
    setMuted(element.muted);
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
  function seek(value: number) {
    const element = video.current;
    if (!element || !Number.isFinite(element.duration)) return;
    element.currentTime = Math.max(0, Math.min(element.duration, value));
    setTime(element.currentTime);
  }
  function changeVolume(value: number) {
    if (!video.current) return;
    video.current.volume = value;
    video.current.muted = value === 0;
    sync();
  }
  function toggleMute() {
    if (!video.current) return;
    video.current.muted = !video.current.muted;
    sync();
  }
  return {
    paused,
    buffering,
    time,
    duration,
    volume,
    muted,
    sync,
    toggle,
    seek,
    changeVolume,
    toggleMute,
    setBuffering,
  };
}

export function formatPlaybackTime(seconds: number) {
  const value = Math.max(0, Math.floor(seconds));
  const hours = Math.floor(value / 3600);
  const minutes = Math.floor((value % 3600) / 60);
  const remainder = String(value % 60).padStart(2, '0');
  return hours
    ? `${hours}:${String(minutes).padStart(2, '0')}:${remainder}`
    : `${minutes}:${remainder}`;
}
