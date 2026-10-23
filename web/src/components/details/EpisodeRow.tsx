import { memo } from 'react';

import type { Episode, WatchProgress } from '../../model';

import { Icon } from '../common/Icon';
import { EpisodeInfo } from './EpisodeInfo';
import { WatchIndicator } from './WatchIndicator';

export const EpisodeRow = memo(function EpisodeRow({
  episode,
  onPlay,
  onPreload,
  progress,
}: {
  episode: Episode;
  progress?: WatchProgress;
  onPlay: (episode: Episode) => void;
  onPreload?: (episode: Episode) => void;
}) {
  return (
    <li>
      <div
        className="episode-entry"
        onPointerEnter={(event) => {
          if (event.pointerType === 'mouse') onPreload?.(episode);
        }}
        onFocusCapture={() => onPreload?.(episode)}
        onPointerDown={() => onPreload?.(episode)}
      >
        <button
          className="episode-play"
          onClick={() => onPlay(episode)}
          aria-label={`Play episode ${episode.number}: ${episode.name}`}
        />
        <span className="episode-number">{episode.number}</span>
        <span className="episode-thumbnail">
          {episode.stillUrl ? (
            <img src={episode.stillUrl} alt="" loading="lazy" decoding="async" />
          ) : (
            <Icon name="play" />
          )}
          <WatchIndicator progress={progress} />
        </span>
        <div className="episode-copy">
          <span className="episode-title">{episode.name}</span>
          {episode.overview && <EpisodeInfo episode={episode} />}
        </div>
        <span className="episode-actions">
          <span className="episode-play-icon">
            <Icon name="play" />
          </span>
          {episode.runtime ? (
            <span className="episode-runtime">{episode.runtime} min</span>
          ) : null}
        </span>
      </div>
    </li>
  );
});
