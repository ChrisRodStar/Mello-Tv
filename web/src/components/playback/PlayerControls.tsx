import type { CSSProperties } from 'react';

import { formatPlaybackTime, type usePlayerControls } from '../../hooks/usePlayerControls';
import { Icon } from '../common/Icon';

export function PlayerControls({
  player,
  fullscreen,
  canFullscreen,
  onFullscreen,
  onNext,
}: {
  player: ReturnType<typeof usePlayerControls>;
  fullscreen: boolean;
  canFullscreen: boolean;
  onFullscreen: () => void;
  onNext?: () => void;
}) {
  return (
    <>
      <input
        className="player-timeline"
        type="range"
        aria-label="Seek playback"
        aria-valuetext={`${formatPlaybackTime(player.time)} of ${formatPlaybackTime(player.duration)}`}
        min={0}
        max={player.duration || 1}
        step={0.1}
        value={player.time}
        disabled={!player.duration}
        onChange={(event) => player.seek(Number(event.target.value))}
        style={
          {
            '--progress': `${player.duration ? (player.time / player.duration) * 100 : 0}%`,
          } as CSSProperties
        }
      />
      <div className="player-transport">
        <div className="player-controls-left">
          <button
            className="player-control player-skip"
            aria-label="Back 10 seconds"
            onClick={() => player.seek(player.time - 10)}
          >
            <Icon name="rewind" />
            <span>10</span>
          </button>
          <button
            className="player-control player-toggle"
            aria-label={player.paused ? 'Play' : 'Pause'}
            onClick={() => void player.toggle()}
          >
            <Icon name={player.paused ? 'play' : 'pause'} />
          </button>
          <button
            className="player-control player-skip"
            aria-label="Forward 10 seconds"
            onClick={() => player.seek(player.time + 10)}
          >
            <Icon name="forward" />
            <span>10</span>
          </button>
        </div>
      </div>
      <div className="player-controls-row">
        <span className="player-time">
          {formatPlaybackTime(player.time)}
          <span> / {formatPlaybackTime(player.duration)}</span>
        </span>
        <div className="player-controls-right">
          {onNext && (
            <button className="player-control" aria-label="Next episode" onClick={onNext}>
              <Icon name="right" />
            </button>
          )}
          <div className="player-volume">
            <button
              className="player-control"
              aria-label={player.muted ? 'Unmute' : 'Mute'}
              onClick={player.toggleMute}
            >
              <Icon name={player.muted ? 'muted' : 'volume'} />
            </button>
            <input
              type="range"
              aria-label="Volume"
              min={0}
              max={1}
              step={0.05}
              value={player.muted ? 0 : player.volume}
              onChange={(event) => player.changeVolume(Number(event.target.value))}
            />
          </div>
          {canFullscreen && (
            <button
              className="player-control"
              aria-label={fullscreen ? 'Exit fullscreen' : 'Enter fullscreen'}
              onClick={onFullscreen}
            >
              <Icon name={fullscreen ? 'collapse' : 'expand'} />
            </button>
          )}
        </div>
      </div>
    </>
  );
}
