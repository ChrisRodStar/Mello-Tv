import { z } from 'zod';
import { apiJSON, noContent, profilePath } from './api';
import {
  profilesWire,
  profileWire,
  savedTitlesWire,
  viewProfile,
  viewTitle,
  preferencesWire,
} from './wire';
import type { Profile } from '../model';
import { rememberTitle } from './catalog';

export async function mapLimited<T, R>(
  values: T[],
  work: (v: T) => Promise<R>,
  limit = 4,
): Promise<R[]> {
  const out = new Array<R>(values.length);
  let index = 0;
  await Promise.all(
    Array.from({ length: Math.min(limit, values.length) }, async () => {
      for (;;) {
        const i = index++;
        if (i >= values.length) return;
        out[i] = await work(values[i]!);
      }
    }),
  );
  return out;
}
export async function readProfiles(signal: AbortSignal): Promise<Profile[]> {
  const { profiles } = await apiJSON('/api/v1/profiles', profilesWire, { signal });
  return mapLimited(profiles, async (p) => {
    const { titles } = await apiJSON(`${profilePath(p.id)}/favorites`, savedTitlesWire, {
      signal,
    });
    return viewProfile(
      p,
      titles.map((t) => rememberTitle(viewTitle(t))),
    );
  });
}
export async function saveProfile(
  profile: Profile,
  isNew: boolean,
  previous: Profile | undefined,
  signal: AbortSignal,
): Promise<string> {
  const path = isNew ? '/api/v1/profiles' : profilePath(profile.id);
  const p = await apiJSON(path, profileWire, {
    method: isNew ? 'POST' : 'PUT',
    signal,
    body: JSON.stringify({ name: profile.name, color: profile.color }),
  });
  // The server allocates identity. Never reuse the form's temporary client ID.
  const pid = p.id;
  if (
    !isNew &&
    previous &&
    JSON.stringify(previous.preferences) !== JSON.stringify(profile.preferences)
  ) {
    const before = new Map(
      previous.preferences.likedTitles.map((t) => [`${t.type}:${t.id}`, t]),
    );
    const after = new Map(profile.preferences.likedTitles.map((t) => [`${t.type}:${t.id}`, t]));
    if (after.size > 50) throw new Error('Choose up to 50 favorites.');
    // Reconcile the replaceable set via idempotent item routes. Save completion
    // last; a failed favorite write cannot falsely finish onboarding.
    for (const [k, t] of before)
      if (!after.has(k))
        await apiJSON(`${profilePath(pid)}/favorites/${t.type}/${t.id}`, noContent, {
          method: 'DELETE',
          signal,
        });
    for (const [k, t] of after)
      if (!before.has(k))
        await apiJSON(`${profilePath(pid)}/favorites/${t.type}/${t.id}`, noContent, {
          method: 'PUT',
          signal,
        });
    const { completed, format, genres } = profile.preferences;
    await apiJSON(`${profilePath(pid)}/preferences`, z.unknown(), {
      method: 'PUT',
      signal,
      body: JSON.stringify({ completed, format, genres }),
    });
  }
  return pid;
}
