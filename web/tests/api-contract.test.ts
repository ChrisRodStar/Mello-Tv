import { expect, test } from 'bun:test';
import { apiPlaybackResolver, changePlayback, type PlaybackRequest } from '../src/lib/playback';
import { clearWebSession } from '../src/lib/profile-storage';
import { settleProgress, trackProgress } from '../src/lib/progress-ledger';

const request: PlaybackRequest = {
  type: 'tv',
  titleId: 9,
  season: 0,
  episode: 2,
  episodeTitle: 'Special',
  title: 'Series',
  profileId: 'owned-profile',
  catalogTitle: {
    type: 'tv',
    id: 9,
    title: 'Series',
    overview: '',
    year: '',
    rating: 0,
    posterUrl: null,
    imdbId: 'tt1234',
    malId: 42,
  },
};
const reply = {
  id: 'api-playback-id',
  title: { type: 'tv', id: 9, season: 0, episode: 2 },
  url: '/stream/different-relay-id/entry?st=synthetic.jwt',
  format: 'mp4',
  expires_at: '2026-10-18T12:00:00Z',
  revision: 8,
  source_generation: 3,
  reload_required: true,
  resume_seconds: 14,
};
async function browser<T>(work: () => Promise<T>) {
  const before = globalThis.window;
  Object.defineProperty(globalThis, 'window', {
    configurable: true,
    value: { location: { origin: 'http://127.0.0.1:5173' }, dispatchEvent: () => true },
  });
  try {
    return await work();
  } finally {
    Object.defineProperty(globalThis, 'window', { configurable: true, value: before });
  }
}
test('setup retries retain one key, exact specials/cross IDs and cookie auth', () =>
  browser(async () => {
    const original = globalThis.fetch;
    const calls: { path: string; options: RequestInit }[] = [];
    globalThis.fetch = (async (path: any, options: any) => {
      calls.push({ path: String(path), options });
      return Response.json(reply);
    }) as typeof fetch;
    try {
      await apiPlaybackResolver(request, new AbortController().signal);
      await apiPlaybackResolver(request, new AbortController().signal);
      expect(calls[0]!.path).toBe('/api/v1/profiles/owned-profile/playback');
      const payload = JSON.parse(calls[0]!.options.body as string);
      expect(payload).toMatchObject({
        type: 'tv',
        id: 9,
        season: 0,
        episode: 2,
        imdb_id: 'tt1234',
        mal_id: 42,
      });
      expect(payload.context_key).toBeUndefined();
      expect(calls[0]!.options.credentials).toBe('same-origin');
      expect(new Headers(calls[0]!.options.headers).get('Idempotency-Key')).toBe(
        new Headers(calls[1]!.options.headers).get('Idempotency-Key'),
      );
    } finally {
      globalThis.fetch = original;
    }
  }));
test('source changes use API playback identity and CAS, never the relay URL identity', () =>
  browser(async () => {
    const original = globalThis.fetch;
    let path = '',
      body: any;
    globalThis.fetch = (async (p: any, o: any) => {
      path = String(p);
      body = JSON.parse(o.body);
      return Response.json({ ...reply, revision: 9, source_generation: 4 });
    }) as typeof fetch;
    try {
      await changePlayback(
        request,
        { url: 'http://127.0.0.1:5173' + reply.url, session: reply as any },
        'backup',
        'stable-operation-key',
        new AbortController().signal,
      );
      expect(path).toBe('/api/v1/profiles/owned-profile/playback/api-playback-id/backup');
      expect(body).toEqual({ expected_revision: 8 });
    } finally {
      globalThis.fetch = original;
    }
  }));
test('response identity mismatch fails before exposing a media source', () =>
  browser(async () => {
    const original = globalThis.fetch;
    globalThis.fetch = (async () =>
      Response.json({
        ...reply,
        title: { type: 'tv', id: 9, season: 1, episode: 2 },
      })) as typeof fetch;
    try {
      await expect(apiPlaybackResolver(request, new AbortController().signal)).rejects.toThrow(
        'Invalid playback identity',
      );
    } finally {
      globalThis.fetch = original;
    }
  }));
test('final progress settles before session revocation can proceed', async () => {
  let finish!: () => void;
  const work = new Promise<void>((r) => (finish = r));
  trackProgress('playback', work);
  let settled = false;
  const closing = settleProgress('playback').then(() => (settled = true));
  await Promise.resolve();
  expect(settled).toBe(false);
  finish();
  await closing;
  expect(settled).toBe(true);
});
test('sign-out cleanup preserves reference-app browser storage', () => {
  const previousLocal = globalThis.localStorage,
    previousSession = globalThis.sessionStorage;
  const make = () => {
    const map = new Map([
      ['family-stream.navigation.v1', 'reference'],
      ['family-stream.web.v1.navigation', 'copied'],
    ]);
    return {
      get length() {
        return map.size;
      },
      key: (i: number) => [...map.keys()][i] ?? null,
      removeItem: (k: string) => map.delete(k),
      getItem: (k: string) => map.get(k) ?? null,
    };
  };
  const local = make(),
    session = make();
  Object.defineProperty(globalThis, 'localStorage', { configurable: true, value: local });
  Object.defineProperty(globalThis, 'sessionStorage', { configurable: true, value: session });
  try {
    clearWebSession();
    expect(local.getItem('family-stream.navigation.v1')).toBe('reference');
    expect(session.getItem('family-stream.navigation.v1')).toBe('reference');
    expect(local.getItem('family-stream.web.v1.navigation')).toBeNull();
  } finally {
    Object.defineProperty(globalThis, 'localStorage', {
      configurable: true,
      value: previousLocal,
    });
    Object.defineProperty(globalThis, 'sessionStorage', {
      configurable: true,
      value: previousSession,
    });
  }
});
