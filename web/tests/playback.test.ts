import { expect, test } from 'bun:test';
import { publicArtwork, streamURL } from '../src/lib/api';
import { viewTitle, viewProgress, playbackWire } from '../src/lib/wire';

// Synthetic fixture only. No provider, relay, VPN or external media requests.
const origin = 'http://127.0.0.1:5173';
function withWindow<T>(work: () => T): T {
  const before = globalThis.window;
  Object.defineProperty(globalThis, 'window', {
    configurable: true,
    value: { location: { origin } },
  });
  try {
    return work();
  } finally {
    Object.defineProperty(globalThis, 'window', { configurable: true, value: before });
  }
}
test('stream capabilities stay on the single origin and retain their query', () =>
  withWindow(() => {
    expect(streamURL('/stream/opaque-session/entry?st=synthetic.jwt')).toBe(
      origin + '/stream/opaque-session/entry?st=synthetic.jwt',
    );
    for (const url of [
      'https://upstream.example/movie.mp4',
      '/api/hls/old/entry',
      '/stream/opaque-session/entry?st=a&st=b',
      '/stream/opaque-session/entry?st=a&other=b',
    ])
      expect(() => streamURL(url)).toThrow();
  }));
test('artwork uses only the trusted CDN and normalizes legacy references', () => {
  expect(publicArtwork('/api/v1/artwork/w500/poster.jpg')).toBe(
    'https://image.tmdb.org/t/p/w500/poster.jpg',
  );
  expect(publicArtwork('https://image.tmdb.org/poster.jpg')).toBeNull();
  expect(publicArtwork('/internal/v1/artwork/w500/poster.jpg')).toBe(
    'https://image.tmdb.org/t/p/w500/poster.jpg',
  );
  expect(publicArtwork('https://image.tmdb.org.evil.test/t/p/w500/poster.jpg')).toBeNull();
  expect(publicArtwork('https://image.tmdb.org/t/p/w500/poster.jpg?token=secret')).toBeNull();
});
test('catalog identities, cross IDs, and public units map explicitly', () => {
  const title = viewTitle({
    type: 'tv',
    id: 7,
    title: 'Series',
    overview: '',
    date: '2026-01-01',
    rating: 8,
    details: true,
    imdb_id: 'tt1234',
    mal_id: 42,
    poster: '/api/v1/artwork/w500/poster.jpg',
  });
  expect(title.year).toBe('2026');
  expect(title.malId).toBe(42);
  const progress = viewProgress(
    {
      type: 'tv',
      id: 7,
      season: 0,
      episode: 2,
      position_seconds: 3000,
      duration_seconds: 3600,
      watched_seconds: 600,
      completed: false,
      updated_at: '2026-10-17T12:00:00Z',
    },
    title,
  );
  expect(progress.season).toBe(0);
  expect(progress.position).toBe(3000);
  expect(progress.watchedSeconds).toBe(600);
});
test('relay reload and lease metadata are required', () => {
  expect(playbackWire.safeParse({ url: '/stream/id/entry?st=x' }).success).toBe(false);
});
