import type { CSSProperties } from 'react';

import { formatPlaybackTime, type usePlayerControls } from '../../hooks/usePlayerControls';
import { Icon, type IconName } from '../common/Icon';

export type PlayerPanel = 'speed' | 'source';
export const PLAYBACK_SPEEDS = [0.5, 0.75, 1, 1.25, 1.5, 2] as const;

/** Bottom chrome mirroring the iOS player: timeline with remaining time above a capsule of actions. */
export function PlayerControls({
  player,
  speed,
  fullscreen,
  canFullscreen,
  onFullscreen,
  onPanel,
  onNext,
}: {
  player: ReturnType<typeof usePlayerControls>;
  speed: number;
  fullscreen: boolean;
  canFullscreen: boolean;
  onFullscreen: () => void;
  onPanel: (panel: PlayerPanel) => void;
  onNext?: () => void;
}) {
  const remaining = Math.max(0, player.duration - player.time);
  return (
    <>
      <div className="player-timeline-row">
        <input
          className="player-timeline"
          type="range"
          aria-label="Playback position"
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
        <span className="player-time" aria-label={`${formatPlaybackTime(remaining)} remaining`}>
          −{formatPlaybackTime(remaining)}
        </span>
      </div>
      <div className="player-actions">
        <PlayerAction
          icon="speedometer"
          label={`${speed}×`}
          aria-label={`Playback speed, ${speed} times`}
          onClick={() => onPanel('speed')}
        />
        <PlayerAction icon="refresh" label="Source" onClick={() => onPanel('source')} />
        <div className="player-action player-volume">
          <button
            className="player-volume-toggle"
            aria-label={player.muted ? 'Unmute' : 'Mute'}
            onClick={player.toggleMute}
          >
            <Icon name={player.muted || player.volume === 0 ? 'muted' : 'volume'} />
          </button>
          <input
            type="range"
            aria-label="Volume"
            min={0}
            max={1}
            step={0.05}
            value={player.muted ? 0 : player.volume}
            onChange={(event) => player.changeVolume(Number(event.target.value))}
            style={
              {
                '--progress': `${(player.muted ? 0 : player.volume) * 100}%`,
              } as CSSProperties
            }
          />
        </div>
        {canFullscreen && (
          <PlayerAction
            icon={fullscreen ? 'collapse' : 'expand'}
            label={fullscreen ? 'Exit full screen' : 'Full screen'}
            short={fullscreen ? 'Exit' : 'Full'}
            onClick={onFullscreen}
          />
        )}
        {onNext && (
          <PlayerAction
            icon="skip-forward"
            label="Next episode"
            short="Next"
            onClick={onNext}
          />
        )}
      </div>
    </>
  );
}

function PlayerAction({
  icon,
  label,
  short,
  onClick,
  'aria-label': ariaLabel,
}: {
  icon: IconName;
  label: string;
  short?: string;
  onClick: () => void;
  'aria-label'?: string;
}) {
  return (
    <button className="player-action" aria-label={ariaLabel ?? label} onClick={onClick}>
      <Icon name={icon} />
      <span className={short ? 'player-action-full' : undefined}>{label}</span>
      {short && (
        <span className="player-action-short" aria-hidden="true">
          {short}
        </span>
      )}
    </button>
  );
}
