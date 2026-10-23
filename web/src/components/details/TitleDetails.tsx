import { lazy, Suspense, useEffect, useRef, useState } from 'react';

import type { Episode, Title } from '../../model';

import { useProfileLibrary } from '../../hooks/useProfileLibrary';
import { useTitleDetails } from '../../hooks/useTitleDetails';
import type { PlaybackRequest, PlaybackResolver } from '../../lib/playback';
import { Icon } from '../common/Icon';
import { EpisodeList } from './EpisodeList';

const loadPlayback = () => import('../playback/PlaybackView');
const PlaybackView = lazy(() => loadPlayback().then((m) => ({ default: m.PlaybackView })));

export function TitleDetails({
  title,
  onClose,
  profileId,
  resolvePlayback,
  initialPlayback,
}: {
  title: Title;
  onClose: () => void;
  profileId: string;
  resolvePlayback?: PlaybackResolver;
  initialPlayback?: PlaybackRequest;
}) {
  const { details, season, setSeason, episodes, metadataError, episodeError, retry } =
    useTitleDetails(title);
  const library = useProfileLibrary(profileId);
  const savedProgress = library.progress.find(
    (item) => item.title.id === title.id && item.title.type === title.type,
  );
  const restoredSeason = useRef(false);
  useEffect(() => {
    if (!restoredSeason.current && library.ready && details) {
      restoredSeason.current = true;
      if (initialPlayback?.type === 'tv') {
        setSeason(initialPlayback.season);
        return;
      }
      if (
        savedProgress &&
        title.type === 'tv' &&
        details.seasons.some((item) => item.number === savedProgress.season)
      ) {
        const savedSeason = details.seasons.find(
          (item) => item.number === savedProgress.season,
        );
        const nextSeason =
          savedProgress.completed &&
          savedSeason &&
          savedProgress.episode >= savedSeason.episodeCount
            ? details.seasons.find((item) => item.number > savedProgress.season)
            : undefined;
        setSeason(nextSeason?.number ?? savedProgress.season);
      }
    }
  }, [library.ready, details, savedProgress, title.type, setSeason, initialPlayback]);
  const resumeEpisode =
    savedProgress && savedProgress.season === season
      ? episodes?.find(
          (item) => item.number === savedProgress.episode + (savedProgress.completed ? 1 : 0),
        )
      : undefined;
  const [playback, setPlayback] = useState<PlaybackRequest | null>(initialPlayback ?? null);
  const playButton = useRef<HTMLButtonElement>(null);

  const nextEpisode =
    playback?.type === 'tv' && playback.season === season
      ? episodes?.find((episode) => episode.number === playback.episode + 1)
      : undefined;

  useEffect(() => {
    // Warm the player code as well as the resolver before the Play gesture.
    void loadPlayback().catch(() => {});
    void import('hls.js').catch(() => {});
  }, []);

  function preloadEpisode(_episode: Episode) {
    // Only preload player code; source/session creation belongs to the Play action.
    void loadPlayback().catch(() => {});
  }

  function play(episode?: Episode) {
    const progress = library.progress.find(
      (p) =>
        p.title.id === title.id &&
        p.title.type === title.type &&
        (title.type === 'movie' || (p.season === season && p.episode === episode?.number)),
    );
    const base = {
      titleId: title.id,
      title: title.title,
      profileId,
      catalogTitle: title,
      ...(progress?.completed ? { startAt: 0 } : {}),
    };
    if (title.type === 'movie') setPlayback({ ...base, type: 'movie' });
    else if (episode && season !== null)
      setPlayback({
        ...base,
        type: 'tv',
        season,
        episode: episode.number,
        episodeTitle: episode.name,
      });
  }

  const dialog = useRef<HTMLDialogElement>(null);
  useEffect(() => {
    const element = dialog.current;
    element?.showModal();
    return () => {
      element?.close();
    };
  }, []);

  return (
    <dialog
      ref={dialog}
      className={playback ? 'detail-dialog playback-dialog' : 'detail-dialog'}
      onCancel={(event) => {
        if (playback) {
          event.preventDefault();
          if (initialPlayback) {
            onClose();
            return;
          }
          setPlayback(null);
          requestAnimationFrame(() => playButton.current?.focus());
        } else onClose();
      }}
      onClick={(event) => {
        if (event.target === event.currentTarget) onClose();
      }}
      aria-labelledby="detail-title"
    >
      <div className="dialog-content netflix-detail">
        {!playback && (
          <button
            className="close-button"
            onClick={onClose}
            aria-label="Close title details"
            autoFocus
          >
            <Icon name="close" />
          </button>
        )}
        {playback ? (
          <Suspense
            fallback={
              <div className="playback-view player-buffering">
                <span />
              </div>
            }
          >
            <PlaybackView
              request={playback}
              resolver={resolvePlayback}
              onNext={nextEpisode ? () => play(nextEpisode) : undefined}
              onBack={() => {
                if (initialPlayback) {
                  onClose();
                  return;
                }
                setPlayback(null);
                requestAnimationFrame(() => playButton.current?.focus());
              }}
            />
          </Suspense>
        ) : (
          <>
            <div className="detail-artwork">
              {title.backdropUrl || title.posterUrl ? (
                <img src={title.backdropUrl || title.posterUrl || ''} alt="" decoding="async" />
              ) : null}
              <div className="detail-title-overlay">
                <span className="detail-type">
                  {title.type === 'movie' ? 'Movie' : 'TV Series'}
                </span>
                <h2 id="detail-title">{title.title}</h2>
              </div>
            </div>
            <div className="detail-body">
              <div className="detail-play-actions">
                <button
                  ref={playButton}
                  className="detail-play-button"
                  disabled={title.type === 'tv' && (!episodes?.length || !!episodeError)}
                  onClick={() => play(resumeEpisode ?? episodes?.[0])}
                  aria-label={
                    title.type === 'movie'
                      ? `Play ${title.title}`
                      : `Play ${resumeEpisode ? 'saved episode' : 'first episode of selected season'}`
                  }
                >
                  <Icon name="play" />
                  {savedProgress &&
                  !savedProgress.completed &&
                  (title.type === 'movie' || resumeEpisode)
                    ? 'Resume'
                    : resumeEpisode && savedProgress?.completed
                      ? 'Next episode'
                      : 'Play'}
                </button>
                <button
                  className="profile-secondary"
                  disabled={!library.ready || library.saving}
                  aria-pressed={library.list.some(
                    (item) => item.id === title.id && item.type === title.type,
                  )}
                  onClick={() => void library.toggle(title)}
                >
                  {library.list.some((item) => item.id === title.id && item.type === title.type)
                    ? 'In My List'
                    : 'Add to My List'}
                </button>
              </div>
              {library.error && (
                <p className="profile-error" role="alert">
                  {library.error}
                </p>
              )}
              <div className="detail-summary">
                <div className="detail-meta">
                  {title.year && <span>{title.year}</span>}
                  {title.rating > 0 && (
                    <span className="detail-score">{title.rating.toFixed(1)} / 10</span>
                  )}
                  {details?.runtime ? (
                    <span>
                      {details.runtime} min
                      {title.type === 'tv' ? ' / episode' : ''}
                    </span>
                  ) : null}
                  {details && title.type === 'tv' && (
                    <span>
                      {details.seasons.filter((s) => s.number > 0).length}{' '}
                      {details.seasons.filter((s) => s.number > 0).length === 1
                        ? 'season'
                        : 'seasons'}
                    </span>
                  )}
                  <span className="detail-format">
                    {title.type === 'movie' ? 'Movie' : 'Series'}
                  </span>
                </div>
                {title.overview && <p>{title.overview}</p>}
              </div>
              {metadataError ? (
                <div role="alert" className="metadata-status">
                  {metadataError} <button onClick={retry}>Try again</button>
                </div>
              ) : !details ? (
                <p className="metadata-status" role="status">
                  Loading details…
                </p>
              ) : (
                <p className="detail-genres">{details.genres.join(' · ')}</p>
              )}
              {title.type === 'tv' && details && (
                <section className="episode-section" aria-label="Episodes">
                  <div className="episode-heading">
                    <h3>Episodes</h3>
                    {details.seasons.length > 0 && (
                      <div className="season-picker">
                        <select
                          aria-label="Season"
                          value={season ?? ''}
                          onChange={(event) => setSeason(Number(event.target.value))}
                        >
                          {details.seasons.map((s) => (
                            <option key={s.number} value={s.number}>
                              {s.name}
                            </option>
                          ))}
                        </select>
                        <Icon name="down" />
                      </div>
                    )}
                  </div>
                  {episodeError ? (
                    <div className="metadata-status" role="alert">
                      {episodeError} <button onClick={retry}>Try again</button>
                    </div>
                  ) : season === null ? null : !episodes ? (
                    <p role="status">Loading episodes…</p>
                  ) : episodes.length === 0 ? null : (
                    <EpisodeList
                      key={season}
                      episodes={episodes}
                      progress={library.progress.filter(
                        (item) =>
                          item.title.id === title.id &&
                          item.title.type === 'tv' &&
                          item.season === season,
                      )}
                      onPlay={play}
                      onPreload={preloadEpisode}
                    />
                  )}
                </section>
              )}
            </div>
          </>
        )}
      </div>
    </dialog>
  );
}
