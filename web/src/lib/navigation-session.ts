import { titleSchema, type Title } from '../model';

import type { Category } from '../types';

const key = 'family-stream.web.v1.navigation';
export type NavigationSession = {
  filter: Category;
  profilesOpen: boolean;
  searchOpen: boolean;
  query: string;
  selected: Title | null;
};

export function readNavigationSession(): NavigationSession {
  const fallback: NavigationSession = {
    filter: 'all',
    profilesOpen: false,
    searchOpen: false,
    query: '',
    selected: null,
  };
  try {
    const saved = sessionStorage.getItem(key);
    if (!saved) return fallback;
    const value = JSON.parse(saved);
    if (
      !value ||
      !['all', 'movie', 'tv'].includes(value.filter) ||
      typeof value.profilesOpen !== 'boolean' ||
      typeof value.searchOpen !== 'boolean' ||
      typeof value.query !== 'string'
    )
      return fallback;
    const title = titleSchema.nullable().safeParse(value.selected);
    if (!title.success) return fallback;
    return {
      filter: value.filter,
      profilesOpen: value.profilesOpen,
      searchOpen: value.searchOpen,
      query: value.query,
      selected: title.data,
    };
  } catch {
    return fallback;
  }
}

export function saveNavigationSession(session: NavigationSession) {
  try {
    sessionStorage.setItem(key, JSON.stringify(session));
  } catch {
    // Navigation remains usable when browser storage is unavailable.
  }
}
