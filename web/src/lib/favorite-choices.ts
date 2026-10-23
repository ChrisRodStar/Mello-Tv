import type { ProfilePreferences, Title } from '../model';

type Preferences = Pick<ProfilePreferences, 'genres' | 'format'>;
type Shelf = { id: string; titles: Title[] };

export function selectFavoriteChoices(shelves: Shelf[], preferences: Preferences): Title[] {
  const buckets = preferences.genres.map((genre, genreIndex) => {
    const types: Title['type'][] =
      preferences.format === 'both'
        ? genreIndex % 2
          ? ['tv', 'movie']
          : ['movie', 'tv']
        : [preferences.format];
    const lists = types.map((type) => {
      const qualified = shelves.find((shelf) => shelf.id === `favorites-${type}-${genre}`);
      const fallback =
        type === 'movie'
          ? shelves.find((shelf) => shelf.id === `onboarding-${genre}`)
          : undefined;
      return (
        (qualified ?? fallback)?.titles.filter(
          (title) => title.type === type && title.posterUrl,
        ) ?? []
      );
    });
    return Array.from(
      { length: Math.max(0, ...lists.map((list) => list.length)) },
      (_, index) => lists.flatMap((list) => (list[index] ? [list[index]!] : [])),
    ).flat();
  });
  const positions = buckets.map(() => 0);
  const used = new Set<string>();
  const choices: Title[] = [];
  while (choices.length < 48) {
    let added = false;
    for (let index = 0; index < buckets.length && choices.length < 48; index++) {
      const bucket = buckets[index]!;
      while (positions[index]! < bucket.length) {
        const title = bucket[positions[index]!]!;
        positions[index]!++;
        const key = `${title.type}:${title.id}`;
        if (used.has(key)) continue;
        used.add(key);
        choices.push(title);
        added = true;
        break;
      }
    }
    if (!added) break;
  }
  return choices;
}
