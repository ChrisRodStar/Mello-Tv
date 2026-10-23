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
      <h1 id="profile-title">{managing ? 'Manage profiles' : 'Who’s watching?'}</h1>
      <div className="profile-list">
        {profiles.profiles.map((profile) => (
          <button
            className="profile-tile"
            key={profile.id}
            onClick={() => {
              profiles.clearError();
              if (managing) setEditing(profile);
              else if (profiles.select(profile.id)) onSelected();
            }}
            aria-label={managing ? `Edit ${profile.name}` : `Watch as ${profile.name}`}
          >
            <ProfileAvatar profile={profile} />
            <span className="profile-name">{profile.name}</span>
            <span className="profile-current">
              {profile.id === profiles.activeId ? 'Current' : '\u00a0'}
            </span>
          </button>
        ))}
        {profiles.profiles.length < 6 && (
          <button
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
            <span className="profile-avatar">
              <Icon name="plus" />
            </span>
            <span className="profile-name">Add profile</span>
            <span className="profile-current" aria-hidden="true">
              &nbsp;
            </span>
          </button>
        )}
      </div>
      {profiles.error && (
        <p className="profile-error" role="alert">
          {profiles.error}
        </p>
      )}
      <button
        className="profile-secondary"
        onClick={() => {
          profiles.clearError();
          setManaging(!managing);
        }}
      >
        {managing ? 'Done' : 'Manage profiles'}
      </button>
    </section>
  );
}
