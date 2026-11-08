import { apiJSON, profilePath, combineSignal } from './api';
import { historyWire, savedTitlesWire, viewTitle, viewProgress } from './wire';
import { rememberTitle, enrichTitle } from './catalog';
import { mapLimited } from './profile-api';
import type { WatchProgress } from '../model';
import type { Category } from '../types';

export type ContinueWatchingAction = 'remove' | 'watched';

export function continueWatchingItems(progress: WatchProgress[], filter: Category = 'all') {
  const seen = new Set<string>();
  return progress.filter((item) => {
    if (item.continueWatchingRemovedAt !== undefined) return false;
    const key = `${item.title.type}:${item.title.id}`;
    if (seen.has(key)) return false;
    seen.add(key);
    return (
      (filter === 'all' || item.title.type === filter) &&
      (item.completed ? item.title.type === 'tv' : item.position >= 10)
    );
  });
}

export async function loadHistory(pid: string, offset: number, signal: AbortSignal) {
  const history = await apiJSON(`${profilePath(pid)}/history?offset=${offset}`, historyWire, {
    signal,
  });
  const progress = await mapLimited(history.records, async (p) =>
    viewProgress(p, await enrichTitle(p.type, p.id, signal)),
  );
  return {
    progress,
    hasMore: history.records.length === history.limit,
    nextOffset: history.offset + history.records.length,
  };
}
export async function loadLibrary(pid: string, signal: AbortSignal) {
  const [saved, history] = await Promise.all([
    apiJSON(`${profilePath(pid)}/library`, savedTitlesWire, { signal }),
    loadHistory(pid, 0, signal),
  ]);
  return { list: saved.titles.map((t) => rememberTitle(viewTitle(t))), ...history };
}
