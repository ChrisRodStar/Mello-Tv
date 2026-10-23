import { expect, test } from 'bun:test';
import { preferenceGenres } from '../src/model';
import { genreArtworkCandidates, loadGenreArtwork } from '../src/lib/genre-artwork';

const image = (name: string) => `https://image.tmdb.org/t/p/w1280/${name}.jpg`;
const summaries = preferenceGenres.map((genre) => ({
  id: `onboarding-${genre.id}`,
  titles: [
    { title: `${genre.id} first`, backdrop: image(`${genre.id}-first`) },
    { title: `${genre.id} second`, backdrop: image(`${genre.id}-second`) },
  ],
}));

test('summary-only genre shelves populate all eight cards with distinct artwork', async () => {
  const artwork = await loadGenreArtwork(
    summaries,
    new AbortController().signal,
    async () => true,
  );
  expect(Object.keys(artwork)).toHaveLength(8);
  expect(new Set(Object.values(artwork).map((item) => item.imageUrl)).size).toBe(8);
  expect(artwork.animation?.imageUrl).toBe(image('animation-first'));
});

test('a cache miss falls back within its category and never uses unrelated titles', async () => {
  const shelves = [
    ...summaries,
    { id: 'tv-drama', titles: [{ title: 'Unqualified show', backdrop: image('unrelated') }] },
  ];
  const artwork = await loadGenreArtwork(
    shelves,
    new AbortController().signal,
    async (url) => url !== image('animation-first'),
  );
  expect(artwork.animation?.imageUrl).toBe(image('animation-second'));
  expect(artwork.drama?.imageUrl).toBe(image('drama-first'));
  expect(
    genreArtworkCandidates([{ id: 'trending', titles: shelves[0]!.titles }]).animation,
  ).toEqual([]);
});

test('unsafe artwork and cancellation do not start image requests', async () => {
  expect(
    genreArtworkCandidates([
      {
        id: 'onboarding-horror',
        titles: [{ title: 'Bad', backdrop: 'https://external.test/private.jpg' }],
      },
    ]).horror,
  ).toEqual([]);
  const controller = new AbortController();
  controller.abort();
  let calls = 0;
  const artwork = await loadGenreArtwork(summaries, controller.signal, async () => {
    calls++;
    return true;
  });
  expect(artwork).toEqual({});
  expect(calls).toBe(0);
});
