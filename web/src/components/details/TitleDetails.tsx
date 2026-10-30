import { lazy, Suspense, useCallback, useEffect, useRef, useState } from 'react';

import type { Episode, Title } from '../../model';

import { useProfileLibrary } from '../../hooks/useProfileLibrary';
import { useTitleDetails } from '../../hooks/useTitleDetails';
import { loadCatalog } from '../../lib/catalog';
import type { PlaybackRequest, PlaybackResolver } from '../../lib/playback';
import { Icon } from '../common/Icon';
import { EpisodeList } from './EpisodeList';

const loadPlayback = () => import('../playback/PlaybackView');
const PlaybackView = lazy(() => loadPlayback().then((m) => ({ default: m.PlaybackView })));

type DetailTab = 'episodes' | 'about' | 'related';

export function TitleDetails({
  title,
  onClose,
  profileId,
  resolvePlayback,
  initialPlayback,
  onSelect,
}: {
  title: Title;
  onClose: () => void;
  profileId: string;
  resolvePlayback?: PlaybackResolver;
  initialPlayback?: PlaybackRequest;
  onSelect?: (title: Title) => void;
}) {
  const { details, season, setSeason, episodes, metadataError, episodeError, retry } =
    useTitleDetails(title);
  const library = useProfileLibrary(profileId);
  const [selectedTab, setSelectedTab] = useState<DetailTab>('episodes');

  const [suggestions, setSuggestions] = useState<Title[] | null>(null);
  const [loadingSuggestions, setLoadingSuggestions] = useState(false);

  const savedProgress = library.progress.find(
    (item) => item.title.id === title.id && item.title.type === title.type,
  );
  const inMyList = library.list.some(
    (item) => item.id === title.id && item.type === title.type,
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

  // Load suggestions / related titles
  useEffect(() => {
    let active = true;
    const controller = new AbortController();
    async function fetchRelated() {
      setLoadingSuggestions(true);
      try {
        const catalog = await loadCatalog(profileId, title.type, controller.signal);
        if (!active) return;
        const candidates = catalog.titles.filter(
          (t) => t.id !== title.id && Boolean(t.posterUrl),
        );
        setSuggestions(candidates.slice(0, 18));
      } catch {
        if (active && !controller.signal.aborted) {
          setSuggestions([]);
        }
      } finally {
        if (active) setLoadingSuggestions(false);
      }
    }
    void fetchRelated();
    return () => {
      active = false;
      controller.abort();
    };
  }, [profileId, title.id, title.type]);

  const isEpisodeWatched = useCallback(
    (epNum: number) =>
      library.progress.some(
        (p) =>
          p.title.id === title.id &&
          p.title.type === 'tv' &&
          p.season === season &&
          p.episode === epNum &&
          p.completed,
      ),
    [library.progress, title.id, title.type, season],
  );

  const resumeEpisode = (() => {
    if (!episodes || episodes.length === 0) return undefined;
    let targetNumber: number;
    if (savedProgress && savedProgress.season === season) {
      targetNumber = isEpisodeWatched(savedProgress.episode)
        ? savedProgress.episode + 1
        : savedProgress.episode;
    } else {
      targetNumber = episodes[0]?.number ?? 1;
    }
    const candidate = episodes.find((e) => e.number === targetNumber) ?? episodes[0];
    if (!candidate || !isEpisodeWatched(candidate.number)) return candidate;
    return (
      episodes.find((e) => e.number >= candidate.number && !isEpisodeWatched(e.number)) ??
      episodes.find((e) => !isEpisodeWatched(e.number)) ??
      candidate
    );
  })();

  const [playback, setPlayback] = useState<PlaybackRequest | null>(initialPlayback ?? null);
  const playButton = useRef<HTMLButtonElement>(null);

  const nextEpisode =
    playback?.type === 'tv' && playback.season === season
      ? episodes?.find((episode) => episode.number === playback.episode + 1)
      : undefined;

  useEffect(() => {
    void loadPlayback().catch(() => {});
    void import('hls.js').catch(() => {});
  }, []);

  function preloadEpisode(_episode: Episode) {
    void loadPlayback().catch(() => {});
  }

  function play(episode?: Episode, startFromBeginning = false) {
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
      ...(startFromBeginning || progress?.completed ? { startAt: 0 } : {}),
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

  const playButtonTitle = (() => {
    if (savedProgress) {
      if (title.type === 'tv') {
        if (savedProgress.season === season && resumeEpisode) {
          if (
            resumeEpisode.number === savedProgress.episode &&
            !isEpisodeWatched(savedProgress.episode)
          ) {
            return 'Resume';
          }
          return resumeEpisode.number > savedProgress.episode ? 'Next Episode' : 'Play';
        }
        return 'Play';
      }
      if (!savedProgress.completed) return 'Resume';
    }
    return 'Play';
  })();

  const playbackLabel =
    title.type === 'tv' && season !== null
      ? `${playButtonTitle} S${season} E${(resumeEpisode ?? episodes?.[0])?.number ?? 1}`
      : playButtonTitle;

  const headerSubtitle = [title.year, details?.genres[0]].filter(Boolean).join(' · ');

  const seasonCount = details ? details.seasons.filter((s) => s.number > 0).length : 0;
  const metadataLine = [
    title.year,
    title.rating > 0 ? `${title.rating.toFixed(1)} / 10` : '',
    details?.runtime
      ? `${details.runtime} min${title.type === 'tv' ? ' / ep' : ''}`
      : '',
    title.type === 'tv' && details
      ? `${seasonCount} ${seasonCount === 1 ? 'season' : 'seasons'}`
      : '',
  ]
    .filter(Boolean)
    .join('  ·  ');

  const genresLine = details?.genres.length ? details.genres.join('  ·  ') : '';

  const aboutSection = (
    <div className="detail-about-content">
      {title.overview && <p className="detail-overview">{title.overview}</p>}
      {metadataLine && <p className="detail-metadata-line">{metadataLine}</p>}
      {genresLine && <p className="detail-genres">{genresLine}</p>}
    </div>
  );

  const suggestionsSection = (
    <div className="detail-suggestions-section">
      {loadingSuggestions ? (
        <p className="metadata-status" role="status">
          Finding {title.type === 'movie' ? 'movies' : 'TV shows'}…
        </p>
      ) : suggestions && suggestions.length > 0 ? (
        <div className="detail-suggestions-grid">
          {suggestions.map((item) => (
            <button
              key={`${item.type}-${item.id}`}
              type="button"
              className="detail-suggestion-card"
              onClick={() => onSelect?.(item)}
              aria-label={`${item.title}${item.year ? `, ${item.year}` : ''}`}
            >
              <div className="detail-suggestion-poster">
                {item.posterUrl ? (
                  <img src={item.posterUrl} alt="" loading="lazy" decoding="async" />
                ) : (
                  <div className="detail-suggestion-poster-placeholder">
                    <Icon
                      name={item.type === 'movie' ? 'film-slate' : 'television-simple'}
                    />
                  </div>
                )}
              </div>
              <span className="detail-suggestion-title">{item.title}</span>
              {item.year && <span className="detail-suggestion-year">{item.year}</span>}
            </button>
          ))}
        </div>
      ) : (
        <p className="metadata-status">
          More {title.type === 'movie' ? 'movies' : 'TV shows'} will appear as the catalog updates.
        </p>
      )}
    </div>
  );

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
      <div className="dialog-content ios-detail-sheet">
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
            {(title.backdropUrl || title.posterUrl) && (
              <div className="detail-page-surface" aria-hidden="true">
                <img src={title.backdropUrl || title.posterUrl || ''} alt="" decoding="async" />
              </div>
            )}

            <div className="detail-artwork">
              {title.backdropUrl || title.posterUrl ? (
                <img src={title.backdropUrl || title.posterUrl || ''} alt="" decoding="async" />
              ) : null}

              <div className="detail-header-controls">
                <button
                  type="button"
                  className={`detail-glass-circle-button bookmark-button${inMyList ? ' is-saved' : ''}`}
                  disabled={!library.ready || library.saving}
                  aria-label={inMyList ? 'Remove from My List' : 'Add to My List'}
                  aria-pressed={inMyList}
                  onClick={() => void library.toggle(title)}
                >
                  <svg
                    viewBox="0 0 24 24"
                    fill={inMyList ? 'currentColor' : 'none'}
                    stroke="currentColor"
                    strokeWidth="2"
                    strokeLinecap="round"
                    strokeLinejoin="round"
                    aria-hidden="true"
                  >
                    <path d="M6 4a2 2 0 0 1 2-2h8a2 2 0 0 1 2 2v17l-6-4-6 4V4z" />
                  </svg>
                </button>

                <button
                  type="button"
                  className="detail-glass-circle-button ellipsis-button"
                  aria-label="Options"
                >
                  <Icon name="ellipsis" />
                </button>

                <button
                  type="button"
                  className="detail-glass-circle-button close-button"
                  onClick={onClose}
                  aria-label="Close title details"
                  autoFocus
                >
                  <Icon name="close" />
                </button>
              </div>

              <div className="detail-title-overlay">
                <h2 id="detail-title">{title.title}</h2>
                {headerSubtitle && <p className="detail-subtitle">{headerSubtitle}</p>}
              </div>
            </div>

            <div className="detail-body">
              <div className="detail-play-actions">
                <button
                  ref={playButton}
                  type="button"
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
                  <span>{playbackLabel}</span>
                </button>
              </div>

              {library.error && (
                <p className="profile-error" role="alert">
                  {library.error}
                </p>
              )}

              {metadataError ? (
                <div role="alert" className="metadata-status">
                  <span>{metadataError}</span>
                  <button type="button" onClick={retry}>
                    Try again
                  </button>
                </div>
              ) : !details ? (
                <p className="metadata-status" role="status">
                  Loading details…
                </p>
              ) : null}

              {title.type === 'tv' ? (
                <>
                  <div
                    className="detail-tabs-container"
                    role="tablist"
                    aria-label="Title detail sections"
                  >
                    <div className="detail-tabs-capsule">
                      <button
                        type="button"
                        role="tab"
                        aria-selected={selectedTab === 'episodes'}
                        className={`detail-tab-pill${selectedTab === 'episodes' ? ' is-active' : ''}`}
                        onClick={() => setSelectedTab('episodes')}
                      >
                        Episodes
                      </button>
                      <button
                        type="button"
                        role="tab"
                        aria-selected={selectedTab === 'about'}
                        className={`detail-tab-pill${selectedTab === 'about' ? ' is-active' : ''}`}
                        onClick={() => setSelectedTab('about')}
                      >
                        About
                      </button>
                      <button
                        type="button"
                        role="tab"
                        aria-selected={selectedTab === 'related'}
                        className={`detail-tab-pill${selectedTab === 'related' ? ' is-active' : ''}`}
                        onClick={() => setSelectedTab('related')}
                      >
                        Related
                      </button>
                    </div>
                  </div>

                  {selectedTab === 'episodes' ? (
                    details ? (
                      <section className="episode-section" aria-label="Episodes">
                        <div className="episode-heading">
                          {details.seasons.length > 0 && (
                            <div className="season-picker">
                              <select
                                aria-label="Season"
                                value={season ?? ''}
                                disabled={details.seasons.length < 2}
                                onChange={(event) => setSeason(Number(event.target.value))}
                              >
                                {details.seasons.map((s) => (
                                  <option key={s.number} value={s.number}>
                                    {s.name || `Season ${s.number}`}
                                  </option>
                                ))}
                              </select>
                              {details.seasons.length > 1 && <Icon name="down" />}
                            </div>
                          )}
                          {episodes && (
                            <span className="episode-count">
                              {episodes.length}{' '}
                              {episodes.length === 1 ? 'episode' : 'episodes'}
                            </span>
                          )}
                        </div>

                        {episodeError ? (
                          <div className="metadata-status" role="alert">
                            <span>{episodeError}</span>
                            <button type="button" onClick={retry}>
                              Try again
                            </button>
                          </div>
                        ) : season === null ? null : !episodes ? (
                          <p className="metadata-status" role="status">
                            Loading episodes…
                          </p>
                        ) : episodes.length === 0 ? (
                          <p className="metadata-status">
                            No episodes are available for this season.
                          </p>
                        ) : (
                          <EpisodeList
                            key={season}
                            episodes={episodes}
                            resumeEpisodeNumber={resumeEpisode?.number}
                            progress={library.progress.filter(
                              (item) =>
                                item.title.id === title.id &&
                                item.title.type === 'tv' &&
                                item.season === season,
                            )}
                            onPlay={play}
                            onPreload={preloadEpisode}
                            onRewatch={(ep) => play(ep, true)}
                          />
                        )}
                      </section>
                    ) : null
                  ) : selectedTab === 'about' ? (
                    aboutSection
                  ) : (
                    suggestionsSection
                  )}
                </>
              ) : (
                <>
                  {aboutSection}
                  {suggestionsSection}
                </>
              )}
            </div>
          </>
        )}
      </div>
    </dialog>
  );
}
