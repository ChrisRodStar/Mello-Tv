import { useMemo, useState, type CSSProperties } from 'react';

import type { Profile, Title, WatchProgress } from '../../model';

import { useProfileLibrary } from '../../hooks/useProfileLibrary';
import { Icon } from '../common/Icon';
import { ProfileAvatar } from './ProfileAvatar';

type ProfileTab = 'continue' | 'list' | 'history';

interface HistoryDayGroup {
  id: string;
  date: Date | null;
  items: WatchProgress[];
}

function getProfileFraction(item: WatchProgress): number | null {
  if (
    !Number.isFinite(item.position) ||
    !Number.isFinite(item.duration) ||
    item.duration <= 0
  ) {
    return null;
  }
  return Math.min(1, Math.max(0, item.position / item.duration));
}

function getContinueRemaining(item: WatchProgress, fraction: number | null): string {
  if (item.completed) {
    return item.title.type === 'tv' ? 'Next episode' : 'Play again';
  }
  if (fraction === null) return 'Resume';
  const minutes = Math.ceil((item.duration * (1 - fraction)) / 60);
  if (!Number.isFinite(minutes)) return 'Resume';
  if (minutes < 1) return '<1m';
  return minutes >= 60 ? `${Math.floor(minutes / 60)}h ${minutes % 60}m` : `${minutes}m`;
}

function toTimestampMs(updatedAt: number): number | null {
  if (!Number.isFinite(updatedAt) || updatedAt <= 0) return null;
  const ms = updatedAt < 1e11 ? updatedAt * 1000 : updatedAt;
  if (ms > 253402300799000) return null;
  return ms;
}

function startOfDay(date: Date): Date {
  return new Date(date.getFullYear(), date.getMonth(), date.getDate());
}

function formatHistoryDayTitle(date: Date): string {
  const now = new Date();
  const today = startOfDay(now).getTime();
  const target = startOfDay(date).getTime();
  const diffDays = Math.round((today - target) / 86400000);
  if (diffDays === 0) return 'Today';
  if (diffDays === 1) return 'Yesterday';
  return new Intl.DateTimeFormat(undefined, { weekday: 'long' }).format(date);
}

function formatHistoryMonthYear(date: Date): string {
  return new Intl.DateTimeFormat(undefined, { month: 'long', year: 'numeric' }).format(date);
}

function groupHistoryByDay(progress: WatchProgress[]): HistoryDayGroup[] {
  const sorted = [...progress].sort((a, b) => {
    const left = toTimestampMs(a.updatedAt) ?? 0;
    const right = toTimestampMs(b.updatedAt) ?? 0;
    return right - left;
  });

  const grouped = new Map<number, { date: Date; items: WatchProgress[] }>();
  const undated: WatchProgress[] = [];

  for (const item of sorted) {
    const ms = toTimestampMs(item.updatedAt);
    if (ms === null) {
      undated.push(item);
      continue;
    }
    const dayDate = startOfDay(new Date(ms));
    const key = dayDate.getTime();
    const existing = grouped.get(key);
    if (existing) {
      existing.items.push(item);
    } else {
      grouped.set(key, { date: dayDate, items: [item] });
    }
  }

  const result: HistoryDayGroup[] = Array.from(grouped.entries())
    .sort((a, b) => b[0] - a[0])
    .map(([key, group]) => ({
      id: String(key),
      date: group.date,
      items: group.items,
    }));

  if (undated.length > 0) {
    result.push({
      id: 'undated',
      date: null,
      items: undated,
    });
  }

  return result;
}

function BrandTVWatermark() {
  return (
    <span className="profile-brand-tv-mark" aria-hidden="true">
      <svg viewBox="0 0 24 24" width="11" height="11" fill="currentColor">
        <path d="M18.71 19.5c-.83 1.24-1.71 2.45-3.05 2.47-1.34.03-1.77-.79-3.29-.79-1.53 0-2 .77-3.27.82-1.31.05-2.3-1.32-3.14-2.53C4.25 17 2.94 12.45 4.7 9.39c.87-1.52 2.43-2.48 4.12-2.51 1.28-.02 2.5.87 3.29.87.78 0 2.26-1.07 3.81-.91.65.03 2.47.26 3.64 1.98-.09.06-2.17 1.28-2.15 3.81.03 3.02 2.65 4.03 2.68 4.04-.03.07-.42 1.44-1.38 2.83M15.96 6.33c.67-.81 1.12-1.94 1-3.08-.96.04-2.13.64-2.82 1.45-.61.7-.11 1.85-.99 2.97 1.08.08 2.15-.53 2.81-1.34" />
      </svg>
      <span>TV</span>
    </span>
  );
}

function CardDetailsGlyph() {
  return (
    <span className="profile-card-details-glyph" aria-hidden="true">
      <svg viewBox="0 0 24 24" width="15" height="15" fill="currentColor">
        <circle cx="5" cy="12" r="2" />
        <circle cx="12" cy="12" r="2" />
        <circle cx="19" cy="12" r="2" />
      </svg>
    </span>
  );
}

function ArtworkFallback({ title, type }: { title: string; type: 'movie' | 'tv' }) {
  return (
    <span className="personal-art-fallback" aria-hidden="true">
      <svg
        viewBox="0 0 256 256"
        width="24"
        height="24"
        fill="none"
        stroke="currentColor"
        strokeWidth="16"
        strokeLinecap="round"
        strokeLinejoin="round"
      >
        {type === 'movie' ? (
          <>
            <rect x="32" y="48" width="192" height="160" rx="8" />
            <line x1="32" y1="96" x2="224" y2="96" />
            <line x1="88" y1="48" x2="72" y2="96" />
            <line x1="144" y1="48" x2="128" y2="96" />
            <line x1="200" y1="48" x2="184" y2="96" />
          </>
        ) : (
          <>
            <rect x="32" y="72" width="192" height="136" rx="8" />
            <polyline points="80 24 128 72 176 24" />
          </>
        )}
      </svg>
      <span>{title}</span>
    </span>
  );
}

export function ProfilePage({
  profile,
  onSelect,
  onSwitch,
  onBrowse,
  onResume,
  onPreferences,
}: {
  profile: Profile;
  onSelect: (title: Title) => void;
  onSwitch: () => void;
  onBrowse: () => void;
  onResume: (item: WatchProgress) => Promise<void>;
  onPreferences?: () => void;
}) {
  const library = useProfileLibrary(profile.id);
  const [tab, setTab] = useState<ProfileTab>('continue');
  const [resolvingKey, setResolvingKey] = useState<string | null>(null);

  const continueItems = useMemo(() => {
    const seen = new Set<string>();
    return library.progress.filter((item) => {
      const key = `${item.title.type}:${item.title.id}`;
      if (seen.has(key)) return false;
      seen.add(key);
      return item.position >= 10 && (!item.completed || item.title.type === 'tv');
    });
  }, [library.progress]);

  const historyDays = useMemo(
    () => groupHistoryByDay(library.progress),
    [library.progress],
  );

  async function handlePlay(item: WatchProgress) {
    if (resolvingKey) return;
    const key = `${item.title.type}-${item.title.id}-${item.season}-${item.episode}`;
    setResolvingKey(key);
    try {
      await onResume(item);
    } finally {
      setResolvingKey(null);
    }
  }

  const surfaceStyle = {
    '--profile-color': profile.color,
  } as CSSProperties;

  return (
    <section
      className="personal-profile"
      style={surfaceStyle}
      aria-labelledby="personal-profile-title"
    >
      <div className="personal-profile-ambient" aria-hidden="true" />

      <header className="personal-profile-header">
        <div className="personal-profile-identity-left">
          <h1 id="personal-profile-title">{profile.name}</h1>
          {onPreferences && (
            <button
              type="button"
              className="personal-profile-preferences-btn"
              onClick={onPreferences}
            >
              <span>Viewing preferences</span>
              <svg
                viewBox="0 0 24 24"
                width="11"
                height="11"
                fill="none"
                stroke="currentColor"
                strokeWidth="2.6"
                strokeLinecap="round"
                strokeLinejoin="round"
                aria-hidden="true"
              >
                <path d="M9 5l7 7-7 7" />
              </svg>
            </button>
          )}
        </div>

        <button
          type="button"
          className="personal-profile-switch-artwork"
          onClick={onSwitch}
          aria-label={`Switch profile, current profile ${profile.name}`}
          title="Switch profile"
        >
          <span className="personal-profile-switch-avatar">
            <ProfileAvatar profile={profile} size={80} />
          </span>
          <span className="personal-profile-switch-badge" aria-hidden="true">
            <svg
              viewBox="0 0 24 24"
              width="12"
              height="12"
              fill="none"
              stroke="currentColor"
              strokeWidth="2.1"
              strokeLinecap="round"
              strokeLinejoin="round"
            >
              <path d="M4 7h16m-4-4 4 4-4 4M20 17H4m4-4-4 4 4 4" />
            </svg>
          </span>
        </button>
      </header>

      <div className="personal-profile-picker-row">
        <div className="personal-profile-tabs" role="tablist" aria-label="Library collection">
          {(
            [
              ['continue', 'Continue'],
              ['list', 'My List'],
              ['history', 'History'],
            ] as const
          ).map(([value, label]) => (
            <button
              key={value}
              id={`library-tab-${value}`}
              type="button"
              role="tab"
              aria-selected={tab === value}
              aria-controls="library-panel"
              tabIndex={tab === value ? 0 : -1}
              onKeyDown={(event) => {
                const tabs = ['continue', 'list', 'history'] as const;
                const index = tabs.indexOf(value);
                const next =
                  event.key === 'ArrowRight'
                    ? (index + 1) % tabs.length
                    : event.key === 'ArrowLeft'
                      ? (index + tabs.length - 1) % tabs.length
                      : event.key === 'Home'
                        ? 0
                        : event.key === 'End'
                          ? tabs.length - 1
                          : -1;
                if (next >= 0) {
                  event.preventDefault();
                  setTab(tabs[next]!);
                  document.getElementById(`library-tab-${tabs[next]}`)?.focus();
                }
              }}
              onClick={() => setTab(value)}
            >
              {label}
            </button>
          ))}
        </div>
      </div>

      <div id="library-panel" role="tabpanel" aria-labelledby={`library-tab-${tab}`}>
        {library.error ? (
          <div className="profile-error" role="alert">
            <span>{library.error}</span>
            <button
              type="button"
              className="profile-secondary"
              onClick={() => void library.refresh()}
            >
              Try again
            </button>
          </div>
        ) : !library.ready ? (
          <div
            className="personal-library-loading"
            role="status"
            aria-label="Loading your library"
          >
            <span />
            <span />
            <span />
          </div>
        ) : tab === 'continue' ? (
          continueItems.length === 0 ? (
            <div className="personal-library-empty">
              <h2>Ready when you are</h2>
              <p>Pick a movie or series. You can return to it here.</p>
            </div>
          ) : (
            <div className="personal-poster-shelf">
              {continueItems.map((item) => {
                const key = `${item.title.type}-${item.title.id}-${item.season}-${item.episode}`;
                const fraction = getProfileFraction(item);
                const remaining = getContinueRemaining(item, fraction);
                const posterSrc = item.title.posterUrl ?? item.title.backdropUrl;
                const isPreparing = resolvingKey === key;

                return (
                  <button
                    key={`${item.title.type}-${item.title.id}`}
                    type="button"
                    className="profile-continue-poster-card"
                    disabled={resolvingKey !== null}
                    onClick={() => void handlePlay(item)}
                    aria-label={`${item.completed ? 'Play' : 'Resume'} ${item.title.title}`}
                  >
                    <span className="profile-continue-poster-art">
                      {posterSrc ? (
                        <img src={posterSrc} alt="" loading="lazy" decoding="async" />
                      ) : (
                        <ArtworkFallback title={item.title.title} type={item.title.type} />
                      )}
                      <BrandTVWatermark />
                      <span className="profile-continue-poster-frost" aria-hidden="true" />
                      <span className="profile-continue-poster-shade" aria-hidden="true" />
                      <span className="profile-continue-poster-meta">
                        {item.title.type === 'tv' && (
                          <span className="profile-continue-poster-episode">
                            S{item.season}, E{item.episode}
                          </span>
                        )}
                        <span className="profile-continue-poster-status">
                          <span className="profile-continue-poster-play" aria-hidden="true">
                            {isPreparing ? (
                              <span className="profile-inline-spinner" />
                            ) : (
                              <Icon name="play" />
                            )}
                          </span>
                          {fraction !== null && (
                            <span
                              className="profile-continue-capsule-track"
                              aria-hidden="true"
                            >
                              <span
                                className="profile-continue-capsule-fill"
                                style={{ width: `${Math.round(fraction * 100)}%` }}
                              />
                            </span>
                          )}
                          <span className="profile-continue-remaining">{remaining}</span>
                        </span>
                      </span>
                    </span>
                  </button>
                );
              })}
            </div>
          )
        ) : tab === 'list' ? (
          library.list.length === 0 ? (
            <div className="personal-library-empty">
              <h2>Save something for later</h2>
              <p>Tap the bookmark on any title to add it to My List.</p>
              <button
                type="button"
                className="personal-browse-titles-btn"
                onClick={onBrowse}
              >
                Browse titles
              </button>
            </div>
          ) : (
            <div className="personal-poster-shelf">
              {library.list.map((title) => (
                <button
                  key={`${title.type}-${title.id}`}
                  type="button"
                  className="profile-saved-poster-tile"
                  onClick={() => onSelect(title)}
                  aria-label={`${title.title}, ${title.type === 'movie' ? 'Movie' : 'Series'}${title.year ? `, ${title.year}` : ''}`}
                >
                  <span className="profile-saved-poster-art">
                    {title.posterUrl ? (
                      <img src={title.posterUrl} alt="" loading="lazy" decoding="async" />
                    ) : (
                      <ArtworkFallback title={title.title} type={title.type} />
                    )}
                    <CardDetailsGlyph />
                  </span>
                </button>
              ))}
            </div>
          )
        ) : library.progress.length === 0 ? (
          <div className="personal-library-empty">
            <h2>Your viewing starts here</h2>
            <p>The movies and episodes you watch will appear here.</p>
          </div>
        ) : (
          <div className="profile-history-journal">
            {historyDays.map((day) => (
              <section key={day.id} className="profile-history-day">
                <header className="profile-history-day-header">
                  {day.date ? (
                    <>
                      <span className="profile-history-day-number">
                        {day.date.getDate()}
                      </span>
                      <div className="profile-history-day-labels">
                        <span className="profile-history-day-title">
                          {formatHistoryDayTitle(day.date)}
                        </span>
                        <span className="profile-history-day-month">
                          {formatHistoryMonthYear(day.date)}
                        </span>
                      </div>
                    </>
                  ) : (
                    <span className="profile-history-day-title">Earlier viewing</span>
                  )}
                </header>

                <div className="profile-history-timeline">
                  {day.items.map((item) => {
                    const key = `${item.title.type}-${item.title.id}-${item.season}-${item.episode}`;
                    const fraction = getProfileFraction(item);
                    const thumbSrc = item.title.backdropUrl ?? item.title.posterUrl;
                    const isPreparing = resolvingKey === key;
                    const remainingMinutes =
                      fraction !== null
                        ? Math.max(1, Math.ceil((item.duration * (1 - fraction)) / 60))
                        : null;

                    return (
                      <div key={key} className="profile-history-row">
                        <span className="profile-history-rail" aria-hidden="true" />
                        <span
                          className={
                            item.completed
                              ? 'profile-history-dot is-completed'
                              : 'profile-history-dot'
                          }
                          aria-hidden="true"
                        />

                        <div className="profile-history-row-content">
                          <button
                            type="button"
                            className="profile-history-artwork-btn"
                            disabled={resolvingKey !== null}
                            onClick={() => void handlePlay(item)}
                            aria-label={`${item.completed ? (item.title.type === 'tv' ? 'Continue' : 'Play') : 'Resume'} ${item.title.title}`}
                          >
                            {thumbSrc ? (
                              <img
                                src={thumbSrc}
                                alt=""
                                loading="lazy"
                                decoding="async"
                              />
                            ) : (
                              <ArtworkFallback
                                title={item.title.title}
                                type={item.title.type}
                              />
                            )}
                            <span
                              className="profile-history-artwork-shade"
                              aria-hidden="true"
                            />
                            <span
                              className="profile-history-play-glyph"
                              aria-hidden="true"
                            >
                              {isPreparing ? (
                                <span className="profile-inline-spinner" />
                              ) : (
                                <Icon name="play" />
                              )}
                            </span>
                            {fraction !== null && (
                              <span
                                className="profile-history-progress-track"
                                aria-hidden="true"
                              >
                                <span
                                  className={
                                    item.completed
                                      ? 'profile-history-progress-fill is-completed'
                                      : 'profile-history-progress-fill'
                                  }
                                  style={{ width: `${Math.round(fraction * 100)}%` }}
                                />
                              </span>
                            )}
                          </button>

                          <button
                            type="button"
                            className="profile-history-title-btn"
                            onClick={() => onSelect(item.title)}
                          >
                            <strong className="profile-history-item-title">
                              {item.title.title}
                            </strong>
                            {item.title.type === 'tv' && (
                              <span className="profile-history-item-episode">
                                S{item.season}, E{item.episode}
                                {item.episodeTitle ? ` · ${item.episodeTitle}` : ''}
                              </span>
                            )}
                            <span className="profile-history-item-status">
                              {item.completed ? (
                                <>
                                  <svg
                                    viewBox="0 0 24 24"
                                    width="10"
                                    height="10"
                                    fill="none"
                                    stroke="currentColor"
                                    strokeWidth="3"
                                    strokeLinecap="round"
                                    strokeLinejoin="round"
                                    aria-hidden="true"
                                  >
                                    <path d="M5 13l4 4L19 7" />
                                  </svg>
                                  <span>Watched</span>
                                </>
                              ) : remainingMinutes !== null &&
                                Number.isFinite(remainingMinutes) ? (
                                <span>{remainingMinutes}m remaining</span>
                              ) : (
                                <span>In progress</span>
                              )}
                            </span>
                          </button>
                        </div>
                      </div>
                    );
                  })}
                </div>
              </section>
            ))}

            {library.hasMore && (
              <div className="personal-profile-footer">
                <button
                  type="button"
                  className="profile-secondary"
                  disabled={library.saving}
                  onClick={() => void library.loadMore()}
                >
                  {library.saving ? 'Loading…' : 'Load more history'}
                </button>
              </div>
            )}
          </div>
        )}
      </div>
    </section>
  );
}
