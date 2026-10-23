import { preferenceGenres } from '../model';
import { publicArtwork } from './api';

type Shelf = {
  id: string;
  titles: { title: string; backdrop?: string; poster?: string; genres?: string[] }[];
};
export type GenreArtwork = Record<string, { imageUrl: string; title: string }>;
type Candidate = { imageUrl: string; title: string };

// Only use catalog shelves explicitly qualified for this onboarding category.
// Poster and next-title fallbacks remain within the same genre and language.
export function genreArtworkCandidates(shelves: Shelf[]) {
  return Object.fromEntries(
    preferenceGenres.map((genre) => {
      const unique = new Map<string, Candidate>();
      for (const shelf of shelves.filter((shelf) => shelf.id === `onboarding-${genre.id}`)) {
        for (const title of shelf.titles) {
          for (const value of [title.backdrop, title.poster]) {
            const imageUrl = publicArtwork(value);
            if (imageUrl && !unique.has(imageUrl))
              unique.set(imageUrl, { imageUrl, title: title.title });
          }
        }
      }
      return [genre.id, [...unique.values()].slice(0, 16)];
    }),
  );
}

export function loadArtworkImage(url: string, signal: AbortSignal): Promise<boolean> {
  return new Promise((resolve) => {
    if (signal.aborted) return resolve(false);
    const image = new Image();
    const finish = (loaded: boolean) => {
      clearTimeout(timeout);
      signal.removeEventListener('abort', abort);
      image.onload = image.onerror = null;
      if (!loaded) image.removeAttribute('src');
      resolve(loaded);
    };
    const abort = () => finish(false);
    const timeout = setTimeout(abort, 2000);
    signal.addEventListener('abort', abort, { once: true });
    image.onload = () => finish(image.naturalWidth > 0);
    image.onerror = () => finish(false);
    image.src = url;
  });
}

export async function loadGenreArtwork(
  shelves: Shelf[],
  signal: AbortSignal,
  loadImage = loadArtworkImage,
): Promise<GenreArtwork> {
  const candidates = genreArtworkCandidates(shelves);
  const loaded = new Map<string, Promise<boolean>>();
  const used = new Set<string>();
  const usedTitles = new Set<string>();
  const artwork: GenreArtwork = {};
  await Promise.all(
    preferenceGenres.map(async (genre) => {
      for (const candidate of candidates[genre.id]!) {
        if (signal.aborted) return;
        if (usedTitles.has(candidate.title)) continue;
        let request = loaded.get(candidate.imageUrl);
        if (!request) {
          request = loadImage(candidate.imageUrl, signal);
          loaded.set(candidate.imageUrl, request);
        }
        if (
          (await request) &&
          !signal.aborted &&
          !used.has(candidate.imageUrl) &&
          !usedTitles.has(candidate.title)
        ) {
          used.add(candidate.imageUrl);
          usedTitles.add(candidate.title);
          artwork[genre.id] = candidate;
          return;
        }
      }
    }),
  );
  return artwork;
}
