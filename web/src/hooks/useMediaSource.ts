import { useCallback, useEffect, useRef, useState, type RefObject } from 'react';
import type Hls from 'hls.js';
import type { PlaybackSource } from '../lib/playback';
import { videoQualityOptions } from '../lib/video-quality';

type Option = { id: number; label: string };
type AudioTrack = { enabled: boolean; label: string; language: string };
type AudioTracks = EventTarget & { length: number; [index: number]: AudioTrack };
type VideoWithAudio = HTMLVideoElement & { audioTracks?: AudioTracks };
const trackLabel = (name: string | undefined, language: string | undefined, fallback: string) =>
  name || language || fallback;

export function useMediaSource(
  video: RefObject<HTMLVideoElement | null>,
  source: PlaybackSource | null,
  onError: () => void,
) {
  const hlsRef = useRef<Hls | null>(null);
  const [qualities, setQualities] = useState<Option[]>([]);
  const [quality, setQualityState] = useState(-1);
  const [audioTracks, setAudioTracks] = useState<Option[]>([]);
  const [audioTrack, setAudioTrackState] = useState(-1);
  const [subtitleTracks, setSubtitleTracks] = useState<Option[]>([]);
  const [subtitleTrack, setSubtitleTrackState] = useState(-1);
  const [autoplayBlocked, setAutoplayBlocked] = useState(false);
  const setQuality = useCallback((id: number) => {
    const hls = hlsRef.current;
    if (hls) {
      if (id !== -1 && (id < 0 || id >= hls.levels.length)) return;
      hls.currentLevel = id;
    } else if (id !== -1 && id !== 0) {
      return;
    }
    setQualityState(id);
  }, []);
  const setAudioTrack = useCallback(
    (id: number) => {
      const hls = hlsRef.current;
      if (hls && hls.audioTracks.length) {
        if (id < 0 || id >= hls.audioTracks.length) return;
        hls.audioTrack = id;
      } else {
        const tracks = (video.current as VideoWithAudio | null)?.audioTracks;
        if (!tracks || id < 0 || id >= tracks.length) return;
        for (let i = 0; i < tracks.length; i++) tracks[i]!.enabled = i === id;
      }
      setAudioTrackState(id);
    },
    [video],
  );
  const setSubtitleTrack = useCallback(
    (id: number) => {
      const hls = hlsRef.current;
      if (hls && hls.subtitleTracks.length) {
        if (id < -1 || id >= hls.subtitleTracks.length) return;
        hls.subtitleTrack = id;
      } else {
        const tracks = video.current?.textTracks;
        if (
          !tracks ||
          id < -1 ||
          id >= tracks.length ||
          (id !== -1 && !['subtitles', 'captions'].includes(tracks[id]!.kind))
        )
          return;
        for (let i = 0; i < tracks.length; i++) {
          if (['subtitles', 'captions'].includes(tracks[i]!.kind))
            tracks[i]!.mode = i === id ? 'showing' : 'disabled';
        }
      }
      setSubtitleTrackState(id);
    },
    [video],
  );
  useEffect(() => {
    const element = video.current;
    setQualities([]);
    setQualityState(-1);
    setAudioTracks([]);
    setAudioTrackState(-1);
    setSubtitleTracks([]);
    setSubtitleTrackState(-1);
    setAutoplayBlocked(false);
    if (!element || !source) return;
    let disposed = false;
    let instance: Hls | null = null;
    const nativeAudio = (element as VideoWithAudio).audioTracks;
    const syncTracks = () => {
      if (disposed) return;
      const fallbackLevels = [
        ...(source.session.available_heights?.map((height) => ({ height })) ?? []),
        ...(element.videoHeight > 0
          ? [{ width: element.videoWidth, height: element.videoHeight }]
          : []),
      ];
      if (instance) {
        setQualities(videoQualityOptions(instance.levels, fallbackLevels));
        setQualityState(instance.manualLevel);
      } else {
        setQualities(videoQualityOptions([], fallbackLevels));
      }
      if (instance?.audioTracks.length) {
        setAudioTracks(
          instance.audioTracks.map((track, id) => ({
            id,
            label: trackLabel(track.name, track.lang, `Audio ${id + 1}`),
          })),
        );
        setAudioTrackState(instance.audioTrack);
      } else {
        const tracks = nativeAudio
          ? Array.from({ length: nativeAudio.length }, (_, id) => ({
              id,
              track: nativeAudio[id]!,
            }))
          : [];
        setAudioTracks(
          tracks.map(({ id, track }) => ({
            id,
            label: trackLabel(track.label, track.language, `Audio ${id + 1}`),
          })),
        );
        setAudioTrackState(tracks.find(({ track }) => track.enabled)?.id ?? -1);
      }
      if (instance?.subtitleTracks.length) {
        setSubtitleTracks(
          instance.subtitleTracks.map((track, id) => ({
            id,
            label: trackLabel(track.name, track.lang, `Subtitle ${id + 1}`),
          })),
        );
        setSubtitleTrackState(instance.subtitleTrack);
      } else {
        const tracks = Array.from(element.textTracks)
          .map((track, id) => ({ id, track }))
          .filter(({ track }) => ['subtitles', 'captions'].includes(track.kind));
        setSubtitleTracks(
          tracks.map(({ id, track }) => ({
            id,
            label: trackLabel(track.label, track.language, `Subtitle ${id + 1}`),
          })),
        );
        setSubtitleTrackState(tracks.find(({ track }) => track.mode === 'showing')?.id ?? -1);
      }
    };
    const onPlaying = () => {
      setAutoplayBlocked(false);
      syncTracks();
    };
    const play = () => {
      void element.play().catch(() => {
        if (!disposed) setAutoplayBlocked(true);
      });
    };
    element.addEventListener('playing', onPlaying);
    element.addEventListener('loadedmetadata', syncTracks);
    element.addEventListener('loadeddata', syncTracks);
    element.addEventListener('resize', syncTracks);
    element.textTracks.addEventListener('addtrack', syncTracks);
    element.textTracks.addEventListener('removetrack', syncTracks);
    element.textTracks.addEventListener('change', syncTracks);
    nativeAudio?.addEventListener('addtrack', syncTracks);
    nativeAudio?.addEventListener('removetrack', syncTracks);
    nativeAudio?.addEventListener('change', syncTracks);
    const hls =
      /mpegurl/i.test(source.mimeType ?? '') ||
      /\.m3u8$/i.test(new URL(source.url, document.baseURI).pathname);
    async function attach() {
      if (!hls) {
        element!.src = source!.url;
        element!.load();
        syncTracks();
        play();
        return;
      }
      const nativeHls = !!element!.canPlayType('application/vnd.apple.mpegurl');
      const Hls = await import('hls.js').then((module) => module.default).catch(() => null);
      if (disposed) return;
      if (!Hls || !Hls.isSupported()) {
        if (nativeHls) {
          element!.src = source!.url;
          element!.load();
          syncTracks();
          play();
        } else onError();
        return;
      }
      instance = new Hls({ maxBufferLength: 30, maxMaxBufferLength: 60, backBufferLength: 30 });
      hlsRef.current = instance;
      instance.on(Hls.Events.ERROR, (_event, data) => {
        if (data.fatal && !disposed) onError();
      });
      instance.on(Hls.Events.MANIFEST_PARSED, () => {
        syncTracks();
        if (!disposed) play();
      });
      instance.on(Hls.Events.LEVELS_UPDATED, syncTracks);
      instance.on(Hls.Events.LEVEL_LOADED, syncTracks);
      instance.on(Hls.Events.LEVEL_SWITCHED, syncTracks);
      instance.on(Hls.Events.AUDIO_TRACKS_UPDATED, syncTracks);
      instance.on(Hls.Events.AUDIO_TRACK_SWITCHED, syncTracks);
      instance.on(Hls.Events.SUBTITLE_TRACKS_UPDATED, syncTracks);
      instance.on(Hls.Events.SUBTITLE_TRACK_SWITCH, syncTracks);
      instance.loadSource(source!.url);
      instance.attachMedia(element!);
    }
    void attach().catch(() => {
      if (!disposed) onError();
    });
    return () => {
      disposed = true;
      element.removeEventListener('playing', onPlaying);
      element.removeEventListener('loadedmetadata', syncTracks);
      element.removeEventListener('loadeddata', syncTracks);
      element.removeEventListener('resize', syncTracks);
      element.textTracks.removeEventListener('addtrack', syncTracks);
      element.textTracks.removeEventListener('removetrack', syncTracks);
      element.textTracks.removeEventListener('change', syncTracks);
      nativeAudio?.removeEventListener('addtrack', syncTracks);
      nativeAudio?.removeEventListener('removetrack', syncTracks);
      nativeAudio?.removeEventListener('change', syncTracks);
      instance?.destroy();
      hlsRef.current = null;
      element.pause();
      element.removeAttribute('src');
      element.load();
    };
  }, [video, source, onError]);
  return {
    qualities,
    quality,
    setQuality,
    audioTracks,
    audioTrack,
    setAudioTrack,
    subtitleTracks,
    subtitleTrack,
    setSubtitleTrack,
    autoplayBlocked,
  };
}
