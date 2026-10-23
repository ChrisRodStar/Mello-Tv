import { z } from 'zod';

export class APIError extends Error {
  constructor(
    public readonly status: number,
    public readonly code: string,
  ) {
    super(
      messages[code] ??
        (status === 404
          ? 'This item is not cached yet. Try again shortly.'
          : 'Couldn’t complete that request. Try again.'),
    );
    this.name = 'APIError';
  }
}
const messages: Record<string, string> = {
  no_source: 'No playable source is available for this title.',
  playback_busy: 'Another playback request is finishing. Try again shortly.',
  source_refresh_required: 'No backup remains. Refresh the source to try again.',
  vpn_unavailable: 'Playback is temporarily unavailable. Try again shortly.',
  revision_conflict: 'Playback changed. Retry to recover its current state.',
  operation_pending: 'A playback change is still pending. Retry that action.',
  operation_superseded: 'Playback has moved on. Reload its current state.',
  session_expired: 'This playback session has expired. Start playback again.',
  revoked: 'This playback session has ended.',
  renew_too_early: 'This playback lease was just issued. Try extending it later.',
  invalid_request: 'Check the information and try again.',
};
export async function apiJSON<T>(
  path: string,
  schema: z.ZodType<T>,
  options: RequestInit = {},
): Promise<T> {
  if (!path.startsWith('/api/v1/')) throw new Error('Invalid API path');
  const headers = new Headers(options.headers);
  if (options.body !== undefined) headers.set('Content-Type', 'application/json');
  const response = await fetch(path, {
    ...options,
    headers,
    credentials: 'omit',
    redirect: 'error',
    signal: options.signal ?? AbortSignal.timeout(12000),
    cache: 'no-store',
  });
  if (!response.ok) {
    let code = 'unavailable';
    try {
      const value = z.object({ error: z.string() }).parse(await response.json());
      code = value.error;
    } catch {
      /* fixed public failure */
    }
    throw new APIError(response.status, code);
  }
  return schema.parse(response.status === 204 ? undefined : await response.json());
}
export const noContent = z.undefined();
export function profilePath(id: string) {
  return `/api/v1/profiles/${encodeURIComponent(id)}`;
}
export function requestID() {
  return crypto.randomUUID();
}
export function isMissing(error: unknown) {
  return error instanceof APIError && error.status === 404;
}
export function combineSignal(signal: AbortSignal, milliseconds = 12000) {
  return AbortSignal.any([signal, AbortSignal.timeout(milliseconds)]);
}
// Never persist capabilities, credentials or private upstream URLs in browser state.
export function publicArtwork(value?: string | null): string | null {
  if (!value) return null;
  const normalized = value.replace(
    /^\/(?:api|internal)\/v1\/artwork\//,
    'https://image.tmdb.org/t/p/',
  );
  return /^https:\/\/image\.tmdb\.org\/t\/p\/(?:w300|w500|w1280)\/[A-Za-z0-9_-]+\.(?:jpg|jpeg|png|webp)$/.test(
    normalized,
  )
    ? normalized
    : null;
}

export function streamURL(value: string) {
  const url = new URL(value, window.location.origin);
  if (
    url.origin !== window.location.origin ||
    !/^\/stream\/[A-Za-z0-9_-]+\/entry$/.test(url.pathname) ||
    [...url.searchParams.keys()].length !== 1 ||
    !url.searchParams.get('st') ||
    url.username ||
    url.password ||
    url.hash
  ) {
    throw new Error('Invalid playback source');
  }
  return url.href;
}
