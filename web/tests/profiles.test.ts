import { expect, test } from 'bun:test';
import { viewProfile } from '../src/lib/wire';
import { profileSchema } from '../src/model';

test('server profile IDs and independent favorites become the existing view model', () => {
  const profile = viewProfile(
    {
      id: 'server-allocated-id',
      name: 'Viewer',
      color: '#446e88',
      preferences: { completed: true, format: 'both', genres: ['drama'] },
    },
    [
      {
        id: 7,
        type: 'movie',
        title: 'Example',
        overview: '',
        rating: 0,
        year: '',
        posterUrl: null,
      },
    ],
  );
  expect(profile.id).toBe('server-allocated-id');
  expect(profile.preferences.likedTitles).toEqual([{ id: 7, type: 'movie', title: 'Example' }]);
  expect(profileSchema.safeParse(profile).success).toBe(true);
});
test('completed API onboarding can exist before favorite choices are cached', () => {
  expect(
    profileSchema.safeParse(
      viewProfile({
        id: 'server-id',
        name: 'Viewer',
        color: '#446e88',
        preferences: { completed: true, format: 'movie', genres: [] },
      }),
    ).success,
  ).toBe(true);
});
