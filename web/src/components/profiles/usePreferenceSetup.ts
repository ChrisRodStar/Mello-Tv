import { useEffect, useRef, useState } from 'react';
import {
  profilePreferencesSchema,
  type Profile,
  type ProfilePreferences,
  type Title,
} from '../../model';
import { loadChoices } from '../../lib/catalog';

const draftKey = (id: string) => `family-stream.web.v1.onboarding.${id}`;
function readDraft(id: string) {
  try {
    const value = JSON.parse(sessionStorage.getItem(draftKey(id)) ?? 'null');
    const preferences = profilePreferencesSchema.safeParse(value?.preferences);
    if (
      preferences.success &&
      preferences.data.likedTitles.length <= 50 &&
      (value.step === 'genres' || value.step === 'titles')
    )
      return { preferences: preferences.data, step: value.step as 'genres' | 'titles' };
  } catch {
    /* Session storage is optional. */
  }
  return undefined;
}

export function usePreferenceSetup(
  profile: Profile,
  onSave: (p: ProfilePreferences) => Promise<boolean>,
) {
  const [step, setStep] = useState<'genres' | 'titles' | 'generating'>(
    () => readDraft(profile.id)?.step ?? 'genres',
  );
  const [genres, setGenres] = useState(() =>
    (readDraft(profile.id)?.preferences ?? profile.preferences).genres.filter(
      (g) => g !== 'family',
    ),
  );
  const [format, setFormat] = useState(
    () => (readDraft(profile.id)?.preferences ?? profile.preferences).format,
  );
  const [likedTitles, setLikedTitles] = useState(
    () => (readDraft(profile.id)?.preferences ?? profile.preferences).likedTitles,
  );
  const [titles, setTitles] = useState<Title[]>([]);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');
  const pending = useRef<AbortController | null>(null);
  const locked = useRef(false);
  useEffect(() => {
    try {
      if (step !== 'generating')
        sessionStorage.setItem(
          draftKey(profile.id),
          JSON.stringify({
            step,
            preferences: { genres, format, likedTitles, completed: false },
          }),
        );
    } catch {
      /* Keep onboarding usable with storage disabled. */
    }
  }, [step, genres, format, likedTitles, profile.id]);
  useEffect(() => () => pending.current?.abort(), []);
  useEffect(() => {
    if (step !== 'titles') return;
    const controller = new AbortController();
    async function refresh() {
      try {
        const choices = await loadChoices(
          AbortSignal.any([controller.signal, AbortSignal.timeout(120000)]),
          { genres, format },
          profile.id,
        );
        if (controller.signal.aborted) return;
        setTitles(choices);
        if (!choices.length) setError('No saved title choices are available yet. Try again shortly.');
      } catch (cause) {
        if (!controller.signal.aborted)
          setError(cause instanceof Error ? cause.message : 'Couldn’t load saved title choices. Try again.');
      }
    }
    void refresh();
    return () => {
      controller.abort();
    };
  }, [step, genres, format, profile.id]);
  async function next() {
    if (locked.current || genres.length < 3) return;
    locked.current = true;
    const controller = new AbortController();
    pending.current = controller;
    setBusy(true);
    setError('');
    try {
      const choices = await loadChoices(
        AbortSignal.any([controller.signal, AbortSignal.timeout(120000)]),
        { genres, format },
        profile.id,
      );
      if (controller.signal.aborted) return;
      setTitles(choices);
      setStep('titles');
    } catch (cause) {
      if (!controller.signal.aborted)
        setError(cause instanceof Error ? cause.message : 'Couldn’t load saved title choices. Try again.');
    } finally {
      locked.current = false;
      setBusy(false);
    }
  }
  async function generate(skip = false) {
    if (locked.current || !genres.length || (!skip && likedTitles.length < 3)) return;
    locked.current = true;
    setBusy(true);
    setError('');
    setStep('generating');
    try {
      if (!(await onSave({ genres, format, likedTitles, completed: true }))) setStep('titles');
      else {
        try {
          sessionStorage.removeItem(draftKey(profile.id));
        } catch {}
      }
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : 'Couldn’t save your preferences. Try again.');
      setStep('titles');
    } finally {
      locked.current = false;
      setBusy(false);
    }
  }
  return {
    step,
    genres,
    setGenres,
    format,
    setFormat,
    likedTitles,
    setLikedTitles,
    titles,
    busy,
    error,
    next,
    generate,
    back: () => {
      setStep('genres');
      setError('');
    },
  };
}
