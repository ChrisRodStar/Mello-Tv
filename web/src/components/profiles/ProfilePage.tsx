import { useState } from 'react';

import type { Profile, Title, WatchProgress } from '../../model';

import { useProfileLibrary } from '../../hooks/useProfileLibrary';
import { ContinueWatching } from '../catalog/ContinueWatching';
import { Icon } from '../common/Icon';
import { ProfileAvatar } from './ProfileAvatar';

export function ProfilePage({
  profile,
  onSelect,
  onSwitch,
  onBrowse,
  onResume,
}: {
  profile: Profile;
  onSelect: (title: Title) => void;
  onSwitch: () => void;
  onBrowse: () => void;
  onResume: (item: WatchProgress) => Promise<void>;
}) {
  const library = useProfileLibrary(profile.id);
  const [tab, setTab] = useState<'continue' | 'list' | 'history'>('continue');
  const latest = library.progress.filter(
    (item, index, rows) =>
      rows.findIndex(
        (row) => row.title.id === item.title.id && row.title.type === item.title.type,
      ) === index,
  );
  const progress =
    tab === 'history'
      ? library.progress
      : latest.filter(
          (item) => (item.title.type === 'tv' || !item.completed) && item.position >= 10,
        );
  const artwork =
    latest.find((item) => item.title.backdropUrl)?.title.backdropUrl ??
    library.list.find((title) => title.backdropUrl)?.backdropUrl;
  return (
    <section className="personal-profile" aria-labelledby="personal-profile-title">
      <header className="personal-profile-header">
        {artwork && <img className="personal-profile-backdrop" src={artwork} alt="" />}
        <div className="personal-profile-identity">
          <div className="personal-profile-avatar-control">
            <ProfileAvatar profile={profile} />
            <button
              className="personal-profile-switch"
              onClick={onSwitch}
              aria-label="Switch profile"
              title="Switch profile"
            >
              <Icon name="switch" />
            </button>
          </div>
          <h1 id="personal-profile-title">{profile.name}</h1>

        </div>
      </header>
      <div className="personal-profile-tabs" role="tablist" aria-label="Your library">
        {(
          [
            ['continue', 'Continue watching'],
            ['list', 'My List'],
            ['history', 'Watch history'],
          ] as const
        ).map(([value, label]) => (
          <button
            key={value}
            id={`library-tab-${value}`}
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
      <div id="library-panel" role="tabpanel" aria-labelledby={`library-tab-${tab}`}>
        {library.error ? (
          <div className="profile-error" role="alert">
            {library.error}
            <button className="profile-secondary" onClick={() => void library.refresh()}>
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
        ) : tab === 'continue' && progress.length ? (
          <ContinueWatching
            profileId={profile.id}
            filter="all"
            progress={library.progress}
            onSelect={onSelect}
            onResume={onResume}
          />
        ) : (
          <div
            className={
              tab === 'list'
                ? 'personal-profile-grid personal-poster-grid'
                : 'personal-profile-grid personal-watch-grid'
            }
          >
            {tab === 'list'
              ? library.list.map((title) => (
                  <button
                    key={`${title.type}-${title.id}`}
                    className="personal-library-card"
                    onClick={() => onSelect(title)}
                  >
                    <span className="personal-library-art">
                      {title.posterUrl ? (
                        <img src={title.posterUrl} alt="" loading="lazy" decoding="async" />
                      ) : (
                        <span className="personal-art-fallback">{title.title}</span>
                      )}
                      <span className="personal-library-play" aria-hidden="true">
                        <Icon name="play" />
                      </span>
                    </span>
                    <strong>{title.title}</strong>
                  </button>
                ))
              : progress.map((item) => (
                  <button
                    key={`${item.title.type}-${item.title.id}-${item.season}-${item.episode}`}
                    className="personal-library-card"
                    onClick={() => onSelect(item.title)}
                  >
                    <span className="personal-library-art">
                      {item.title.backdropUrl || item.title.posterUrl ? (
                        <img
                          src={item.title.backdropUrl ?? item.title.posterUrl!}
                          alt=""
                          loading="lazy"
                          decoding="async"
                        />
                      ) : (
                        <span className="personal-art-fallback">{item.title.title}</span>
                      )}
                      <span className="personal-library-play" aria-hidden="true">
                        <Icon name="play" />
                      </span>
                    </span>
                    <strong>{item.title.title}</strong>
                    {item.title.type === 'tv' && (
                      <span>
                        Season {item.season} · Episode {item.episode}
                      </span>
                    )}
                    <span>
                      {item.watchedSeconds < 60
                        ? 'Less than a minute watched'
                        : `${Math.floor(item.watchedSeconds / 60)} min watched`}
                      {item.completed ? ' · Finished' : ''}
                    </span>
                    {item.duration > 0 && (
                      <progress
                        value={item.position}
                        max={item.duration}
                        aria-label={`Progress in ${item.title.title}`}
                      />
                    )}
                  </button>
                ))}
            {(tab === 'list' ? !library.list.length : !progress.length) && (
              <div className="personal-library-empty">
                <h2>
                  {tab === 'list'
                    ? 'Nothing saved yet'
                    : tab === 'history'
                      ? 'No watch history yet'
                      : 'Nothing to resume'}
                </h2>
                <button className="profile-primary" onClick={onBrowse}>
                  Browse titles
                </button>
              </div>
            )}
          </div>
        )}
        {tab === 'history' && library.hasMore && (
          <button
            className="profile-secondary"
            disabled={library.saving}
            onClick={() => void library.loadMore()}
          >
            Load more history
          </button>
        )}
      </div>
    </section>
  );
}
