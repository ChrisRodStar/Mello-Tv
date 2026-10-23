import { usePlaybackSession } from '../../hooks/usePlaybackSession';
import { useCallback, useEffect, useRef, useState } from 'react';

import { useMediaSource } from '../../hooks/useMediaSource';
import { usePlayerControls } from '../../hooks/usePlayerControls';
import { usePlayerVisibility } from '../../hooks/usePlayerVisibility';
import { useWatchProgress } from '../../hooks/useWatchProgress';
import {
  resolvePlaybackSource,
  type PlaybackRequest,
  type PlaybackResolver,
  type PlaybackSource,
} from '../../lib/playback';
import { Icon } from '../common/Icon';
import { PlayerControls } from './PlayerControls';

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
  const stage = useRef<HTMLElement>(null);
  const video = useRef<HTMLVideoElement>(null);
  const lastTap = useRef<{ x: number; y: number } | null>(null);
  const pendingTap = useRef<{ timer: ReturnType<typeof setTimeout>; zone: number } | null>(
    null,
  );
  const player = usePlayerControls(video);
  const mediaError = session.mediaError;
  useMediaSource(video, status === 'ready' ? source : null, mediaError);
  const progress = useWatchProgress(video, request, status === 'ready' ? source : null);
  const chrome = usePlayerVisibility(status === 'ready' && !player.paused && !player.buffering);
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
    };
  }, []);
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
  return (
    <section
      ref={stage}
      tabIndex={-1}
      className={
        chrome.visible || status !== 'ready' ? 'playback-view' : 'playback-view player-idle'
      }
      aria-labelledby="detail-title"
      onPointerMove={(event) => {
        if (event.pointerType !== 'mouse') return;
        const tap = lastTap.current;
        if (tap && Math.hypot(event.clientX - tap.x, event.clientY - tap.y) < 8) return;
        lastTap.current = null;
        chrome.wake();
      }}
      onPointerDown={(event) => {
        if ((event.target as HTMLElement).closest('button, input')) chrome.wake();
      }}
      onClick={(event) => {
        if ((event.target as HTMLElement).closest('button, input, .playback-status')) return;
        if (status !== 'ready') return;
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
          if (zone && zone === previous.zone) {
            player.seek(player.time + zone * 10);
            chrome.wake();
          } else chrome.toggle();
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
        chrome.wake();
        if ((event.target as HTMLElement).closest('button, input')) {
          chrome.setFocused(true);
          return;
        }
        if (event.key === 'Escape') {
          event.preventDefault();
          event.stopPropagation();
          void back();
          return;
        }
        if (status !== 'ready') return;
        if (event.key === ' ' || event.key.toLowerCase() === 'k') {
          event.preventDefault();
          void player.toggle();
        }
        if (event.key === 'ArrowLeft') {
          event.preventDefault();
          player.seek(player.time - 10);
        }
        if (event.key === 'ArrowRight') {
          event.preventDefault();
          player.seek(player.time + 10);
        }
        if (event.key.toLowerCase() === 'm') player.toggleMute();
        if (event.key.toLowerCase() === 'f' && document.fullscreenEnabled)
          void toggleFullscreen();
      }}
    >
      {status === 'ready' && source ? (
        <video
          ref={video}
          key={source.url}
          autoPlay
          playsInline
          aria-label={`Playing ${request.type === 'tv' ? request.episodeTitle : request.title}`}
          onLoadedMetadata={player.sync}
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
          }}
          onCanPlay={() => player.setBuffering(false)}
          onError={mediaError}
        />
      ) : (
        <div className="playback-status" role={status === 'error' ? 'alert' : 'status'}>
          <h3>
            {status === 'loading'
              ? 'Loading playback…'
              : status === 'error'
                ? 'Couldn’t play this title'
                : 'Playback unavailable'}
          </h3>
          {source && (
            <div className="player-source-actions">
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
          {session.error && <p role="alert">{session.error}</p>}
          {(status === 'error' || status === 'unavailable') && (
            <button
              className="detail-play-button"
              onClick={() => {
                void progress.flush().finally(() => session.retry(player.time));
              }}
            >
              Try again
            </button>
          )}
        </div>
      )}
      {status === 'ready' && player.buffering && (
        <div className="player-buffering" role="status" aria-label="Buffering video">
          <span />
        </div>
      )}
      <header
        className="player-header player-chrome"
        onPointerEnter={(event) => {
          if (event.pointerType === 'mouse') chrome.setHovered(true);
        }}
        onPointerLeave={() => chrome.setHovered(false)}
        onFocusCapture={(event) =>
          chrome.setFocused((event.target as HTMLElement).matches(':focus-visible'))
        }
        onBlurCapture={(event) => {
          if (!event.currentTarget.contains(event.relatedTarget as Node | null))
            chrome.setFocused(false);
        }}
      >
        <button
          className="player-control player-back"
          aria-label="Back to details"
          onClick={() => void back()}
        >
          <Icon name="left" />
        </button>
        <div className="player-title">
          <h2 id="detail-title">{request.title}</h2>
          {request.type === 'tv' && (
            <p>
              Season {request.season} · Episode {request.episode} · {request.episodeTitle}
            </p>
          )}
        </div>
      </header>
      {status === 'ready' && (
        <footer
          className="player-footer player-chrome"
          onPointerEnter={(event) => {
            if (event.pointerType === 'mouse') chrome.setHovered(true);
          }}
          onPointerLeave={() => chrome.setHovered(false)}
          onFocusCapture={(event) =>
            chrome.setFocused((event.target as HTMLElement).matches(':focus-visible'))
          }
          onBlurCapture={(event) => {
            if (!event.currentTarget.contains(event.relatedTarget as Node | null))
              chrome.setFocused(false);
          }}
        >
          <PlayerControls
            player={player}
            fullscreen={fullscreen}
            canFullscreen={!!document.fullscreenEnabled}
            onFullscreen={() => void toggleFullscreen()}
            onNext={onNext}
          />
          <div className="player-source-actions" aria-label="Playback recovery">
            <button
              disabled={session.busy}
              onClick={() => {
                void progress.flush().then(() => session.mutate('backup', player.time));
              }}
            >
              Try backup
            </button>
            <button
              disabled={session.busy}
              onClick={() => {
                void progress.flush().then(() => session.mutate('refresh', player.time));
              }}
            >
              Refresh source
            </button>
            <button
              disabled={session.busy}
              onClick={() => {
                void progress.flush().then(() => session.mutate('renew', player.time));
              }}
            >
              Extend playback
            </button>
          </div>
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
        </footer>
      )}
    </section>
  );
}
