import { expect, test } from 'bun:test';
import { awaitMetadata } from '../src/lib/await-metadata';
import { APIError } from '../src/lib/api';

test('a cold season is retried until the requested metadata arrives', async () => {
  let calls = 0;
  const season = await awaitMetadata(
    async () => {
      if (++calls === 1) throw new APIError(404, 'missing');
      return { season: 1, episodes: [1] };
    },
    () => true,
    new AbortController().signal,
  );
  expect(calls).toBe(2);
  expect(season.episodes).toEqual([1]);
});

test('cancellation stops pending metadata reads', async () => {
  const controller = new AbortController();
  let calls = 0;
  const pending = awaitMetadata(
    async () => {
      calls++;
      controller.abort(new Error('dialog closed'));
      return { details: false };
    },
    (value) => value.details,
    controller.signal,
  );
  await expect(pending).rejects.toThrow('dialog closed');
  expect(calls).toBe(1);
});

test('authentication and invalid response errors are not hidden by metadata retries', async () => {
  let calls = 0;
  const originalError = new APIError(401, 'unauthorized');
  let caught: unknown;
  try {
    await awaitMetadata(
      async () => {
        calls++;
        throw originalError;
      },
      () => true,
      new AbortController().signal,
    );
  } catch (error) {
    caught = error;
  }
  expect(caught).toBe(originalError);
  expect(caught).toBeInstanceOf(APIError);
  expect((caught as APIError).status).toBe(401);
  expect((caught as APIError).code).toBe('unauthorized');
  expect(calls).toBe(1);
});
