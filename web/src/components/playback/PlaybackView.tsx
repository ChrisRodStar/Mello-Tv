import { usePlaybackSession } from '../../hooks/usePlaybackSession';
import { useEffect, useRef, useState, type FocusEvent, type PointerEvent } from 'react';

import { useMediaSource } from '../../hooks/useMediaSource';
import { usePlayerControls } from '../../hooks/usePlayerControls';
import { usePlayerVisibility } from '../../hooks/usePlayerVisibility';
import { useWatchProgress } from '../../hooks/useWatchProgress';
import type { PlaybackRequest, PlaybackResolver } from '../../lib/playback';
import { Icon } from '../common/Icon';
import { PLAYBACK_SPEEDS, PlayerControls, type PlayerPanel } from './PlayerControls';

type SeekFeedback = { direction: -1 | 1; seconds: number; id: number };

export function PlaybackView({
  request,
  resolver,
  onBack,
  onNext,
}: {
  request: PlaybackRequest;
  resolver?: PlaybackResolver;
  onBack: () => void;
  onNext?: () => void;
}) {
  const session = usePlaybackSession(request, resolver);
  const { source, status } = session;
  const [fullscreen, setFullscreen] = useState(false);
  const [fullscreenError, setFullscreenError] = useState('');
  const [speed, setSpeed] = useState(1);
  const [panel, setPanel] = useState<PlayerPanel | null>(null);
  const [locked, setLocked] = useState(false);
  const [unlockVisible, setUnlockVisible] = useState(false);
  const [seekFeedback, setSeekFeedback] = useState<SeekFeedback | null>(null);
  // The first rendered frame for the current source, as on iOS. Until then the
  // player shows preparation instead of transport controls.
  const [startedURL, setStartedURL] = useState<string | null>(null);
  const stage = useRef<HTMLElement>(null);
  const video = useRef<HTMLVideoElement>(null);
  const lastTap = useRef<{ x: number; y: number } | null>(null);
  const pendingTap = useRef<{ timer: ReturnType<typeof setTimeout>; zone: number } | null>(
    null,
  );
  const unlockTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const feedbackTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const player = usePlayerControls(video);
  const mediaError = session.mediaError;
  const ready = status === 'ready' && !!source;
  useMediaSource(video, ready ? source : null, mediaError);
  const progress = useWatchProgress(video, request, ready ? source : null);
  const started = ready && startedURL === source.url;
  const preparing =
    status === 'loading' || (ready && !started && !(player.paused && !player.buffering));
  const chrome = usePlayerVisibility(
    ready && !preparing && !player.paused && !player.buffering && !panel,
  );
  const chromeVisible = !locked && (chrome.visible || !ready || preparing || !!panel);

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
      if (unlockTimer.current) clearTimeout(unlockTimer.current);
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

  async function toggleFullscreen() {
    try {
      if (document.fullscreenElement) await document.exitFullscreen();
      else {
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
    await progress.flush();
    if (document.fullscreenElement === stage.current) {
      try {
        await document.exitFullscreen();
      } catch {
        /* Leaving the player still restores details. */
      }
    }
    onBack();
  }
  function revealUnlock() {
    setUnlockVisible(true);
    if (unlockTimer.current) clearTimeout(unlockTimer.current);
    unlockTimer.current = setTimeout(() => setUnlockVisible(false), 2400);
  }
  function lock() {
    setPanel(null);
    setLocked(true);
    revealUnlock();
  }
  function unlock() {
    if (unlockTimer.current) clearTimeout(unlockTimer.current);
    setUnlockVisible(false);
    setLocked(false);
    chrome.wake();
    requestAnimationFrame(() => stage.current?.focus());
  }
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
  function runSourceAction(kind: 'backup' | 'refresh' | 'renew') {
    setPanel(null);
    void progress.flush().then(() => session.mutate(kind, player.time));
  }
  const chromeHandlers = {
    onPointerEnter: (event: PointerEvent) => {
      if (event.pointerType === 'mouse') chrome.setHovered(true);
    },
    onPointerLeave: () => chrome.setHovered(false),
    onFocusCapture: (event: FocusEvent) =>
      chrome.setFocused((event.target as HTMLElement).matches(':focus-visible')),
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
        if (event.pointerType !== 'mouse' || locked) return;
        const tap = lastTap.current;
        if (tap && Math.hypot(event.clientX - tap.x, event.clientY - tap.y) < 8) return;
        lastTap.current = null;
        chrome.wake();
      }}
      onPointerDown={(event) => {
        if ((event.target as HTMLElement).closest('button, input')) chrome.wake();
      }}
      onClick={(event) => {
        if (
          (event.target as HTMLElement).closest(
            'button, input, .playback-status, .player-panel',
          )
        )
          return;
        if (locked) {
          revealUnlock();
          return;
        }
        if (!ready || preparing) return;
        lastTap.current = { x: event.clientX, y: event.clientY };
        const bounds = event.currentTarget.getBoundingClientRect();
        const rotated = getComputedStyle(event.currentTarget).transform !== 'none';
        const fraction = rotated
          ? (event.clientY - bounds.top) / bounds.height
          : (event.clientX - bounds.left) / bounds.width;
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
        if (event.key === 'Escape' && panel) {
          event.preventDefault();
          event.stopPropagation();
          setPanel(null);
          stage.current?.focus();
          return;
        }
        if (locked) {
          if (event.key !== 'Escape') {
            revealUnlock();
            return;
          }
        } else chrome.wake();
        if ((event.target as HTMLElement).closest('button, input')) {
          if (!locked) chrome.setFocused(true);
          if (event.key !== 'Escape') return;
        }
        if (event.key === 'Escape') {
          event.preventDefault();
          event.stopPropagation();
          void back();
          return;
        }
        if (!ready || panel) return;
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
        if (event.key.toLowerCase() === 'f' && document.fullscreenEnabled)
          void toggleFullscreen();
      }}
    >
      {ready && (
        <video
          ref={video}
          key={source.url}
          autoPlay
          playsInline
          aria-label={`Playing ${request.type === 'tv' ? request.episodeTitle : request.title}`}
          onLoadedMetadata={(event) => {
            event.currentTarget.defaultPlaybackRate = speed;
            event.currentTarget.playbackRate = speed;
            player.sync();
          }}
          onTimeUpdate={player.sync}
          onVolumeChange={player.sync}
          onPlay={player.sync}
          onPause={player.sync}
          onEnded={() => {
            player.sync();
            onNext?.();
          }}
          onWaiting={() => player.setBuffering(true)}
          onPlaying={() => {
            player.sync();
            player.setBuffering(false);
            setStartedURL(source.url);
          }}
          onCanPlay={() => player.setBuffering(false)}
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
                void progress.flush().finally(() => session.retry(player.time));
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
                Refresh source
              </button>
            </div>
          )}
        </div>
      ) : (
        <button
          className="player-transport player-chrome"
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
      {locked && player.buffering && !player.paused && (
        <div className="player-buffering" role="status" aria-label="Buffering video">
          <span />
        </div>
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
      <header className="player-header player-chrome" {...chromeHandlers}>
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
        {started && (
          <button
            className="player-round-button"
            aria-label="Lock playback controls"
            onClick={lock}
          >
            <Icon name="lock-open" />
          </button>
        )}
      </header>
      {ready && !preparing && (
        <footer className="player-footer player-chrome" {...chromeHandlers}>
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
            fullscreen={fullscreen}
            canFullscreen={!!document.fullscreenEnabled}
            onFullscreen={() => void toggleFullscreen()}
            onPanel={setPanel}
            onNext={onNext}
          />
        </footer>
      )}
      {panel && (
        <div className="player-panel-layer">
          <button
            className="player-panel-dismiss"
            aria-label="Dismiss playback menu"
            tabIndex={-1}
            onClick={() => setPanel(null)}
          />
          <div
            className="player-panel"
            role="dialog"
            aria-modal="true"
            aria-labelledby="player-panel-title"
          >
            <div className="player-panel-header">
              <h3 id="player-panel-title">
                {panel === 'speed' ? 'Playback speed' : 'Playback source'}
              </h3>
              <button
                className="player-panel-close"
                aria-label="Close playback menu"
                autoFocus
                onClick={() => {
                  setPanel(null);
                  stage.current?.focus();
                }}
              >
                <Icon name="close" />
              </button>
            </div>
            {panel === 'speed' ? (
              <div
                className="player-speed-options"
                role="radiogroup"
                aria-label="Playback speed"
              >
                {PLAYBACK_SPEEDS.map((value) => (
                  <button
                    key={value}
                    role="radio"
                    aria-checked={speed === value}
                    aria-label={value === 1 ? 'Normal speed' : `${value} times speed`}
                    className={speed === value ? 'is-selected' : undefined}
                    onClick={() => {
                      setSpeed(value);
                      setPanel(null);
                      stage.current?.focus();
                    }}
                  >
                    {value}×
                  </button>
                ))}
              </div>
            ) : (
              <div className="player-source-options">
                <button disabled={session.busy} onClick={() => runSourceAction('backup')}>
                  <strong>Try backup</strong>
                  <span>Switch to the next available source</span>
                </button>
                <button disabled={session.busy} onClick={() => runSourceAction('refresh')}>
                  <strong>Refresh source</strong>
                  <span>Reload the current source</span>
                </button>
                <button disabled={session.busy} onClick={() => runSourceAction('renew')}>
                  <strong>Extend playback</strong>
                  <span>Renew this playback session</span>
                </button>
              </div>
            )}
          </div>
        </div>
      )}
      {locked && unlockVisible && (
        <button className="player-unlock" onClick={unlock}>
          <Icon name="lock" />
          Unlock controls
        </button>
      )}
    </section>
  );
}
