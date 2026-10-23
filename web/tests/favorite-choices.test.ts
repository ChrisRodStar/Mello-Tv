import { expect, test } from 'bun:test';
import { selectFavoriteChoices } from '../src/lib/favorite-choices';
import type { Title } from '../src/model';

const title = (id: number, name: string, type: 'movie' | 'tv' = 'movie'): Title => ({
  id,
  type,
  title: name,
  overview: '',
  year: '2020',
  rating: 8,
  posterUrl: `/api/v1/artwork/w500/${id}.jpg`,
});

test('favorites respect selected genres, balance them, and exclude unrelated general shelves', () => {
  const shelves = [
    { id: 'favorites-movie-action', titles: [title(1, 'Action'), title(2, 'Action 2')] },
    { id: 'favorites-movie-scifi', titles: [title(3, 'Science fiction')] },
    { id: 'favorites-movie-animation', titles: [title(4, 'Anime')] },
    { id: 'favorites-movie-horror', titles: [title(5, 'Horror')] },
    { id: 'favorites-movie-comedy', titles: [title(6, 'Unselected comedy')] },
    { id: 'tv-popular', titles: [title(7, 'Unrelated variety show', 'tv')] },
  ];
  const choices = selectFavoriteChoices(shelves, {
    genres: ['action', 'scifi', 'animation', 'horror'],
    format: 'both',
  });
  expect(choices.map((item) => item.id)).toEqual([1, 3, 4, 5, 2]);
});

test('movie/TV formats are honored and duplicates do not crowd out genre coverage', () => {
  const shelves = [
    { id: 'favorites-movie-action', titles: [title(1, 'Shared'), title(2, 'Action')] },
    { id: 'favorites-movie-scifi', titles: [title(1, 'Shared'), title(3, 'Space')] },
    { id: 'favorites-tv-action', titles: [title(4, 'Action TV', 'tv')] },
    { id: 'favorites-tv-scifi', titles: [title(5, 'Space TV', 'tv')] },
  ];
  const preferences = { genres: ['action', 'scifi'] as const };
  expect(
    selectFavoriteChoices(shelves, { genres: [...preferences.genres], format: 'tv' }).map(
      (item) => item.id,
    ),
  ).toEqual([4, 5]);
  expect(
    selectFavoriteChoices(shelves, { genres: [...preferences.genres], format: 'movie' }).map(
      (item) => item.id,
    ),
  ).toEqual([1, 3, 2]);
  expect(
    selectFavoriteChoices(shelves, { genres: [...preferences.genres], format: 'both' })
      .slice(0, 2)
      .map((item) => item.type),
  ).toEqual(['movie', 'tv']);
});

test('cold-cache fallback stays within selected genres and never turns TV-only into movies', () => {
  const shelves = [
    { id: 'onboarding-animation', titles: [title(129, 'Spirited Away')] },
    { id: 'onboarding-drama', titles: [title(8, 'Drama')] },
  ];
  expect(
    selectFavoriteChoices(shelves, { genres: ['animation'], format: 'movie' }).map(
      (item) => item.id,
    ),
  ).toEqual([129]);
  expect(selectFavoriteChoices(shelves, { genres: ['animation'], format: 'tv' })).toEqual([]);
});
