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
    if (step !== 'titles') return () => pending.current?.abort();
    const controller = new AbortController();
    let retry: ReturnType<typeof setTimeout> | undefined;
    async function refresh(attempt: number) {
      try {
        const choices = await loadChoices(
          AbortSignal.any([controller.signal, AbortSignal.timeout(15000)]),
          { genres, format },
        );
        if (controller.signal.aborted) return;
        if (choices.length) setTitles(choices);
        if (choices.length >= 24 || attempt >= 24) return;
      } catch {
        if (controller.signal.aborted || attempt >= 24) return;
      }
      retry = setTimeout(() => void refresh(attempt + 1), 5000);
    }
    void refresh(0);
    return () => {
      controller.abort();
      clearTimeout(retry);
    };
  }, [step, genres, format, likedTitles, profile.id]);
  async function next() {
    if (locked.current || genres.length < 3) return;
    locked.current = true;
    const controller = new AbortController();
    pending.current = controller;
    setBusy(true);
    setError('');
    try {
      const choices = await loadChoices(
        AbortSignal.any([controller.signal, AbortSignal.timeout(15000)]),
        { genres, format },
      );
      if (controller.signal.aborted) return;
      setTitles(choices);
      setStep('titles');
    } catch {
      if (!controller.signal.aborted) setError('Couldn’t load saved title choices. Try again.');
    } finally {
      locked.current = false;
      if (!controller.signal.aborted) setBusy(false);
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
    } catch {
      setError('Couldn’t save your preferences. Try again.');
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
