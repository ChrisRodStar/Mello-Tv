import type { Title } from '../model';
import { apiJSON, noContent, profilePath, requestID, streamURL } from './api';
import { playbackWire, type WirePlayback } from './wire';

export type PlaybackRequest = {
  titleId: number;
  title: string;
  profileId: string;
  catalogTitle?: Title;
  startAt?: number;
} & ({ type: 'movie' } | { type: 'tv'; season: number; episode: number; episodeTitle: string });
export type PlaybackSource = { url: string; mimeType?: string; session: WirePlayback };
export type PlaybackResolver = (
  request: PlaybackRequest,
  signal: AbortSignal,
) => Promise<PlaybackSource | null>;
const createKeys = new WeakMap<PlaybackRequest, string>();
export function resetPlaybackRequest(request: PlaybackRequest) {
  createKeys.delete(request);
}
function sourceFor(request: PlaybackRequest, reply: WirePlayback): PlaybackSource {
  if (
    reply.title.id !== request.titleId ||
    reply.title.type !== request.type ||
    (request.type === 'tv' &&
      (reply.title.season !== request.season || reply.title.episode !== request.episode))
  )
    throw new Error('Invalid playback identity');
  if (request.startAt !== undefined) reply = { ...reply, resume_seconds: request.startAt };
  return {
    url: streamURL(reply.url),
    mimeType: reply.format === 'hls' ? 'application/vnd.apple.mpegurl' : 'video/mp4',
    session: reply,
  };
}
export async function resolvePlaybackSource(
  request: PlaybackRequest,
  signal: AbortSignal,
  resolver?: PlaybackResolver,
): Promise<PlaybackSource | null> {
  if (!resolver) return null;
  const source = await resolver(request, signal);
  if (!source) return null;
  signal.throwIfAborted();
  return { ...source, url: streamURL(source.url) };
}
export const apiPlaybackResolver: PlaybackResolver = async (request, signal) => {
  let key = createKeys.get(request);
  if (!key) {
    key = requestID();
    createKeys.set(request, key);
  }
  const nativeHLS =
    typeof document !== 'undefined' &&
    !!document.createElement('video').canPlayType('application/vnd.apple.mpegurl');
  const reply = await apiJSON(`${profilePath(request.profileId)}/playback`, playbackWire, {
    method: 'POST',
    signal,
    headers: { 'Idempotency-Key': key },
    body: JSON.stringify({
      type: request.type,
      id: request.titleId,
      ...(request.type === 'tv' ? { season: request.season, episode: request.episode } : {}),
      ...(request.catalogTitle?.imdbId ? { imdb_id: request.catalogTitle.imdbId } : {}),
      ...(request.catalogTitle?.malId ? { mal_id: request.catalogTitle.malId } : {}),
      device: {
        formats: ['hls', 'mp4'],
        codecs: ['avc1', 'mp4a'],
        supports_encrypted_hls: nativeHLS || typeof MediaSource !== 'undefined',
        allow_unknown_codecs: false,
        allow_unknown_resolution: false,
      },
    }),
  });
  return sourceFor(request, reply);
};
export async function recoverPlayback(
  request: PlaybackRequest,
  id: string,
  signal: AbortSignal,
) {
  return sourceFor(
    request,
    await apiJSON(
      `${profilePath(request.profileId)}/playback/${encodeURIComponent(id)}`,
      playbackWire,
      { signal },
    ),
  );
}
export async function changePlayback(
  request: PlaybackRequest,
  source: PlaybackSource,
  kind: 'backup' | 'refresh' | 'renew',
  key: string,
  signal: AbortSignal,
) {
  const reply = await apiJSON(
    `${profilePath(request.profileId)}/playback/${encodeURIComponent(source.session.id)}/${kind}`,
    playbackWire,
    {
      method: 'POST',
      signal,
      headers: { 'Idempotency-Key': key },
      body: JSON.stringify({ expected_revision: source.session.revision }),
    },
  );
  return sourceFor(request, reply);
}
export async function revokePlayback(request: PlaybackRequest, id: string) {
  await apiJSON(
    `${profilePath(request.profileId)}/playback/${encodeURIComponent(id)}`,
    noContent,
    { method: 'DELETE', keepalive: true, signal: AbortSignal.timeout(12000) },
  );
}
