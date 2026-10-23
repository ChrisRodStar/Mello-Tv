import { useState } from 'react';

import { profileColors, type Profile } from '../../lib/profiles';
import { ProfileAvatar } from './ProfileAvatar';

export function ProfileForm({
  profile,
  canDelete,
  error,
  saving = false,
  canCancel = true,
  onSave,
  onDelete,
  onCancel,
  onPreferences,
}: {
  profile: Profile;
  canDelete: boolean;
  error: string;
  saving?: boolean;
  canCancel?: boolean;
  onSave: (profile: Profile) => void;
  onDelete: () => void;
  onCancel: () => void;
  onPreferences?: () => void;
}) {
  const [name, setName] = useState(profile.name);
  const [color, setColor] = useState(profile.color);
  const [confirmDelete, setConfirmDelete] = useState(false);
  return (
    <section className="profile-editor" aria-labelledby="profile-form-title">
      <h1 id="profile-form-title">{profile.name ? 'Edit profile' : 'Add profile'}</h1>
      <form
        aria-busy={saving}
        onSubmit={(event) => {
          event.preventDefault();
          if (name.trim() && !saving) onSave({ ...profile, name: name.trim(), color });
        }}
      >
        <ProfileAvatar profile={{ name, color }} />
        <label htmlFor="profile-name">Name</label>
        <input
          id="profile-name"
          value={name}
          onChange={(event) => setName(event.target.value)}
          maxLength={24}
          autoFocus
          autoComplete="off"
          required
          disabled={saving}
        />
        <fieldset disabled={saving}>
          <legend>Avatar color</legend>
          <div className="profile-colors">
            {profileColors.map((option) => (
              <button
                key={option}
                type="button"
                style={{ backgroundColor: option }}
                aria-label={
                  ['Blue', 'Terracotta', 'Olive', 'Violet', 'Gold', 'Teal'][
                    profileColors.indexOf(option)
                  ]
                }
                aria-pressed={color === option}
                onClick={() => setColor(option)}
              />
            ))}
          </div>
        </fieldset>
        {error && (
          <p className="profile-error" role="alert">
            {error}
          </p>
        )}
        <div className="profile-form-actions">
          <button className="profile-primary" disabled={!name.trim() || saving} type="submit">
            {saving ? 'Saving…' : 'Save'}
          </button>
          {canCancel && (
            <button
              className="profile-secondary"
              type="button"
              onClick={onCancel}
              disabled={saving}
            >
              Cancel
            </button>
          )}
        </div>
        {onPreferences && (
          <button
            className="profile-secondary profile-preferences-link"
            type="button"
            disabled={saving}
            onClick={onPreferences}
          >
            Preferences
          </button>
        )}
        {canDelete && (
          <div className="profile-delete">
            {confirmDelete ? (
              <>
                <p>Delete {profile.name}?</p>
                <button type="button" onClick={onDelete} disabled={saving}>
                  Delete profile
                </button>
                <button type="button" onClick={() => setConfirmDelete(false)} disabled={saving}>
                  Keep profile
                </button>
              </>
            ) : (
              <button type="button" onClick={() => setConfirmDelete(true)} disabled={saving}>
                Delete profile
              </button>
            )}
          </div>
        )}
      </form>
    </section>
  );
}
