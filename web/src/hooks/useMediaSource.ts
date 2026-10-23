import { useEffect, type RefObject } from 'react';

import type { PlaybackSource } from '../lib/playback';

export function useMediaSource(
  video: RefObject<HTMLVideoElement | null>,
  source: PlaybackSource | null,
  onError: () => void,
) {
  useEffect(() => {
    const element = video.current;
    if (!element || !source) return;
    let disposed = false;
    let destroy: (() => void) | undefined;
    const hls =
      /mpegurl/i.test(source.mimeType ?? '') || /\.m3u8$/i.test(new URL(source.url).pathname);
    async function attach() {
      if (!hls || element!.canPlayType('application/vnd.apple.mpegurl')) {
        element!.src = source!.url;
        element!.load();
        try {
          await element!.play();
        } catch {
          /* Autoplay may require a user gesture. */
        }
        return;
      }
      const { default: Hls } = await import('hls.js');
      if (disposed) return;
      if (!Hls.isSupported()) {
        onError();
        return;
      }
      const instance = new Hls({ maxBufferLength: 30 });
      destroy = () => instance.destroy();
      instance.on(Hls.Events.ERROR, (_event, data) => {
        if (data.fatal && !disposed) onError();
      });
      instance.on(Hls.Events.MANIFEST_PARSED, () => {
        if (!disposed) void element!.play().catch(() => {});
      });
      instance.loadSource(source!.url);
      instance.attachMedia(element!);
    }
    void attach().catch(() => {
      if (!disposed) onError();
    });
    return () => {
      disposed = true;
      destroy?.();
      element.pause();
      element.removeAttribute('src');
      element.load();
    };
  }, [video, source, onError]);
}
