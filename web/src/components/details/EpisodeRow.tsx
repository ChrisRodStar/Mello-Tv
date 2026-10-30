import { memo, useEffect, useRef, useState } from 'react';

import type { Episode, WatchProgress } from '../../model';

import { Icon } from '../common/Icon';

export const EpisodeRow = memo(function EpisodeRow({
  episode,
  onPlay,
  onPreload,
  onRewatch,
  progress,
  isNextUp = false,
}: {
  episode: Episode;
  progress?: WatchProgress;
  isNextUp?: boolean;
  onPlay: (episode: Episode) => void;
  onPreload?: (episode: Episode) => void;
  onRewatch?: (episode: Episode) => void;
}) {
  const [isExpanded, setIsExpanded] = useState(false);
  const [menuOpen, setMenuOpen] = useState(false);
  const menuRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!menuOpen) return;
    const handleClickOutside = (event: MouseEvent) => {
      if (menuRef.current && !menuRef.current.contains(event.target as Node)) {
        setMenuOpen(false);
      }
    };
    document.addEventListener('pointerdown', handleClickOutside);
    return () => document.removeEventListener('pointerdown', handleClickOutside);
  }, [menuOpen]);

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
                {isNextUp && <span className="episode-eyebrow-next"> • UP NEXT</span>}
              </span>
              <span className="episode-title">{displayTitle}</span>
              {metadataText && <span className="episode-metadata">{metadataText}</span>}
            </button>

            <div className="episode-footer-actions">
              {episode.overview ? (
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
              ) : (
                <span />
              )}

              <div className="episode-menu-container" ref={menuRef}>
                <button
                  type="button"
                  className="episode-menu-button"
                  aria-label={`Options for episode ${episode.number}`}
                  aria-expanded={menuOpen}
                  onClick={() => setMenuOpen((prev) => !prev)}
                >
                  <svg
                    viewBox="0 0 24 24"
                    fill="currentColor"
                    width="16"
                    height="16"
                    aria-hidden="true"
                  >
                    <circle cx="5" cy="12" r="2" />
                    <circle cx="12" cy="12" r="2" />
                    <circle cx="19" cy="12" r="2" />
                  </svg>
                </button>
                {menuOpen && (
                  <div className="episode-menu-dropdown" role="menu">
                    <button
                      type="button"
                      role="menuitem"
                      className="episode-menu-item"
                      onClick={() => {
                        setMenuOpen(false);
                        if (onRewatch) onRewatch(episode);
                        else onPlay(episode);
                      }}
                    >
                      <svg
                        viewBox="0 0 24 24"
                        fill="none"
                        stroke="currentColor"
                        strokeWidth="2"
                        width="14"
                        height="14"
                        aria-hidden="true"
                      >
                        <path d="M3 12a9 9 0 1 0 9-9 9.75 9.75 0 0 0-6.74 2.74L3 8" />
                        <path d="M3 3v5h5" />
                      </svg>
                      <span>{isWatched ? 'Rewatch from beginning' : 'Play from beginning'}</span>
                    </button>
                  </div>
                )}
              </div>
            </div>
          </div>
        </div>

        {isExpanded && episode.overview && (
          <p className="episode-overview">{episode.overview}</p>
        )}
      </div>
    </li>
  );
});
