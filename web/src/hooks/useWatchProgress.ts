import { trackProgress } from '../lib/progress-ledger';
import { useEffect, useRef, type RefObject } from 'react';
import { apiJSON, profilePath } from '../lib/api';
import { progressWire, viewProgress } from '../lib/wire';
import { cachedTitle } from '../lib/catalog';
import type { PlaybackRequest, PlaybackSource } from '../lib/playback';

type Counter = { total: number; sequence: number; acknowledged: number };
export function useWatchProgress(
  video: RefObject<HTMLVideoElement | null>,
  request: PlaybackRequest,
  source: PlaybackSource | null,
) {
  const counters = useRef(new Map<string, Counter>());
  const flushCurrent = useRef<() => Promise<void>>(async () => {});
  useEffect(() => {
    const node = video.current;
    if (!node || !source) return;
    const sid = source.session.id;
    let counter = counters.current.get(sid);
    if (!counter) {
      counter = { total: 0, sequence: 0, acknowledged: 0 };
      counters.current.set(sid, counter);
    }
    const measured = counter;
    let lastClock = performance.now(),
      lastPosition = node.currentTime,
      lastSent = -1,
      lastSavedPosition = -1,
      restored = false;
    let snapshot = { position: Math.max(0, source.session.resume_seconds), duration: 0 };
    const restore = () => {
      if (restored || !Number.isFinite(node.duration) || node.duration <= 0) return;
      restored = true;
      const resume = source.session.resume_seconds;
      if (resume > 0 && resume < node.duration - 0.25) node.currentTime = resume;
      lastPosition = node.currentTime;
      lastClock = performance.now();
    };
    const sample = () => {
      const now = performance.now(),
        elapsed = Math.min(2.5, Math.max(0, (now - lastClock) / 1000)),
        advanced = node.currentTime - lastPosition;
      if (
        !node.paused &&
        !node.seeking &&
        node.readyState >= 2 &&
        document.visibilityState === 'visible' &&
        advanced > 0 &&
        advanced <= elapsed * node.playbackRate + 1
      )
        measured.total += Math.min(elapsed, advanced / Math.max(0.1, node.playbackRate));
      lastPosition = node.currentTime;
      lastClock = now;
      if (node.readyState >= 1 && Number.isFinite(node.duration) && node.duration > 0)
        snapshot = {
          position: Math.max(0, node.currentTime),
          duration: Math.min(86400, node.duration),
        };
    };
    const flush = async () => {
      sample();
      if (
        measured.total <= 0 ||
        (measured.total === lastSent && Math.abs(snapshot.position - lastSavedPosition) < 1)
      )
        return;
      const sent = measured.total,
        sequence = ++measured.sequence;
      lastSent = sent;
      lastSavedPosition = snapshot.position;
      try {
        const p = await trackProgress(
          sid,
          apiJSON(`${profilePath(request.profileId)}/progress`, progressWire, {
            method: 'PUT',
            keepalive: true,
            body: JSON.stringify({
              playback_id: sid,
              sequence,
              position_seconds: Math.min(86400, snapshot.position),
              duration_seconds: snapshot.duration,
              watched_seconds: sent,
            }),
          }),
        );
        if (sequence >= measured.acknowledged) {
          measured.acknowledged = sequence;
          const progress = viewProgress(
            p,
            request.catalogTitle ?? cachedTitle(request.type, request.titleId),
            request.type === 'tv' ? request.episodeTitle : '',
          );
          window.dispatchEvent(
            new CustomEvent('family-stream-progress', {
              detail: { profileId: request.profileId, progress },
            }),
          );
        }
      } catch {
        if (sequence === measured.sequence) lastSent = -1;
      }
    };
    flushCurrent.current = flush;
    const onFlush = () => {
      void flush();
    };
    node.addEventListener('loadedmetadata', restore);
    node.addEventListener('pause', onFlush);
    node.addEventListener('ended', onFlush);
    window.addEventListener('pagehide', onFlush);
    document.addEventListener('visibilitychange', onFlush);
    restore();
    const tick = setInterval(sample, 1000),
      save = setInterval(onFlush, 15000);
    return () => {
      void flush();
      flushCurrent.current = async () => {};
      clearInterval(tick);
      clearInterval(save);
      node.removeEventListener('loadedmetadata', restore);
      node.removeEventListener('pause', onFlush);
      node.removeEventListener('ended', onFlush);
      window.removeEventListener('pagehide', onFlush);
      document.removeEventListener('visibilitychange', onFlush);
    };
  }, [video, request, source?.url, source?.session.id]);
  return { flush: () => flushCurrent.current() };
}
