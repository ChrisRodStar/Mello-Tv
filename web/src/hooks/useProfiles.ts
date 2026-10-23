import { readProfiles, saveProfile } from '../lib/profile-api';
import { apiJSON, noContent, profilePath, combineSignal } from '../lib/api';
import { useCallback, useEffect, useRef, useState } from 'react';

import { profileListSchema } from '../model';

import { initialProfiles, type Profile, type ProfileState } from '../lib/profiles';

const activeKey = 'family-stream.web.v1.active-profile';
async function requestProfiles(_path: string, options: RequestInit) {
  return readProfiles(options.signal ?? AbortSignal.timeout(12000));
}

export function useProfiles() {
  const [state, setState] = useState<ProfileState>(() => {
    return initialProfiles;
  });
  const [ready, setReady] = useState(false);
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState('');
  const current = useRef(state);
  current.current = state;
  const busy = useRef(false);
  const alive = useRef(true);
  const loaded = useRef(false);
  const revision = useRef(0);
  const pending = useRef<AbortController | null>(null);

  const apply = useCallback((profiles: Profile[], preferred?: string) => {
    let remembered = '';
    try {
      remembered = localStorage.getItem(activeKey) ?? '';
    } catch {}
    const active = preferred ?? (current.current.activeId || remembered);
    const activeId = profiles.some((p) => p.id === active) ? active : '';
    const next = { profiles, activeId };
    current.current = next;
    setState(next);
    try {
      localStorage.setItem(activeKey, activeId);
    } catch {
      /* Selection still works in memory when local storage is unavailable. */
    }
  }, []);

  const refresh = useCallback(async () => {
    if (busy.current) return;
    pending.current?.abort();
    const controller = new AbortController();
    pending.current = controller;
    const token = ++revision.current;
    if (!loaded.current) setLoading(true);
    try {
      const profiles = await requestProfiles('', {
        signal: combineSignal(controller.signal, 20000),
      });
      if (!alive.current || controller.signal.aborted || token !== revision.current) return;
      apply(profiles);
      loaded.current = true;
      setReady(true);
      setError('');
    } catch {
      if (alive.current && !controller.signal.aborted && token === revision.current)
        setError('Couldn’t load profiles. Try again.');
    } finally {
      if (alive.current && token === revision.current) setLoading(false);
    }
  }, [apply]);

  useEffect(() => {
    alive.current = true;
    void refresh();
    const update = () => {
      if (document.visibilityState === 'visible') void refresh();
    };
    const interval = setInterval(update, 30000);
    window.addEventListener('focus', update);
    document.addEventListener('visibilitychange', update);
    return () => {
      alive.current = false;
      revision.current++;
      pending.current?.abort();
      clearInterval(interval);
      window.removeEventListener('focus', update);
      document.removeEventListener('visibilitychange', update);
    };
  }, [refresh]);

  async function mutate(path: string, method: string, profile?: Profile) {
    if (busy.current || !loaded.current) return false;
    busy.current = true;
    revision.current++;
    pending.current?.abort();
    const controller = new AbortController();
    pending.current = controller;
    setSaving(true);
    setError('');
    try {
      const signal = combineSignal(controller.signal, 30000);
      let savedID: string | undefined;
      if (method === 'DELETE')
        await apiJSON(profilePath(path.slice(1)), noContent, { method: 'DELETE', signal });
      else if (profile)
        savedID = await saveProfile(
          profile,
          method === 'POST',
          current.current.profiles.find((p) => p.id === profile.id),
          signal,
        );
      const profiles = await readProfiles(signal);
      if (!alive.current || controller.signal.aborted) return false;
      apply(
        profiles,
        savedID && (!current.current.activeId || method === 'POST') ? savedID : undefined,
      );
      return true;
    } catch (error) {
      if (alive.current && !controller.signal.aborted)
        setError(error instanceof Error ? error.message : 'Couldn’t save profiles. Try again.');
      return false;
    } finally {
      busy.current = false;
      if (alive.current) setSaving(false);
    }
  }

  return {
    ...state,
    ready,
    loading,
    saving,
    error,
    refresh,
    clearError: () => setError(''),
    select: (id: string) => {
      if (!ready || saving || !current.current.profiles.some((p) => p.id === id)) return false;
      apply(current.current.profiles, id);
      setError('');
      return true;
    },
    upsert: (profile: Profile) =>
      current.current.profiles.some((p) => p.id === profile.id)
        ? mutate('/' + encodeURIComponent(profile.id), 'PUT', profile)
        : mutate('', 'POST', profile),
    remove: (id: string) => mutate('/' + encodeURIComponent(id), 'DELETE'),
  };
}
