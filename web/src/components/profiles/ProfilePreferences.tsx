import { useEffect, useRef } from 'react';

import {
  preferenceGenres,
  type ProfilePreferences as Preferences,
  type Profile,
} from '../../model';

import { PreferenceTitlePicker } from './PreferenceTitlePicker';
import { useGenreArtwork } from './useGenreArtwork';
import { usePreferenceSetup } from './usePreferenceSetup';

export function ProfilePreferences({
  profile,
  saving,
  error,
  onSave,
  onCancel,
}: {
  profile: Profile;
  saving: boolean;
  error: string;
  onSave: (preferences: Preferences) => Promise<boolean>;
  onCancel?: () => void;
}) {
  const setup = usePreferenceSetup(profile, onSave);
  const artwork = useGenreArtwork(profile.id);
  const heading = useRef<HTMLHeadingElement>(null);
  useEffect(() => {
    heading.current?.focus();
  }, [setup.step]);
  const busy = saving || setup.busy;
  const message = setup.error || error;

  if (setup.step === 'generating')
    return (
      <section
        className="profile-preferences preference-building"
        aria-busy="true"
        role="status"
        aria-live="polite"
      >
        <div className="preference-building-posters" aria-hidden="true">
          {setup.titles
            .filter((title) =>
              setup.likedTitles.some(
                (seed) => seed.id === title.id && seed.type === title.type,
              ),
            )
            .slice(0, 3)
            .map((title) => (
              <img key={`${title.type}-${title.id}`} src={title.posterUrl!} alt="" />
            ))}
        </div>
        <div className="preference-loading" aria-hidden="true">
          <span />
          <span />
          <span />
        </div>
        <h1 ref={heading} tabIndex={-1}>
          Saving your preferences
        </h1>
      </section>
    );
  return (
    <section
      className={
        setup.step === 'titles'
          ? 'profile-preferences preference-title-step'
          : 'profile-preferences preference-genre-step'
      }
      aria-labelledby="preferences-title"
      aria-label={
        setup.step === 'genres'
          ? 'Choose at least three genres'
          : 'Choose at least three favorite titles'
      }
    >
      <div
        className="preference-progress"
        aria-label={setup.step === 'genres' ? 'Step 1 of 3' : 'Step 2 of 3'}
      >
        <span className="is-complete" />
        <span className={setup.step === 'titles' ? 'is-complete' : undefined} />
        <span />
      </div>
      <h1 id="preferences-title" ref={heading} tabIndex={-1}>
        {setup.step === 'genres' ? 'Make it yours' : 'Pick a few favorites'}
      </h1>
      <form
        aria-busy={busy}
        onSubmit={(event) => {
          event.preventDefault();
          if (!busy) void (setup.step === 'genres' ? setup.next() : setup.generate());
        }}
      >
        {setup.step === 'genres' ? (
          <>
            <fieldset disabled={busy}>
              <legend className="visually-hidden">Favorite genres</legend>
              <div className="preference-genres">
                {preferenceGenres.map((genre) => {
                  const selected = setup.genres.includes(genre.id);
                  return (
                    <button
                      key={genre.id}
                      type="button"
                      aria-pressed={selected}
                      onClick={() =>
                        setup.setGenres((current) =>
                          selected
                            ? current.filter((id) => id !== genre.id)
                            : [...current, genre.id],
                        )
                      }
                    >
                      {artwork[genre.id]?.imageUrl && (
                        <img
                          className="preference-genre-image"
                          src={artwork[genre.id]!.imageUrl!}
                          alt=""
                          decoding="async"
                        />
                      )}
                      <span className="preference-genre-shade" aria-hidden="true" />
                      <span className="preference-genre-name">{genre.name}</span>
                      <span className="preference-mark" aria-hidden="true">
                        <svg
                          viewBox="0 0 24 24"
                          fill="none"
                          stroke="currentColor"
                          strokeWidth="1.8"
                        >
                          {selected ? (
                            <path d="m5 12 4 4 10-10" />
                          ) : (
                            <path d="M12 5v14M5 12h14" />
                          )}
                        </svg>
                      </span>
                    </button>
                  );
                })}
              </div>
            </fieldset>
            <fieldset disabled={busy} className="preference-format">
              <legend className="visually-hidden">Movies or TV shows</legend>
              <div>
                {(
                  [
                    ['both', 'Both'],
                    ['movie', 'Movies'],
                    ['tv', 'TV shows'],
                  ] as const
                ).map(([value, label]) => (
                  <button
                    key={value}
                    type="button"
                    aria-pressed={setup.format === value}
                    onClick={() => setup.setFormat(value)}
                  >
                    {label}
                  </button>
                ))}
              </div>
            </fieldset>
          </>
        ) : (
          <>
            <PreferenceTitlePicker
              key={setup.titles.map((title) => `${title.type}:${title.id}`).join('|')}
              titles={setup.titles}
              selected={setup.likedTitles}
              busy={busy}
              onToggle={(title) =>
                setup.setLikedTitles((current) =>
                  current.some((seed) => seed.id === title.id && seed.type === title.type)
                    ? current.filter((seed) => seed.id !== title.id || seed.type !== title.type)
                    : current.length >= 50
                      ? current
                      : [...current, { id: title.id, type: title.type, title: title.title }],
                )
              }
            />
          </>
        )}
        <div
          className={
            setup.step === 'titles'
              ? 'preference-actions preference-actions-sticky'
              : 'preference-actions'
          }
        >
          {message && (
            <p className="profile-error" role="alert">
              {message}
            </p>
          )}
          <div className="profile-form-actions">
            {setup.step === 'titles' && (
              <button
                className="profile-secondary"
                type="button"
                disabled={busy}
                onClick={setup.back}
              >
                Back
              </button>
            )}
            <button
              className="profile-primary"
              type="submit"
              disabled={
                busy ||
                (setup.step === 'genres'
                  ? setup.genres.length < 3
                  : setup.likedTitles.length < 3)
              }
            >
              {busy
                ? 'Loading…'
                : setup.step === 'genres'
                  ? setup.genres.length >= 3
                    ? 'Continue'
                    : `${setup.genres.length} / 3 selected`
                  : setup.likedTitles.length < 3
                    ? `${setup.likedTitles.length} / 3 selected`
                    : 'Save preferences'}
            </button>
            {setup.step === 'titles' && (
              <button
                className="profile-secondary"
                type="button"
                disabled={busy}
                onClick={() => void setup.generate(true)}
              >
                Choose favorites later
              </button>
            )}
            {onCancel && setup.step === 'genres' && (
              <button
                className="profile-secondary"
                type="button"
                disabled={busy}
                onClick={onCancel}
              >
                Cancel
              </button>
            )}
          </div>
        </div>
      </form>
    </section>
  );
}
