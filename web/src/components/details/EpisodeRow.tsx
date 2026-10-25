import { memo, useState } from 'react';

import type { Episode, WatchProgress } from '../../model';

import { Icon } from '../common/Icon';

export const EpisodeRow = memo(function EpisodeRow({
  episode,
  onPlay,
  onPreload,
  progress,
  isNextUp = false,
}: {
  episode: Episode;
  progress?: WatchProgress;
  isNextUp?: boolean;
  onPlay: (episode: Episode) => void;
  onPreload?: (episode: Episode) => void;
}) {
  const [isExpanded, setIsExpanded] = useState(false);

  const isWatched = Boolean(progress?.completed);
  const hasProgress =
    !isWatched &&
    progress !== undefined &&
    Number.isFinite(progress.duration) &&
    Number.isFinite(progress.position) &&
    progress.duration > 0 &&
    progress.position > 0;

  const progressFraction = isWatched
    ? 1
    : hasProgress && progress
      ? Math.min(1, Math.max(0, progress.position / progress.duration))
      : null;

  const percentPlayed =
    progressFraction !== null ? Math.min(100, Math.round(progressFraction * 100)) : null;

  const metadataText = isWatched
    ? 'Watched'
    : hasProgress && progress
      ? `${Math.max(1, Math.ceil((progress.duration - progress.position) / 60))} min remaining`
      : episode.runtime && episode.runtime > 0
        ? `${episode.runtime} min`
        : '';

  const displayTitle = episode.name.trim() || `Episode ${episode.number}`;

  return (
    <li>
      <div
        className={`episode-entry${isNextUp ? ' is-next-up' : ''}${isWatched ? ' is-watched' : ''}`}
        onPointerEnter={(event) => {
          if (event.pointerType === 'mouse') onPreload?.(episode);
        }}
        onFocusCapture={() => onPreload?.(episode)}
        onPointerDown={() => onPreload?.(episode)}
      >
        <div className="episode-row-main">
          <button
            type="button"
            className="episode-thumbnail"
            onClick={() => onPlay(episode)}
            aria-label={`Play episode ${episode.number}: ${displayTitle}`}
          >
            {episode.stillUrl ? (
              <img src={episode.stillUrl} alt="" loading="lazy" decoding="async" />
            ) : (
              <span className="episode-thumbnail-placeholder" aria-hidden="true">
                <svg viewBox="0 0 256 256" fill="none" stroke="currentColor" strokeWidth="16">
                  <rect x="32" y="48" width="192" height="144" rx="16" />
                  <path d="M96 224h64" strokeLinecap="round" />
                </svg>
              </span>
            )}
            <span className="episode-thumbnail-scrim" aria-hidden="true" />
            <span className="episode-thumbnail-play" aria-hidden="true">
              <Icon name="play" />
            </span>
            {progressFraction !== null && (
              <span
                className={`episode-progress-track${isWatched ? ' is-watched' : ''}`}
                aria-label={percentPlayed !== null ? `${percentPlayed}% played` : undefined}
              >
                <span
                  className="episode-progress-fill"
                  style={{ width: `${Math.round(progressFraction * 100)}%` }}
                />
              </span>
            )}
          </button>

          <div className="episode-copy">
            <button
              type="button"
              className="episode-copy-button"
              onClick={() => onPlay(episode)}
            >
              <span className={`episode-eyebrow${isNextUp ? ' is-next-up' : ''}`}>
                EPISODE {episode.number}
                {isNextUp ? ' • UP NEXT' : ''}
              </span>
              <span className="episode-title">{displayTitle}</span>
              {metadataText && <span className="episode-metadata">{metadataText}</span>}
            </button>

            {episode.overview && (
              <div className="episode-footer-actions">
                <button
                  type="button"
                  className="episode-synopsis-toggle"
                  aria-expanded={isExpanded}
                  aria-label={`${isExpanded ? 'Collapse' : 'Expand'} synopsis for episode ${episode.number}`}
                  onClick={() => setIsExpanded((prev) => !prev)}
                >
                  <span>{isExpanded ? 'Less' : 'Synopsis'}</span>
                  <span
                    className={`episode-synopsis-chevron${isExpanded ? ' is-expanded' : ''}`}
                    aria-hidden="true"
                  >
                    {isExpanded ? '⌃' : '⌄'}
                  </span>
                </button>
              </div>
            )}
          </div>
        </div>

        {isExpanded && episode.overview && (
          <p className="episode-overview">{episode.overview}</p>
        )}
      </div>
    </li>
  );
});
