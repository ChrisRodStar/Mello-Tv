import { z } from 'zod';

export const profileColors = [
  '#446e88',
  '#916050',
  '#616b4b',
  '#76618e',
  '#93773c',
  '#536e70',
] as const;
export const profileIdSchema = z
  .string()
  .min(1)
  .max(128)
  .regex(/^[a-zA-Z0-9_-]+$/);
export const preferenceGenres = [
  { id: 'action', name: 'Action & adventure', movie: '28|12', tv: 10759 },
  { id: 'comedy', name: 'Comedy', movie: 35, tv: 35 },
  { id: 'drama', name: 'Drama', movie: 18, tv: 18 },
  { id: 'crime', name: 'Crime & mystery', movie: '80|9648', tv: '80|9648' },
  { id: 'scifi', name: 'Sci-fi & fantasy', movie: '878|14', tv: 10765 },
  { id: 'animation', name: 'Anime', movie: 16, tv: 16 },
  { id: 'documentary', name: 'Documentaries', movie: 99, tv: 99 },
  { id: 'horror', name: 'Horror', movie: 27, tv: null },
] as const;
export const profilePreferencesSchema = z.object({
  genres: z
    .array(
      z.enum([
        'action',
        'comedy',
        'drama',
        'crime',
        'scifi',
        'animation',
        'documentary',
        'horror',
        'family', // Accept previously saved preferences; Family is no longer offered.
      ]),
    )
    .refine((genres) => new Set(genres).size === genres.length),
  format: z.enum(['both', 'movie', 'tv']),
  likedTitles: z
    .array(
      z.object({
        id: z.number().int().positive(),
        type: z.enum(['movie', 'tv']),
        title: z.string().min(1).max(300),
      }),
    )
    .refine(
      (titles) =>
        new Set(titles.map((title) => `${title.type}:${title.id}`)).size === titles.length,
    )
    .default([]),
  completed: z.boolean(),
});
export const emptyPreferences = {
  genres: [],
  format: 'both',
  likedTitles: [],
  completed: false,
} satisfies z.infer<typeof profilePreferencesSchema>;
export type ProfilePreferences = z.infer<typeof profilePreferencesSchema>;
export const profileSchema = z.object({
  id: profileIdSchema,
  name: z.string().trim().min(1).max(24),
  color: z.string().min(1).max(32),
  preferences: profilePreferencesSchema.default(emptyPreferences),
});
export const profileListSchema = z.array(profileSchema).max(128);
export type Profile = z.infer<typeof profileSchema>;

export const titleSchema = z.object({
  id: z.number().int().positive(),
  type: z.enum(['movie', 'tv']),
  title: z.string(),
  overview: z.string(),
  year: z.string(),
  rating: z.number(),
  backdropUrl: z.string().nullable().optional(),
  posterUrl: z.string().nullable(),
  imdbId: z.string().optional(),
  malId: z.number().int().positive().optional(),
});

export const catalogSectionSchema = z.object({
  id: z.string(),
  heading: z.string(),
  titles: z.array(titleSchema),
});

export const catalogSchema = z.object({
  configured: z.boolean(),
  titles: z.array(titleSchema),
  sections: z.array(catalogSectionSchema).optional(),
});

export type Title = z.infer<typeof titleSchema>;
export type Catalog = z.infer<typeof catalogSchema>;

export const detailsSchema = z.object({
  runtime: z.number().nullable(),
  genres: z.array(z.string()),
  seasons: z.array(
    z.object({
      number: z.number(),
      name: z.string(),
      episodeCount: z.number(),
    }),
  ),
});
export const episodesSchema = z.array(
  z.object({
    number: z.number(),
    name: z.string(),
    overview: z.string(),
    runtime: z.number().nullable(),
    stillUrl: z.string().nullable(),
  }),
);
export type Details = z.infer<typeof detailsSchema>;
export type Episode = z.infer<typeof episodesSchema>[number];

export const serviceHealthSchema = z.object({
  api: z.literal('ok'),
  catalog: z.enum(['retained', 'missing']),
});
export type ServiceHealth = z.infer<typeof serviceHealthSchema>;

export const searchResultsSchema = z.object({
  titles: z.array(titleSchema),
  page: z.number().int().positive(),
  totalPages: z.number().int().nonnegative(),
});
export type SearchResults = z.infer<typeof searchResultsSchema>;

export const playbackSourceSchema = z.object({
  url: z.string().regex(/^\/stream\/[A-Za-z0-9_-]+\/entry\?st=/),
  mimeType: z.enum(['application/vnd.apple.mpegurl', 'video/mp4']).optional(),
});
const selectionBase = {
  titleId: z.number().int().positive(),
  provider: z
    .string()
    .regex(/^[a-z0-9-]+$/)
    .optional(),
};
export const playbackSelectionSchema = z.discriminatedUnion('type', [
  z.object({ ...selectionBase, type: z.literal('movie') }),
  z.object({
    ...selectionBase,
    type: z.literal('tv'),
    season: z.number().int().nonnegative(),
    episode: z.number().int().positive(),
  }),
]);
export type PlaybackSelection = z.infer<typeof playbackSelectionSchema>;
export const playbackResolutionSchema = z.discriminatedUnion('status', [
  z.object({
    status: z.literal('ready'),
    provider: z.object({ id: z.string(), name: z.string() }),
    source: playbackSourceSchema,
  }),
  z.object({
    status: z.literal('unavailable'),
    reason: z.enum(['unconfigured', 'no_source']),
  }),
]);

export const watchProgressSchema = z.object({
  title: titleSchema,
  season: z.number().int().nonnegative().default(0),
  episode: z.number().int().nonnegative().default(0),
  episodeTitle: z.string().max(300).default(''),
  position: z.number().finite().nonnegative(),
  duration: z.number().finite().nonnegative(),
  watchedSeconds: z.number().finite().nonnegative(),
  completed: z.boolean(),
  updatedAt: z.number(),
});
export type WatchProgress = z.infer<typeof watchProgressSchema>;
export const watchUpdateSchema = watchProgressSchema
  .omit({ completed: true, updatedAt: true })
  .extend({
    sessionId: profileIdSchema,
  })
  .refine((value) =>
    value.title.type === 'movie'
      ? value.season === 0 && value.episode === 0
      : value.episode > 0,
  );
export const profileLibrarySchema = z.object({
  list: z.array(titleSchema),
  progress: z.array(watchProgressSchema),
});
export type ProfileLibrary = z.infer<typeof profileLibrarySchema>;
