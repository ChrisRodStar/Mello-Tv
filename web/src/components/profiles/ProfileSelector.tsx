import { useState } from 'react';

import { emptyPreferences } from '../../model';

import type { useProfiles } from '../../hooks/useProfiles';
import { createProfileId } from '../../lib/profile-id';
import { profileColors, type Profile } from '../../lib/profiles';
import { Icon } from '../common/Icon';
import { ProfileAvatar } from './ProfileAvatar';
import { ProfileForm } from './ProfileForm';
import { ProfilePreferences } from './ProfilePreferences';

export function ProfileSelector({
  profiles,
  onSelected,
}: {
  profiles: ReturnType<typeof useProfiles>;
  onSelected: () => void;
}) {
  const [preferencesFor, setPreferencesFor] = useState<Profile | null>(null);
  const [managing, setManaging] = useState(false);
  const [editing, setEditing] = useState<Profile | null>(null);
  const [firstProfile] = useState<Profile>(() => ({
    id: createProfileId(),
    name: '',
    color: profileColors[0],
    preferences: emptyPreferences,
  }));

  function finishEdit() {
    setEditing(null);
    profiles.clearError();
  }

  if (!profiles.ready)
    return (
      <section className="profile-selector" aria-live="polite">
        {profiles.loading ? (
          <p role="status">Loading profiles…</p>
        ) : (
          <>
            <h1>Profiles unavailable</h1>
            <button className="profile-primary" onClick={() => void profiles.refresh()}>
              Try again
            </button>
          </>
        )}
      </section>
    );

  if (!profiles.profiles.length)
    return (
      <ProfileForm
        profile={firstProfile}
        canDelete={false}
        error={profiles.error}
        saving={profiles.saving}
        onSave={async (profile) => {
          if (await profiles.upsert(profile)) onSelected();
        }}
        onDelete={() => {}}
        onCancel={onSelected}
        canCancel={false}
      />
    );

  if (preferencesFor)
    return (
      <ProfilePreferences
        key={preferencesFor.id}
        profile={preferencesFor}
        saving={profiles.saving}
        error={profiles.error}
        onCancel={() => {
          setPreferencesFor(null);
          profiles.clearError();
        }}
        onSave={async (preferences) => {
          const saved = await profiles.upsert({ ...preferencesFor, preferences });
          if (saved) {
            setPreferencesFor(null);
            setEditing(null);
          }
          return saved;
        }}
      />
    );

  if (editing)
    return (
      <ProfileForm
        key={editing.id}
        profile={editing}
        canDelete={
          profiles.profiles.length > 1 &&
          profiles.profiles.some((item) => item.id === editing.id)
        }
        error={profiles.error}
        saving={profiles.saving}
        onSave={async (profile) => {
          const isNew = !profiles.profiles.some((item) => item.id === profile.id);
          if (await profiles.upsert(profile)) {
            finishEdit();
            if (isNew) onSelected();
          }
        }}
        onDelete={async () => {
          if (await profiles.remove(editing.id)) finishEdit();
        }}
        onPreferences={
          profiles.profiles.some((item) => item.id === editing.id)
            ? () => {
                profiles.clearError();
                setPreferencesFor(editing);
              }
            : undefined
        }
        onCancel={finishEdit}
      />
    );

  return (
    <section className="profile-selector" aria-labelledby="profile-title">
      <div className="profile-selector-header">
        <h1 id="profile-title">{managing ? 'Manage Profiles' : 'Who’s Watching?'}</h1>
        {managing && (
          <p className="profile-selector-subtitle">
            Select a profile to edit its name, color, or preferences.
          </p>
        )}
      </div>

      <div className="profile-list">
        {profiles.profiles.map((profile) => (
          <button
            type="button"
            className={
              profile.id === profiles.activeId
                ? 'profile-tile is-active-profile'
                : 'profile-tile'
            }
            key={profile.id}
            onClick={() => {
              profiles.clearError();
              if (managing) setEditing(profile);
              else if (profiles.select(profile.id)) onSelected();
            }}
            aria-label={managing ? `Edit ${profile.name}` : `Watch as ${profile.name}`}
          >
            <span className="profile-avatar-wrapper">
              <ProfileAvatar profile={profile} size={104} />
              {managing && (
                <span className="profile-manage-overlay" aria-hidden="true">
                  <svg
                    viewBox="0 0 256 256"
                    width="24"
                    height="24"
                    fill="currentColor"
                    aria-hidden="true"
                  >
                    <path d="M227.32,73.37,182.63,28.69a16,16,0,0,0-22.63,0L36.69,152A15.86,15.86,0,0,0,32,163.31V208a16,16,0,0,0,16,16H92.69A15.86,15.86,0,0,0,104,219.31L227.32,96a16,16,0,0,0,0-22.63ZM92.69,200H56V163.31l88-88L180.69,112ZM192,100.69,155.31,64l16-16L208,84.69Z" />
                  </svg>
                </span>
              )}
            </span>
            <span className="profile-name">{profile.name}</span>
          </button>
        ))}
        {profiles.profiles.length < 6 && (
          <button
            type="button"
            className="profile-tile profile-add"
            onClick={() => {
              profiles.clearError();
              setEditing({
                id: createProfileId(),
                name: '',
                color: profileColors[profiles.profiles.length] ?? profileColors[0],
                preferences: emptyPreferences,
              });
            }}
          >
            <span className="profile-avatar-wrapper">
              <span className="profile-avatar profile-add-avatar" aria-hidden="true">
                <Icon name="plus" />
              </span>
            </span>
            <span className="profile-name">Add Profile</span>
          </button>
        )}
      </div>

      {profiles.error && (
        <p className="profile-error" role="alert">
          {profiles.error}
        </p>
      )}

      <button
        type="button"
        className="profile-manage-button"
        onClick={() => {
          profiles.clearError();
          setManaging(!managing);
        }}
      >
        {managing ? 'Done' : 'Manage Profiles'}
      </button>
    </section>
  );
}
