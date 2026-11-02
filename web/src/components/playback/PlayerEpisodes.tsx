import { useEffect, useRef, useState } from 'react';
import { loadDetails, loadEpisodes } from '../../lib/catalog';
import type { PlaybackRequest } from '../../lib/playback';
import type { Details, Episode, WatchProgress } from '../../model';
import { Icon } from '../common/Icon';
import './player-episodes.css';

type TVRequest = Extract<PlaybackRequest, { type: 'tv' }>;

export function PlayerEpisodes({
  request,
  progress = [],
  onSelect,
  onClose,
}: {
  request: TVRequest;
  progress?: WatchProgress[];
  onSelect: (season: number, episode: Episode) => void;
  onClose: () => void;
}) {
  const [details, setDetails] = useState<Details | null>(null);
  const [season, setSeason] = useState(request.season);
  const [episodes, setEpisodes] = useState<Episode[] | null>(null);
  const [detailsError, setDetailsError] = useState(false);
  const [episodesError, setEpisodesError] = useState(false);
  const list = useRef<HTMLDivElement>(null);
  const [attempt, setAttempt] = useState(0);

  useEffect(() => {
    const controller = new AbortController();
    const title = request.catalogTitle ?? {
      id: request.titleId,
      type: 'tv' as const,
      title: request.title,
      overview: '',
      year: '',
      rating: 0,
      posterUrl: null,
    };
    setDetailsError(false);
    void loadDetails(title, AbortSignal.any([controller.signal, AbortSignal.timeout(10000)]))
      .then((result) => {
        if (!controller.signal.aborted) setDetails(result);
      })
      .catch(() => {
        if (!controller.signal.aborted) setDetailsError(true);
      });
    return () => controller.abort();
  }, [request, attempt]);

  useEffect(() => {
    const controller = new AbortController();
    setEpisodes(null);
    setEpisodesError(false);
    void loadEpisodes(
      request.titleId,
      season,
      AbortSignal.any([controller.signal, AbortSignal.timeout(10000)]),
    )
      .then((result) => {
        if (!controller.signal.aborted) setEpisodes(result);
      })
      .catch(() => {
        if (!controller.signal.aborted) setEpisodesError(true);
      });
    return () => controller.abort();
  }, [request.titleId, season, attempt]);

  useEffect(() => {
    if (episodes)
      list.current
        ?.querySelector('.player-episode.is-playing')
        ?.scrollIntoView({ block: 'nearest', inline: 'center' });
  }, [episodes]);

  return (
    <section
      className="player-episodes"
      role="dialog"
      aria-modal="true"
      aria-label="Episodes"
      onClick={(event) => event.stopPropagation()}
      onKeyDown={(event) => {
        if (event.key === 'Escape') {
          event.preventDefault();
          event.stopPropagation();
          onClose();
        }
      }}
    >
      <header className="player-episodes-header">
        <label className="player-episodes-season">
          <span className="player-episodes-visually-hidden">Season</span>
          <select
            value={season}
            onChange={(event) => setSeason(Number(event.target.value))}
            disabled={!details?.seasons.length}
          >
            {!details?.seasons.some((item) => item.number === season) && (
              <option value={season}>Season {season}</option>
            )}
            {details?.seasons.map((item) => (
              <option key={item.number} value={item.number}>
                {item.name || `Season ${item.number}`}
              </option>
            ))}
          </select>
          <Icon name="down" />
        </label>
        <div className="player-episodes-heading">
          <h2>{request.title}</h2>
          <button type="button" onClick={onClose} aria-label="Close episodes" autoFocus>
            <Icon name="close" />
          </button>
        </div>
      </header>
      {(detailsError || episodesError) && (
        <div className="player-episodes-status" role="alert">
          Couldn’t load {episodesError ? 'episodes' : 'seasons'}.{' '}
          <button type="button" onClick={() => setAttempt((value) => value + 1)}>
            Try again
          </button>
        </div>
      )}
      {!episodesError && !episodes && (
        <p className="player-episodes-status" role="status">
          Loading episodes…
        </p>
      )}
      {episodes && episodes.length === 0 && (
        <p className="player-episodes-status">No episodes are available for this season.</p>
      )}
      {episodes && (
        <div ref={list} className="player-episodes-list">
          {episodes.map((episode) => {
            const playing = season === request.season && episode.number === request.episode;
            const saved = progress.find(
              (item) =>
                item.title.id === request.titleId &&
                item.title.type === 'tv' &&
                item.season === season &&
                item.episode === episode.number,
            );
            const fraction = saved?.duration
              ? Math.min(1, Math.max(0, saved.position / saved.duration))
              : 0;
            return (
              <button
                type="button"
                key={episode.number}
                className={`player-episode${playing ? ' is-playing' : ''}`}
                onClick={() => onSelect(season, episode)}
                aria-current={playing ? 'true' : undefined}
              >
                <span className="player-episode-image">
                  {episode.stillUrl ? (
                    <img src={episode.stillUrl} alt="" loading="lazy" decoding="async" />
                  ) : (
                    <span className="player-episode-placeholder">
                      <Icon name="television-simple" />
                    </span>
                  )}
                  {playing && <span className="player-episode-badge">Playing</span>}
                  {fraction > 0 && (
                    <span className="player-episode-progress">
                      <span style={{ width: `${fraction * 100}%` }} />
                    </span>
                  )}
                </span>
                <span className="player-episode-title">
                  {episode.number}. {episode.name || `Episode ${episode.number}`}
                </span>
                {episode.runtime && (
                  <span className="player-episode-runtime">{episode.runtime} min</span>
                )}
                {episode.overview && (
                  <span className="player-episode-overview">{episode.overview}</span>
                )}
              </button>
            );
          })}
        </div>
      )}
    </section>
  );
}
