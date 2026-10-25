import { useRef, useState } from 'react';

import type { Title, WatchProgress } from '../../model';

import { useProfileLibrary } from '../../hooks/useProfileLibrary';
import type { Category } from '../../types';
import { Icon } from '../common/Icon';
import { BrandTVMark, CardDetailsGlyph, MediaPlaceholderIcon } from './TitleCard';

export { BrandTVMark, CardDetailsGlyph };

function computeFraction(item: WatchProgress): number | null {
  if (
    !Number.isFinite(item.position) ||
    !Number.isFinite(item.duration) ||
    item.duration <= 0
  ) {
    return null;
  }
  return Math.min(1, Math.max(0, item.position / item.duration));
}

function computeRemaining(item: WatchProgress, fraction: number | null): string {
  if (item.completed) {
    return item.title.type === 'tv' ? 'Next episode' : 'Play again';
  }
  if (fraction === null) return 'Resume';
  const minutes = Math.ceil(Math.max(0, item.duration - item.position) / 60);
  if (!Number.isFinite(minutes)) return 'Resume';
  if (minutes < 1) return '<1m';
  return minutes >= 60
    ? `${Math.floor(minutes / 60)}h ${minutes % 60}m`
    : `${minutes}m`;
}

export type ContinueWatchingCardProps = {
  item: WatchProgress;
  isPoster?: boolean;
  showsDetailsButton?: boolean;
  isPreparingPlayback?: boolean;
  isPlaybackDisabled?: boolean;
  onPlay?: () => void;
  onResume?: (item: WatchProgress) => void | Promise<void>;
  onInfo?: () => void;
  onSelect?: (title: Title) => void;
};

export function ContinueWatchingCard({
  item,
  isPoster = false,
  showsDetailsButton = true,
  isPreparingPlayback = false,
  isPlaybackDisabled = false,
  onPlay,
  onResume,
  onInfo,
  onSelect,
}: ContinueWatchingCardProps) {
  const [internalLoading, setInternalLoading] = useState(false);
  const loading = isPreparingPlayback || internalLoading;
  const disabled = isPlaybackDisabled || loading;

  const fraction = computeFraction(item);
  const remaining = computeRemaining(item, fraction);
  const status =
    !isPoster && item.title.type === 'tv' && !item.completed
      ? `S${item.season}, E${item.episode} · ${remaining}`
      : remaining;

  const artworkUrl = isPoster
    ? (item.title.posterUrl ?? item.title.backdropUrl)
    : (item.title.backdropUrl ?? item.title.posterUrl);

  async function handlePlay() {
    if (disabled) return;
    if (onPlay) {
      onPlay();
      return;
    }
    if (onResume) {
      setInternalLoading(true);
      try {
        await onResume(item);
      } finally {
        setInternalLoading(false);
      }
    }
  }

  function handleInfo() {
    if (onInfo) {
      onInfo();
    } else if (onSelect) {
      onSelect(item.title);
    }
  }

  return (
    <div
      className={`continue-card-group${isPoster ? ' is-poster' : ''}${
        showsDetailsButton ? '' : ' no-details'
      }`}
    >
      <button
        type="button"
        className="continue-card"
        disabled={disabled}
        onClick={() => void handlePlay()}
        aria-label={`${item.completed ? 'Play' : 'Resume'} ${item.title.title} (${status})`}
      >
        <span className="continue-art">
          {artworkUrl ? (
            <img src={artworkUrl} alt="" loading="lazy" decoding="async" />
          ) : (
            <span className="continue-art-placeholder">
              <MediaPlaceholderIcon type={item.title.type} />
              <span>{item.title.title}</span>
            </span>
          )}
          <span className="continue-frost" aria-hidden="true" />
          <span className="continue-scrim" aria-hidden="true" />
          <BrandTVMark />
          <span className="continue-status-stack">
            {isPoster && item.title.type === 'tv' && (
              <span className="continue-poster-episode">
                S{item.season}, E{item.episode}
              </span>
            )}
            <span className="continue-status-row">
              {loading ? (
                <span className="continue-spinner" role="status" aria-label="Preparing playback" />
              ) : (
                <svg
                  className="continue-play-glyph"
                  viewBox="0 0 16 16"
                  fill="currentColor"
                  aria-hidden="true"
                >
                  <path d="M4.5 2.75a.75.75 0 0 1 1.14-.64l8.5 5.25a.75.75 0 0 1 0 1.28l-8.5 5.25a.75.75 0 0 1-1.14-.64V2.75Z" />
                </svg>
              )}
              {fraction !== null && (
                <span className="continue-progress-capsule" aria-hidden="true">
                  <span
                    className="continue-progress-fill"
                    style={{ width: `${Math.round(fraction * 100)}%` }}
                  />
                </span>
              )}
              <span className="continue-status-text">{status}</span>
            </span>
          </span>
        </span>
      </button>
      {showsDetailsButton && (
        <button
          type="button"
          className="continue-info"
          onClick={handleInfo}
          aria-label={`Details for ${item.title.title}`}
          title="Title details"
        >
          <CardDetailsGlyph />
        </button>
      )}
    </div>
  );
}

export function ContinueWatching({
  profileId,
  filter,
  onSelect,
  onResume,
  progress,
  isPoster = false,
  showsDetailsButton = true,
  showsHeading = true,
}: {
  profileId: string;
  filter: Category;
  onSelect: (title: Title) => void;
  onResume: (item: WatchProgress) => Promise<void>;
  progress?: WatchProgress[];
  isPoster?: boolean;
  showsDetailsButton?: boolean;
  showsHeading?: boolean;
}) {
  const [playing, setPlaying] = useState<string | null>(null);
  const [playError, setPlayError] = useState('');

  async function play(item: WatchProgress) {
    if (playing) return;
    setPlaying(`${item.title.type}-${item.title.id}`);
    setPlayError('');
    try {
      await onResume(item);
    } catch (error) {
      setPlayError(
        error instanceof Error ? error.message : 'Couldn’t resume playback. Try again.',
      );
    } finally {
      setPlaying(null);
    }
  }

  const library = useProfileLibrary(profileId);
  const track = useRef<HTMLDivElement>(null);
  const seen = new Set<string>();
  const items = (progress ?? library.progress).filter((item) => {
    const key = `${item.title.type}:${item.title.id}`;
    if (seen.has(key)) return false;
    seen.add(key);
    return (
      (filter === 'all' || item.title.type === filter) &&
      item.position >= 10 &&
      (!item.completed || item.title.type === 'tv')
    );
  });

  const scroll = (direction: number) =>
    track.current?.scrollBy({
      left: direction * track.current.clientWidth * 0.8,
      behavior: window.matchMedia('(prefers-reduced-motion: reduce)').matches
        ? 'instant'
        : 'smooth',
    });

  if (!items.length && !library.error) return null;

  return (
    <section className="shelf continue-watching" aria-label="Continue Watching">
      {showsHeading && (
        <div className="shelf-heading">
          <h2>Continue Watching</h2>
          {!!items.length && (
            <div className="shelf-controls">
              <button
                type="button"
                aria-label="Previous continue watching titles"
                onClick={() => scroll(-1)}
              >
                <Icon name="left" />
              </button>
              <button
                type="button"
                aria-label="Next continue watching titles"
                onClick={() => scroll(1)}
              >
                <Icon name="right" />
              </button>
            </div>
          )}
        </div>
      )}
      {library.error && !items.length ? (
        <p className="continue-error" role="alert">
          Couldn’t load watch progress.{' '}
          <button
            type="button"
            className="profile-secondary"
            onClick={() => void library.refresh()}
          >
            Try again
          </button>
        </p>
      ) : (
        <div className={`continue-track${isPoster ? ' is-poster-track' : ''}`} ref={track}>
          {items.map((item) => {
            const itemKey = `${item.title.type}-${item.title.id}`;
            return (
              <ContinueWatchingCard
                key={itemKey}
                item={item}
                isPoster={isPoster}
                showsDetailsButton={showsDetailsButton}
                isPreparingPlayback={playing === itemKey}
                isPlaybackDisabled={playing !== null}
                onPlay={() => void play(item)}
                onInfo={() => onSelect(item.title)}
              />
            );
          })}
        </div>
      )}
      {playError && (
        <p className="continue-error" role="alert">
          {playError}
        </p>
      )}
    </section>
  );
}
