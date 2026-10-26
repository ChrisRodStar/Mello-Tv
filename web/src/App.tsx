import { useCallback, useEffect, useMemo, useRef, useState } from 'react';

import type { Title, WatchProgress } from './model';


import { CatalogContent } from './components/catalog/CatalogContent';
import { TitleDetails } from './components/details/TitleDetails';
import { Navigation } from './components/navigation/Navigation';
import { ProfilePage } from './components/profiles/ProfilePage';
import { ProfilePreferences } from './components/profiles/ProfilePreferences';
import { ProfileSelector } from './components/profiles/ProfileSelector';
import { SearchBar } from './components/search/SearchBar';
import { SearchResults } from './components/search/SearchResults';
import { useCatalog } from './hooks/useCatalog';
import { useProfileActivity } from './hooks/useProfileActivity';
import { useProfiles } from './hooks/useProfiles';
import { readNavigationSession, saveNavigationSession } from './lib/navigation-session';
import type { PlaybackRequest, PlaybackResolver } from './lib/playback';
import { resumePlayback } from './lib/resume-playback';
import type { Category } from './types';

export function App({ resolvePlayback }: { resolvePlayback?: PlaybackResolver } = {}) {
  const [initialNavigation] = useState(readNavigationSession);
  const [filter, setFilter] = useState<Category>(initialNavigation.filter);
  const profiles = useProfiles();
  const [profilesOpen, setProfilesOpen] = useState(initialNavigation.profilesOpen);
  const [switchingProfile, setSwitchingProfile] = useState(false);
  const [editingPreferences, setEditingPreferences] = useState(false);
  const [selected, setSelected] = useState<Title | null>(initialNavigation.selected);
  const [initialPlayback, setInitialPlayback] = useState<PlaybackRequest | undefined>();
  const handleSelect = useCallback((title: Title) => {
    resumeGeneration.current++;
    setInitialPlayback(undefined);
    setSelected(title);
  }, []);
  const resumeGeneration = useRef(0);
  const handleResume = useCallback(
    async (item: WatchProgress) => {
      const generation = ++resumeGeneration.current;
      const request = await resumePlayback(item, profiles.activeId);
      if (generation !== resumeGeneration.current) return;
      setInitialPlayback(request);
      setSelected(item.title);
    },
    [profiles.activeId],
  );
  const [query, setQuery] = useState(initialNavigation.query);
  const [searchOpen, setSearchOpen] = useState(initialNavigation.searchOpen);
  const activeProfile = profiles.profiles.find((profile) => profile.id === profiles.activeId);
  useProfileActivity(activeProfile?.preferences.completed ? activeProfile.id : undefined);
  const onboarding = !!activeProfile && !activeProfile.preferences.completed;
  const showProfiles = switchingProfile || !profiles.activeId || onboarding;
  const showProfilePage = profilesOpen && !showProfiles && !!activeProfile;
  const { catalog, error, health, checking, retry } = useCatalog(filter, activeProfile);

  useEffect(() => {
    saveNavigationSession({ filter, profilesOpen, selected, query, searchOpen });
  }, [filter, profilesOpen, selected, query, searchOpen]);

  const titles = useMemo(
    () => catalog?.titles.filter((title) => filter === 'all' || title.type === filter) ?? [],
    [catalog?.titles, filter],
  );

  const handleProfiles = useCallback(() => {
    setProfilesOpen(true);
    setSwitchingProfile(false);
    setEditingPreferences(false);
    setSearchOpen(false);
    resumeGeneration.current++;
    setInitialPlayback(undefined);
    setSelected(null);
  }, []);

  const handleCategoryChange = useCallback((category: Category) => {
    setProfilesOpen(false);
    setSwitchingProfile(false);
    setEditingPreferences(false);
    setFilter(category);
    resumeGeneration.current++;
    setInitialPlayback(undefined);
    setSelected(null);
    setSearchOpen(false);
    setQuery('');
  }, []);

  const handleSearch = useCallback(() => {
    setProfilesOpen(false);
    setSwitchingProfile(false);
    setEditingPreferences(false);
    setSearchOpen(true);
    resumeGeneration.current++;
    setInitialPlayback(undefined);
    setSelected(null);
    setFilter('all');
  }, []);

  const handleProfileSelected = useCallback(() => {
    resumeGeneration.current++;
    setInitialPlayback(undefined);
    setSelected(null);
    setProfilesOpen(false);
    setSwitchingProfile(false);
    setEditingPreferences(false);
    setSearchOpen(false);
    setFilter('all');
    setQuery('');
  }, []);

  const handleCloseDetails = useCallback(() => {
    resumeGeneration.current++;
    setInitialPlayback(undefined);
    setSelected(null);
  }, []);


  return (
    <div
      className={
        error
          ? 'app-shell service-shell'
          : !activeProfile || onboarding
            ? 'app-shell profile-setup-shell'
            : 'app-shell'
      }
    >
      {!error && activeProfile && !onboarding && (
        <>
          <a className="skip-link" href="#catalog">
            Skip to catalog
          </a>
          <Navigation
            profile={activeProfile}
            filter={filter}
            searchOpen={searchOpen}
            profilesOpen={showProfiles || showProfilePage}
            onProfiles={handleProfiles}
            onCategoryChange={handleCategoryChange}
            onSearch={handleSearch}
          />
        </>
      )}
      <main
        id="catalog"
        className={
          error
            ? 'service-view'
            : showProfiles || (showProfilePage && editingPreferences)
              ? 'profiles-view'
              : searchOpen
                ? 'search-view'
                : undefined
        }
      >
        {searchOpen && !showProfiles && !error && (
          <SearchBar query={query} onQueryChange={setQuery} />
        )}

        {showProfiles && !error ? (
          onboarding && activeProfile ? (
            <ProfilePreferences
              key={activeProfile.id}
              profile={activeProfile}
              saving={profiles.saving}
              error={profiles.error}
              onSave={async (preferences) => {
                const saved = await profiles.upsert({ ...activeProfile, preferences });
                if (saved) handleProfileSelected();
                return saved;
              }}
            />
          ) : (
            <ProfileSelector profiles={profiles} onSelected={handleProfileSelected} />
          )
        ) : showProfilePage && activeProfile && !error ? (
          editingPreferences ? (
            <ProfilePreferences
              key={activeProfile.id}
              profile={activeProfile}
              saving={profiles.saving}
              error={profiles.error}
              onCancel={() => {
                setEditingPreferences(false);
                profiles.clearError();
              }}
              onSave={async (preferences) => {
                const saved = await profiles.upsert({ ...activeProfile, preferences });
                if (saved) {
                  setEditingPreferences(false);
                  profiles.clearError();
                }
                return saved;
              }}
            />
          ) : (
            <ProfilePage
              key={activeProfile.id}
              profile={activeProfile}
              onSelect={handleSelect}
              onResume={handleResume}
              onBrowse={() => handleCategoryChange('all')}
              onPreferences={() => {
                profiles.clearError();
                setEditingPreferences(true);
              }}
              onSwitch={() => {
                resumeGeneration.current++;
                setInitialPlayback(undefined);
                setSelected(null);
                setEditingPreferences(false);
                setSwitchingProfile(true);
              }}
            />
          )
        ) : searchOpen && !error ? (
          <SearchResults query={query.trim()} onSelect={handleSelect} />
        ) : (
          <CatalogContent
            catalog={catalog}
            profileId={activeProfile?.id}
            onResume={handleResume}
            error={error}
            health={health}
            checking={checking}
            filter={filter}
            titles={titles}
            retry={retry}
            onSelect={handleSelect}
          />
        )}
      </main>
      {selected && !error && activeProfile && !showProfiles && (
        <TitleDetails
          key={`${selected.type}-${selected.id}`}
          title={selected}
          profileId={profiles.activeId}
          resolvePlayback={resolvePlayback}
          initialPlayback={initialPlayback}
          onClose={handleCloseDetails}
        />
      )}
    </div>
  );
}
