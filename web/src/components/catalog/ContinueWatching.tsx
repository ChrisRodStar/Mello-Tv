import { useRef, useState } from 'react';

import type { Title, WatchProgress } from '../../model';

import { useProfileLibrary } from '../../hooks/useProfileLibrary';
import type { Category } from '../../types';
import { Icon } from '../common/Icon';

const timestamp = (seconds: number) =>
  `${Math.floor(seconds / 60)}:${Math.floor(seconds % 60)
    .toString()
    .padStart(2, '0')}`;

export function ContinueWatching({
  profileId,
  filter,
  onSelect,
  onResume,
  progress,
}: {
  profileId: string;
  filter: Category;
  onSelect: (title: Title) => void;
  onResume: (item: WatchProgress) => Promise<void>;
  progress?: WatchProgress[];
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
    <section className="shelf continue-watching" aria-label="Continue watching">
      <div className="shelf-heading">
        <h2>Continue watching</h2>
        {!!items.length && (
          <div className="shelf-controls">
            <button aria-label="Previous continue watching titles" onClick={() => scroll(-1)}>
              <Icon name="left" />
            </button>
            <button aria-label="Next continue watching titles" onClick={() => scroll(1)}>
              <Icon name="right" />
            </button>
          </div>
        )}
      </div>
      {library.error && !items.length ? (
        <p className="continue-error" role="alert">
          Couldn’t load watch progress.{' '}
          <button className="profile-secondary" onClick={() => void library.refresh()}>
            Try again
          </button>
        </p>
      ) : (
        <div className="continue-track" ref={track}>
          {items.map((item) => {
            const remaining = Math.max(0, item.duration - item.position);
            const percent =
              item.duration > 0
                ? Math.min(100, Math.round((item.position / item.duration) * 100))
                : null;
            return (
              <div key={`${item.title.type}-${item.title.id}`} className="continue-card-group">
                <button
                  className="continue-card"
                  disabled={!!playing}
                  onClick={() => void play(item)}
                  aria-label={`${item.completed ? 'Continue' : 'Resume'} ${item.title.title}${item.title.type === 'tv' ? `, season ${item.season}, episode ${item.episode}` : ''}${percent !== null ? `, ${percent}% played` : ''}`}
                >
                  <span className="continue-art">
                    {item.title.backdropUrl || item.title.posterUrl ? (
                      <img
                        src={item.title.backdropUrl ?? item.title.posterUrl!}
                        alt=""
                        loading="lazy"
                        decoding="async"
                      />
                    ) : (
                      <span>{item.title.title}</span>
                    )}
                    <span className="continue-play" aria-hidden="true">
                      <Icon name="play" />
                    </span>
                    {percent !== null && (
                      <span className="continue-percentage">{percent}%</span>
                    )}
                  </span>
                  {item.duration > 0 && (
                    <progress
                      value={item.position}
                      max={item.duration}
                      aria-label={`Episode progress for ${item.title.title}`}
                    />
                  )}
                  <strong>{item.title.title}</strong>
                  {item.title.type === 'tv' && (
                    <span className="continue-episode">
                      S{item.season} · E{item.episode}
                      {item.episodeTitle ? ` · ${item.episodeTitle}` : ''}
                    </span>
                  )}
                  <span className="continue-time">
                    {item.completed
                      ? 'Episode finished'
                      : item.duration > 0
                        ? `${timestamp(item.position)} / ${timestamp(item.duration)} · ${Math.ceil(remaining / 60)} min left`
                        : `${timestamp(item.position)} played`}
                  </span>
                </button>
                <button
                  className="continue-info"
                  onClick={() => onSelect(item.title)}
                  aria-label={`Details for ${item.title.title}`}
                  title="Title details"
                >
                  <Icon name="info" />
                </button>
                {playing === `${item.title.type}-${item.title.id}` && (
                  <span className="continue-loading" role="status">
                    Loading…
                  </span>
                )}
              </div>
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
