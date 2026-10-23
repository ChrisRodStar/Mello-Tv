import { z } from 'zod';
import { apiJSON, isMissing, profilePath, publicArtwork } from './api';
import {
  recordWire,
  shelfWire,
  recommendationsWire,
  titleWire,
  seasonWire,
  viewTitle,
} from './wire';
import type { Catalog, Title, Details, Episode } from '../model';
import type { Category } from '../types';
import { selectFavoriteChoices } from './favorite-choices';
import type { ProfilePreferences } from '../model';
import { awaitMetadata } from './await-metadata';

const titles = new Map<string, Title>();
export function rememberTitle(t: Title) {
  if (titles.size >= 1500) titles.delete(titles.keys().next().value!);
  titles.set(`${t.type}:${t.id}`, t);
  return t;
}
export function clearCatalogMemory() {
  titles.clear();
}
export function cachedTitle(type: Title['type'], id: number): Title {
  return (
    titles.get(`${type}:${id}`) ?? {
      type,
      id,
      title: '',
      overview: '',
      year: '',
      rating: 0,
      posterUrl: null,
    }
  );
}
export async function loadTitle(type: Title['type'], id: number, signal: AbortSignal) {
  const record = await apiJSON(`/api/v1/titles/${type}/${id}`, recordWire(titleWire), {
    signal,
  });
  if (record.data.type !== type || record.data.id !== id)
    throw new Error('Invalid title response');
  return { record, title: rememberTitle(viewTitle(record.data)) };
}
export async function enrichTitle(type: Title['type'], id: number, signal: AbortSignal) {
  const known = cachedTitle(type, id);
  if (known.title) return known;
  try {
    return (await loadTitle(type, id, signal)).title;
  } catch {
    signal.throwIfAborted();
    return known;
  }
}
export const browseSchema = z.object({ records: z.array(recordWire(shelfWire)) });
export async function loadBrowse(signal: AbortSignal) {
  return apiJSON('/api/v1/browse', browseSchema, { signal });
}
export async function loadCatalog(
  profileID: string,
  category: Category,
  signal: AbortSignal,
): Promise<Catalog> {
  const browse = await loadBrowse(signal);
  let personalized: z.infer<typeof recommendationsWire> | undefined;
  try {
    const r = await apiJSON(
      `${profilePath(profileID)}/recommendations?type=${category}`,
      recordWire(recommendationsWire),
      { signal },
    );
    personalized = r.data;
  } catch (error) {
    if (!isMissing(error)) {
      signal.throwIfAborted(); /* General browsing remains usable if a personalized result fails. */
    }
  }
  const seenShelves = new Set<string>();
  const shelves = [
    ...(personalized?.sections ?? []).filter(
      (s) => !browse.records.some((r) => r.data.id === s.id),
    ),
    ...browse.records
      .filter(
        (r) => !r.data.id.startsWith('onboarding-') && !r.data.id.startsWith('favorites-'),
      )
      .map((r) => r.data),
  ];
  const sections = shelves
    .filter((s) => {
      if (seenShelves.has(s.id)) return false;
      seenShelves.add(s.id);
      return true;
    })
    .map((s) => ({
      id: s.id,
      heading: s.heading,
      titles: s.titles
        .filter((t) => category === 'all' || t.type === category)
        .map((t) => rememberTitle(viewTitle(t))),
    }))
    .filter((s) => s.titles.length);
  const seen = new Set<string>();
  const all = sections
    .flatMap((s) => s.titles)
    .filter((t) => {
      const k = `${t.type}:${t.id}`;
      if (seen.has(k)) return false;
      seen.add(k);
      return true;
    });
  return { configured: true, titles: all, sections };
}
export async function loadDetails(title: Title, signal: AbortSignal): Promise<Details> {
  const { record } = await awaitMetadata(
    () => loadTitle(title.type, title.id, signal),
    (value) => value.record.data.details,
    signal,
  );
  const t = record.data;
  if (!t.details)
    throw new Error('Details are being added to the saved catalog. Try again shortly.');
  return {
    runtime: t.runtime ?? null,
    genres: t.genres ?? [],
    seasons: (t.seasons ?? []).map((s) => ({
      number: s.season,
      name: s.name,
      episodeCount: s.episode_count,
    })),
  };
}
export async function loadEpisodes(
  showID: number,
  season: number,
  signal: AbortSignal,
): Promise<Episode[]> {
  const { data } = await awaitMetadata(
    () => apiJSON(`/api/v1/tv/${showID}/seasons/${season}`, recordWire(seasonWire), { signal }),
    () => true,
    signal,
  );
  if (data.show_id !== showID || data.season !== season)
    throw new Error('Invalid season response');
  return data.episodes.map((e) => ({
    number: e.episode,
    name: e.name,
    overview: e.overview,
    runtime: e.runtime || null,
    stillUrl: publicArtwork(e.still),
  }));
}
export async function loadChoices(
  signal: AbortSignal,
  preferences: Pick<ProfilePreferences, 'genres' | 'format'>,
): Promise<Title[]> {
  const { records } = await loadBrowse(signal);
  return selectFavoriteChoices(
    records.map(({ data }) => ({ id: data.id, titles: data.titles.map(viewTitle) })),
    preferences,
  ).map(rememberTitle);
}
