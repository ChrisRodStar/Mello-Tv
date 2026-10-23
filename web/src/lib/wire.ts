import { z } from 'zod';
import { publicArtwork } from './api';
import type { Title, WatchProgress, Profile } from '../model';

// Public api.v1 DTOs. The copied components keep their original presentation
// models; only these explicit mappings translate snake_case and record envelopes.
export const identityWire = z.object({
  type: z.enum(['movie', 'tv']),
  id: z.number().int().positive(),
});
export const titleWire = identityWire.extend({
  title: z.string(),
  overview: z.string(),
  date: z.string().optional(),
  rating: z.number(),
  poster: z.string().optional(),
  backdrop: z.string().optional(),
  imdb_id: z.string().optional(),
  mal_id: z.number().int().positive().optional(),
  runtime: z.number().nonnegative().optional(),
  genres: z.array(z.string()).optional(),
  details: z.boolean(),
  seasons: z
    .array(
      z.object({
        season: z.number().int().nonnegative(),
        name: z.string(),
        episode_count: z.number().int().nonnegative(),
      }),
    )
    .optional(),
});
export const shelfWire = z.object({
  id: z.string(),
  heading: z.string(),
  titles: z.array(titleWire),
});
export const recommendationsWire = z.object({
  fingerprint: z.string(),
  pool: z.array(titleWire),
  sections: z.array(shelfWire),
});
export const recordWire = <T extends z.ZodType>(data: T) =>
  z.object({
    kind: z.string(),
    key: z.string(),
    data,
    state: z.enum(['fresh', 'stale']),
    revision: z.number().int().positive(),
    updated_at: z.iso.datetime({ offset: true }),
    expires_at: z.iso.datetime({ offset: true }),
  });
export const episodeWire = z.object({
  tmdb_episode_id: z.number().int(),
  season: z.number().int().nonnegative(),
  episode: z.number().int().positive(),
  name: z.string(),
  overview: z.string(),
  runtime: z.number().nonnegative(),
  still: z.string().optional(),
  air_date: z.string().optional(),
});
export const seasonWire = z.object({
  show_id: z.number().int().positive(),
  season: z.number().int().nonnegative(),
  name: z.string(),
  episodes: z.array(episodeWire),
});
export const preferencesWire = z.object({
  completed: z.boolean(),
  format: z.enum(['both', 'movie', 'tv']),
  genres: z.array(
    z.enum([
      'action',
      'comedy',
      'drama',
      'crime',
      'scifi',
      'animation',
      'documentary',
      'horror',
      'family',
    ]),
  ),
});
export const profileWire = z.object({
  id: z.string(),
  name: z.string(),
  color: z.string(),
  preferences: preferencesWire,
});
export const profilesWire = z.object({ profiles: z.array(profileWire) });
export const savedTitlesWire = z.object({ titles: z.array(titleWire) });
export const progressWire = identityWire.extend({
  season: z.number().int().nonnegative().optional(),
  episode: z.number().int().positive().optional(),
  position_seconds: z.number().nonnegative(),
  duration_seconds: z.number().nonnegative(),
  watched_seconds: z.number().nonnegative(),
  completed: z.boolean(),
  updated_at: z.iso.datetime({ offset: true }),
});
export const historyWire = z.object({
  records: z.array(progressWire),
  offset: z.number().int().nonnegative(),
  limit: z.number().int().positive(),
});
export const playbackWire = z.object({
  id: z.string(),
  title: identityWire.extend({
    season: z.number().int().nonnegative().optional(),
    episode: z.number().int().positive().optional(),
  }),
  url: z.string(),
  format: z.enum(['hls', 'mp4']),
  expires_at: z.iso.datetime({ offset: true }),
  revision: z.number().int().positive(),
  source_generation: z.number().int().positive(),
  reload_required: z.boolean(),
  resume_seconds: z.number().nonnegative(),
});
export type WirePlayback = z.infer<typeof playbackWire>;
export function viewTitle(t: z.infer<typeof titleWire>): Title {
  return {
    id: t.id,
    type: t.type,
    title: t.title,
    overview: t.overview,
    year: t.date?.slice(0, 4) ?? '',
    rating: t.rating,
    posterUrl: publicArtwork(t.poster),
    backdropUrl: publicArtwork(t.backdrop),
    imdbId: t.imdb_id,
    malId: t.mal_id,
  };
}
export function viewProfile(p: z.infer<typeof profileWire>, favorites: Title[] = []): Profile {
  return {
    ...p,
    preferences: {
      ...p.preferences,
      likedTitles: favorites.map((t) => ({ id: t.id, type: t.type, title: t.title })),
    },
  };
}
export function viewProgress(
  p: z.infer<typeof progressWire>,
  title: Title,
  episodeTitle = '',
): WatchProgress {
  return {
    title,
    season: p.season ?? 0,
    episode: p.episode ?? 0,
    episodeTitle,
    position: p.position_seconds,
    duration: p.duration_seconds,
    watchedSeconds: p.watched_seconds,
    completed: p.completed,
    updatedAt: Date.parse(p.updated_at),
  };
}
