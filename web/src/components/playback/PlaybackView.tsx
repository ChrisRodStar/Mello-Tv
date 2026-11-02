import { usePlaybackSession } from '../../hooks/usePlaybackSession';
import { useEffect, useRef, useState, type FocusEvent } from 'react';

import { useMediaSource } from '../../hooks/useMediaSource';
import { usePlayerControls } from '../../hooks/usePlayerControls';
import { usePlayerVisibility } from '../../hooks/usePlayerVisibility';
import { useWatchProgress } from '../../hooks/useWatchProgress';
import type { WatchProgress, Episode } from '../../model';
import { PlayerEpisodes } from './PlayerEpisodes';
import type { PlaybackRequest, PlaybackResolver } from '../../lib/playback';
import { Icon } from '../common/Icon';
import { PLAYBACK_SPEEDS, PlayerControls, type PlayerPanel } from './PlayerControls';

type SeekFeedback = { direction: -1 | 1; seconds: number; id: number };

export function PlaybackView({
  request,
  resolver,
  onBack,
  onNext,
  onEpisode,
  progress: savedProgress,
}: {
  request: PlaybackRequest;
  resolver?: PlaybackResolver;
  onBack: () => void;
  onNext?: () => void;
  onEpisode?: (season: number, episode: Episode) => void;
  progress?: WatchProgress[];
}) {
  const session = usePlaybackSession(request, resolver);
  const { source, status } = session;
  const [fullscreen, setFullscreen] = useState(false);
  const [fullscreenError, setFullscreenError] = useState('');
  const [speed, setSpeed] = useState(1);
  const [panel, setPanel] = useState<PlayerPanel | null>(null);
  const [seekFeedback, setSeekFeedback] = useState<SeekFeedback | null>(null);
  // The first rendered frame for the current source, as on iOS. Until then the
  // player shows preparation instead of transport controls.
  const [startedURL, setStartedURL] = useState<string | null>(null);
  const stage = useRef<HTMLElement>(null);
  const panelNode = useRef<HTMLDivElement>(null);
  const panelTrigger = useRef<HTMLElement | null>(null);
  const keyboardInteraction = useRef(false);
  const transitioning = useRef(false);
  const video = useRef<HTMLVideoElement>(null);
  const lastTap = useRef<{ x: number; y: number } | null>(null);
  const pendingTap = useRef<{ timer: ReturnType<typeof setTimeout>; zone: number } | null>(
    null,
  );
  const feedbackTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const player = usePlayerControls(video);
  const mediaError = session.mediaError;
  const ready = status === 'ready' && !!source;
  const media = useMediaSource(video, ready ? source : null, mediaError);
  const progress = useWatchProgress(video, request, ready ? source : null);
  const started = ready && startedURL === source.url;
  const preparing =
    status === 'loading' ||
    (ready && !started && !media.autoplayBlocked && !(player.paused && !player.buffering));
  const chrome = usePlayerVisibility(
    ready && !preparing && !player.paused && !player.buffering && !player.scrubbing && !panel,
  );
  const chromeVisible = chrome.visible || !ready || preparing || !!panel;
  const canFullscreen =
    !!document.fullscreenEnabled ||
    typeof (video.current as (HTMLVideoElement & { webkitEnterFullscreen?: () => void }) | null)
      ?.webkitEnterFullscreen === 'function';

  useEffect(() => {
    stage.current?.focus();
    const previous = document.body.style.overflow;
    document.body.style.overflow = 'hidden';
    function update() {
      setFullscreen(document.fullscreenElement === stage.current);
      if (!document.fullscreenElement) screen.orientation?.unlock?.();
    }
    document.addEventListener('fullscreenchange', update);
    return () => {
      document.body.style.overflow = previous;
      document.removeEventListener('fullscreenchange', update);
      screen.orientation?.unlock?.();
      if (pendingTap.current) clearTimeout(pendingTap.current.timer);
      if (feedbackTimer.current) clearTimeout(feedbackTimer.current);
    };
  }, []);
  useEffect(() => {
    const element = video.current;
    if (!element) return;
    // Speed persists across pauses, source changes and episode changes.
    element.defaultPlaybackRate = speed;
    element.playbackRate = speed;
  }, [speed, source]);
  useEffect(() => {
    if (!ready) setPanel(null);
  }, [ready]);

  useEffect(() => {
    if (!ready || media.autoplayBlocked || (!preparing && (!player.buffering || player.paused)))
      return;
    const timer = setTimeout(mediaError, 45000);
    return () => clearTimeout(timer);
  }, [
    ready,
    source?.url,
    preparing,
    player.buffering,
    player.paused,
    media.autoplayBlocked,
    mediaError,
  ]);

  async function toggleFullscreen() {
    try {
      if (document.fullscreenElement) await document.exitFullscreen();
      else if (!document.fullscreenEnabled) {
        (
          video.current as (HTMLVideoElement & { webkitEnterFullscreen?: () => void }) | null
        )?.webkitEnterFullscreen?.();
      } else {
        await stage.current?.requestFullscreen();
        if (window.matchMedia('(pointer: coarse)').matches) {
          const orientation = screen.orientation as ScreenOrientation & {
            lock?: (orientation: string) => Promise<void>;
          };
          // Request landscape only from the explicit fullscreen action.
          try {
            await orientation?.lock?.('landscape');
          } catch {
            /* The player follows the device's orientation. */
          }
        }
      }
      setFullscreenError('');
    } catch {
      setFullscreenError('Fullscreen unavailable');
    }
  }
  async function back() {
    if (transitioning.current) return;
    transitioning.current = true;
    video.current?.pause();
    void progress.flush();
    if (document.fullscreenElement === stage.current) {
      try {
        await document.exitFullscreen();
      } catch {
        /* Leaving the player still restores details. */
      }
    }
    onBack();
  }
  function openPanel(value: PlayerPanel) {
    panelTrigger.current = document.activeElement as HTMLElement | null;
    setPanel(value);
  }
  function closePanel() {
    setPanel(null);
    chrome.wake();
    requestAnimationFrame(() => {
      if (panelTrigger.current?.isConnected) panelTrigger.current.focus();
      else stage.current?.focus();
    });
  }
  function next() {
    if (!onNext || transitioning.current) return;
    transitioning.current = true;
    video.current?.pause();
    void progress.flush();
    onNext();
  }
  useEffect(() => {
    transitioning.current = false;
    setPanel(null);
    setStartedURL(null);
    if (pendingTap.current) clearTimeout(pendingTap.current.timer);
    pendingTap.current = null;
  }, [request]);
  useEffect(() => {
    if (!panel) return;
    panelNode.current?.querySelector<HTMLElement>('button, select')?.focus();
  }, [panel]);
  function seekBy(direction: -1 | 1) {
    if (!player.duration) return;
    player.seek(player.time + direction * 10);
    setSeekFeedback((previous) => ({
      direction,
      seconds: previous?.direction === direction ? previous.seconds + 10 : 10,
      id: Date.now(),
    }));
    if (feedbackTimer.current) clearTimeout(feedbackTimer.current);
    feedbackTimer.current = setTimeout(() => setSeekFeedback(null), 750);
  }
  const chromeHandlers = {
    onFocusCapture: () => chrome.setFocused(keyboardInteraction.current),
    onBlurCapture: (event: FocusEvent) => {
      if (!event.currentTarget.contains(event.relatedTarget as Node | null))
        chrome.setFocused(false);
    },
  };

  return (
    <section
      ref={stage}
      tabIndex={-1}
      className={chromeVisible ? 'playback-view' : 'playback-view player-idle'}
      aria-labelledby="detail-title"
      onPointerMove={(event) => {
        if (event.pointerType !== 'mouse') return;
        const tap = lastTap.current;
        if (tap && Math.hypot(event.clientX - tap.x, event.clientY - tap.y) < 8) return;
        lastTap.current = null;
        chrome.wake();
      }}
      onPointerDown={(event) => {
        keyboardInteraction.current = false;
        chrome.setFocused(false);
        if ((event.target as HTMLElement).closest('button, input, select')) chrome.wake();
      }}
      onClick={(event) => {
        if (
          (event.target as HTMLElement).closest(
            'button, input, select, .playback-status, .player-panel, .player-episodes',
          )
        )
          return;
        if (!ready || preparing) return;
        lastTap.current = { x: event.clientX, y: event.clientY };
        const bounds = event.currentTarget.getBoundingClientRect();
        const fraction = (event.clientX - bounds.left) / bounds.width;
        const zone = fraction < 0.35 ? -1 : fraction > 0.65 ? 1 : 0;
        const previous = pendingTap.current;
        if (previous) {
          clearTimeout(previous.timer);
          pendingTap.current = null;
          if (zone && zone === previous.zone) seekBy(zone as -1 | 1);
          else chrome.toggle();
        } else {
          pendingTap.current = {
            zone,
            timer: setTimeout(() => {
              pendingTap.current = null;
              chrome.toggle();
            }, 250),
          };
        }
      }}
      onKeyDown={(event) => {
        keyboardInteraction.current = true;
        if (event.key === 'Tab' && !panel) {
          chrome.wake();
          if (!chromeVisible) {
            event.preventDefault();
            const backwards = event.shiftKey;
            chrome.setFocused(true);
            // Wait for the chrome to render and become focusable after waking.
            requestAnimationFrame(() =>
              requestAnimationFrame(() => {
                const controls = stage.current?.querySelectorAll<HTMLElement>(
                  '.player-header button, .player-footer button, .player-footer input:not(:disabled)',
                );
                (backwards ? controls?.[controls.length - 1] : controls?.[0])?.focus();
              }),
            );
            return;
          }
        }
        if (panel && event.key === 'Tab') {
          const items = panelNode.current?.querySelectorAll<HTMLElement>(
            'button:not(:disabled), select:not(:disabled), input:not(:disabled), [tabindex="0"]',
          );
          if (items?.length) {
            const first = items[0],
              last = items[items.length - 1];
            if (
              event.shiftKey &&
              (document.activeElement === first ||
                !panelNode.current?.contains(document.activeElement))
            ) {
              event.preventDefault();
              last?.focus();
            } else if (
              !event.shiftKey &&
              (document.activeElement === last ||
                !panelNode.current?.contains(document.activeElement))
            ) {
              event.preventDefault();
              first?.focus();
            }
          }
          return;
        }
        if (event.key === 'Escape' && panel) {
          event.preventDefault();
          event.stopPropagation();
          closePanel();
          return;
        }
        chrome.wake();
        if ((event.target as HTMLElement).closest('button, input, select')) {
          chrome.setFocused(true);
          if (event.key !== 'Escape') return;
        }
        if (event.key === 'Escape') {
          event.preventDefault();
          event.stopPropagation();
          void back();
          return;
        }
        if (!ready || panel || event.altKey || event.ctrlKey || event.metaKey) return;
        if (event.key === ' ' || event.key.toLowerCase() === 'k') {
          event.preventDefault();
          void player.toggle();
        }
        if (event.key === 'ArrowLeft') {
          event.preventDefault();
          seekBy(-1);
        }
        if (event.key === 'ArrowRight') {
          event.preventDefault();
          seekBy(1);
        }
        if (event.key.toLowerCase() === 'm') player.toggleMute();
        if (event.key.toLowerCase() === 'f' && canFullscreen) void toggleFullscreen();
      }}
    >
      {ready && (
        <video
          ref={player.attach}
          key={source.url}
          autoPlay
          playsInline
          aria-label={`Playing ${request.type === 'tv' ? request.episodeTitle : request.title}`}
          onLoadedMetadata={(event) => {
            event.currentTarget.defaultPlaybackRate = speed;
            event.currentTarget.playbackRate = speed;
            player.sync();
          }}
          onDurationChange={player.sync}
          onEmptied={player.sync}
          onTimeUpdate={player.sync}
          onVolumeChange={player.sync}
          onPlay={player.sync}
          onPause={player.sync}
          onEnded={() => {
            player.sync();
            next();
          }}
          onWaiting={() => player.setBuffering(true)}
          onPlaying={() => {
            player.sync();
            player.setBuffering(false);
            setStartedURL(source.url);
          }}
          onCanPlay={() => {
            player.sync();
            player.setBuffering(false);
          }}
          onError={mediaError}
        />
      )}
      <div className="player-scrim player-chrome" aria-hidden="true" />
      {preparing ? (
        <div className="playback-status player-preparing" role="status">
          <span className="player-spinner" aria-hidden="true" />
          <p className="player-status-title">
            {status === 'loading' ? 'Finding a playable source' : 'Preparing video'}
          </p>
          <p className="player-status-detail">{request.title}</p>
        </div>
      ) : !ready ? (
        <div className="playback-status" role={status === 'error' ? 'alert' : 'status'}>
          <span className="player-status-icon" aria-hidden="true">
            <Icon name="prohibit" />
          </span>
          <h3>{status === 'error' ? 'Couldn’t play this title' : 'Playback unavailable'}</h3>
          {session.error && <p className="player-status-message">{session.error}</p>}
          {(status === 'error' || status === 'unavailable') && (
            <button
              className="player-retry"
              onClick={() => {
                void progress.flush();
                session.retry(player.time);
              }}
            >
              Try again
            </button>
          )}
          {source && (
            <div className="player-status-actions">
              <button
                disabled={session.busy}
                onClick={() => void session.mutate('backup', player.time)}
              >
                Try backup
              </button>
              <button
                disabled={session.busy}
                onClick={() => void session.mutate('refresh', player.time)}
              >
                Reload video
              </button>
            </div>
          )}
        </div>
      ) : (
        <button
          className="player-transport player-chrome"
          inert={!!panel || !chromeVisible}
          aria-label={player.paused ? 'Play' : player.buffering ? 'Pause, buffering' : 'Pause'}
          onClick={() => void player.toggle()}
        >
          {player.buffering && !player.paused ? (
            <span className="player-spinner" aria-hidden="true" />
          ) : (
            <Icon name={player.paused ? 'play' : 'pause'} />
          )}
        </button>
      )}
      {seekFeedback && (
        <div
          key={seekFeedback.id}
          className={`player-seek-feedback ${seekFeedback.direction > 0 ? 'is-forward' : 'is-back'}`}
          aria-hidden="true"
        >
          <Icon name={seekFeedback.direction > 0 ? 'skip-forward' : 'skip-back'} />
          {seekFeedback.direction > 0 ? '+' : '−'}
          {seekFeedback.seconds}s
        </div>
      )}
      <header
        inert={!!panel || !chromeVisible}
        className="player-header player-chrome"
        {...chromeHandlers}
      >
        <button
          className="player-round-button"
          aria-label="Close player"
          onClick={() => void back()}
        >
          <Icon name="close" />
        </button>
        <div className="player-title">
          <h2 id="detail-title">{request.title}</h2>
          {request.type === 'tv' && (
            <p>
              S{request.season} · E{request.episode}
              {request.episodeTitle ? ` · ${request.episodeTitle}` : ''}
            </p>
          )}
        </div>
        {canFullscreen && (
          <button
            className="player-round-button"
            aria-label={fullscreen ? 'Exit full screen' : 'Full screen'}
            title={fullscreen ? 'Exit full screen' : 'Full screen'}
            onClick={() => void toggleFullscreen()}
          >
            <Icon name={fullscreen ? 'collapse' : 'expand'} />
          </button>
        )}
      </header>
      {ready && !preparing && (
        <footer
          inert={!!panel || !chromeVisible}
          className="player-footer player-chrome"
          {...chromeHandlers}
        >
          {session.error && (
            <p className="player-notice" role="alert">
              {session.error}
            </p>
          )}
          {fullscreenError && (
            <p className="player-notice" role="status">
              {fullscreenError}
            </p>
          )}
          <PlayerControls
            player={player}
            speed={speed}
            onPanel={openPanel}
            qualityLabel={
              media.qualities.find((item) => item.id === media.quality)?.label ?? 'Auto'
            }
            hasEpisodes={request.type === 'tv' && !!onEpisode}
            onNext={onNext ? next : undefined}
          />
        </footer>
      )}
      {panel && panel !== 'episodes' && (
        <div className="player-panel-layer">
          <button
            className="player-panel-dismiss"
            aria-label="Dismiss playback menu"
            tabIndex={-1}
            onClick={closePanel}
          />
          <div
            ref={panelNode}
            className="player-panel"
            role="dialog"
            aria-modal="true"
            aria-labelledby="player-panel-title"
          >
            <div className="player-panel-header">
              <h3 id="player-panel-title">
                {panel === 'speed'
                  ? 'Playback speed'
                  : panel === 'quality'
                    ? 'Video quality'
                    : 'Audio & subtitles'}
              </h3>
              <button
                className="player-panel-close"
                aria-label="Close playback menu"
                autoFocus
                onClick={closePanel}
              >
                <Icon name="close" />
              </button>
            </div>
            {panel === 'speed' ? (
              <div className="player-speed-options" role="group" aria-label="Playback speed">
                {PLAYBACK_SPEEDS.map((value) => (
                  <button
                    key={value}
                    aria-pressed={speed === value}
                    aria-label={value === 1 ? 'Normal speed' : `${value} times speed`}
                    className={speed === value ? 'is-selected' : undefined}
                    onClick={() => {
                      setSpeed(value);
                      closePanel();
                    }}
                  >
                    {value}×
                  </button>
                ))}
              </div>
            ) : panel === 'quality' ? (
              <div className="player-choice-options" role="group" aria-label="Video quality">
                {[{ id: -1, label: 'Auto' }, ...media.qualities].map((item) => (
                  <button
                    key={item.id}
                    aria-pressed={media.quality === item.id}
                    className={media.quality === item.id ? 'is-selected' : ''}
                    onClick={() => media.setQuality(item.id)}
                  >
                    <span>{item.label}</span>
                    {media.quality === item.id && <Icon name="check" />}
                  </button>
                ))}
                <p className="player-panel-note">
                  {media.qualities.length
                    ? 'Auto adjusts to your connection.'
                    : 'Quality is managed automatically for this video.'}
                </p>
              </div>
            ) : panel === 'tracks' ? (
              <div className="player-track-columns">
                <div className="player-choice-options" role="group" aria-label="Audio">
                  <h4>Audio</h4>
                  {media.audioTracks.length ? (
                    media.audioTracks.map((item) => (
                      <button
                        key={item.id}
                        aria-pressed={media.audioTrack === item.id}
                        className={media.audioTrack === item.id ? 'is-selected' : ''}
                        onClick={() => media.setAudioTrack(item.id)}
                      >
                        <span>{item.label}</span>
                        {media.audioTrack === item.id && <Icon name="check" />}
                      </button>
                    ))
                  ) : (
                    <p className="player-panel-note">Default audio</p>
                  )}
                </div>
                <div className="player-choice-options" role="group" aria-label="Subtitles">
                  <h4>Subtitles</h4>
                  {[{ id: -1, label: 'Off' }, ...media.subtitleTracks].map((item) => (
                    <button
                      key={item.id}
                      aria-pressed={media.subtitleTrack === item.id}
                      className={media.subtitleTrack === item.id ? 'is-selected' : ''}
                      onClick={() => media.setSubtitleTrack(item.id)}
                    >
                      <span>{item.label}</span>
                      {media.subtitleTrack === item.id && <Icon name="check" />}
                    </button>
                  ))}
                  {!media.subtitleTracks.length && (
                    <p className="player-panel-note">
                      No subtitles are available in this stream.
                    </p>
                  )}
                </div>
              </div>
            ) : null}
          </div>
        </div>
      )}
      {panel === 'episodes' && request.type === 'tv' && onEpisode && (
        <div ref={panelNode} className="player-episodes-layer">
          <PlayerEpisodes
            progress={savedProgress}
            request={request}
            onClose={closePanel}
            onSelect={(season, episode) => {
              if (season === request.season && episode.number === request.episode) {
                closePanel();
                return;
              }
              if (transitioning.current) return;
              transitioning.current = true;
              video.current?.pause();
              void progress.flush();
              setPanel(null);
              onEpisode(season, episode);
            }}
          />
        </div>
      )}
    </section>
  );
}
