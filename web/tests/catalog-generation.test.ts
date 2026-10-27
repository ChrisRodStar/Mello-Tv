import { afterEach, describe, expect, test } from 'bun:test';
import { apiJSON } from '../src/lib/api';
import { loadBrowse, loadCatalog, waitForCatalogReady } from '../src/lib/catalog';
import { awaitMetadata } from '../src/lib/await-metadata';
import { z } from 'zod';

const originalFetch = globalThis.fetch;
const response = (status: number, body: unknown) =>
  new Response(JSON.stringify(body), { status, headers: { 'Content-Type': 'application/json' } });

afterEach(() => {
  globalThis.fetch = originalFetch;
});

describe('catalog readiness', () => {
  test('waits for pending profile generation before returning ready', async () => {
    let reads = 0;
    globalThis.fetch = async () => response(200, {
      status: ++reads === 1 ? 'pending' : 'ready', updated_at: null, reason: null,
    });
    await expect(waitForCatalogReady('profile-test', AbortSignal.timeout(5000))).resolves.toBeUndefined();
    expect(reads).toBe(2);
  });

  test('surfaces an unconfigured catalog reason', async () => {
    globalThis.fetch = async () => response(200, {
      status: 'unavailable', updated_at: null, reason: 'Configure TMDB in Mac Settings.',
    });
    await expect(waitForCatalogReady('profile-test', AbortSignal.timeout(5000)))
      .rejects.toThrow('Configure TMDB in Mac Settings.');
  });

  test('honors cancellation before starting a status request', async () => {
    const controller = new AbortController();
    controller.abort();
    globalThis.fetch = async () => { throw new Error('must not fetch'); };
    await expect(waitForCatalogReady('profile-test', controller.signal)).rejects.toBeDefined();
  });

  test('accepts the browse status contract with a null reason', async () => {
    globalThis.fetch = async () => response(200, { records: [], status: 'ready', reason: null });
    await expect(loadBrowse(AbortSignal.timeout(5000))).resolves.toMatchObject({ status: 'ready', records: [] });
  });

  test('loads personalized shelves from the full recommendations record envelope', async () => {
    const title = {
      id: 9101,
      type: 'movie',
      title: 'Personalized Pick',
      overview: 'A profile-specific catalog recommendation.',
      date: '2025-01-01',
      rating: 8.1,
      details: false,
    };
    globalThis.fetch = async (input) => {
      const path = String(input);
      if (path.endsWith('/catalog-status'))
        return response(200, { status: 'ready', updated_at: null, reason: null });
      if (path === '/api/v1/browse')
        return response(200, { records: [], status: 'ready', reason: null });
      if (path.includes('/recommendations?type=all'))
        return response(200, {
          kind: 'recommendations',
          key: 'profile-test:all',
          data: {
            fingerprint: 'profile-fingerprint',
            pool: [title],
            sections: [{ id: 'personalized-for-you', heading: 'Picked for You', titles: [title] }],
          },
          state: 'fresh',
          revision: 1,
          updated_at: '2026-10-27T00:00:00Z',
          expires_at: '2026-10-28T00:00:00Z',
          status: 'ready',
          reason: null,
        });
      throw new Error(`Unexpected catalog request: ${path}`);
    };

    const catalog = await loadCatalog('profile-test', 'all', AbortSignal.timeout(5000));
    expect(catalog.sections).toContainEqual(expect.objectContaining({
      id: 'personalized-for-you',
      heading: 'Picked for You',
      titles: [expect.objectContaining({ id: 9101, title: 'Personalized Pick' })],
    }));
  });

  test('retries HTTP 202 metadata responses until the saved record is ready', async () => {
    let reads = 0;
    globalThis.fetch = async () => ++reads === 1
      ? response(202, { status: 'pending', key: 'movie:42' })
      : response(200, { data: { id: 42 } });
    const signal = AbortSignal.timeout(5000);
    const read = () => apiJSON('/api/v1/titles/movie/42', z.object({ data: z.object({ id: z.number() }) }), { signal });
    const result = await awaitMetadata(read, () => true, signal);
    expect(result.data.id).toBe(42);
    expect(reads).toBe(2);
  });

  test('retries HTTP 200 metadata responses marked pending', async () => {
    let reads = 0;
    globalThis.fetch = async () => ++reads === 1
      ? response(200, { status: 'pending', key: 'movie:42' })
      : response(200, { data: { id: 42 } });
    const signal = AbortSignal.timeout(5000);
    const read = () => apiJSON('/api/v1/titles/movie/42', z.object({ data: z.object({ id: z.number() }) }), { signal });
    const result = await awaitMetadata(read, () => true, signal);
    expect(result.data.id).toBe(42);
    expect(reads).toBe(2);
  });
});
