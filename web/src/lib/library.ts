import { apiJSON, profilePath, combineSignal } from './api';
import { historyWire, savedTitlesWire, viewTitle, viewProgress } from './wire';
import { rememberTitle, enrichTitle } from './catalog';
import { mapLimited } from './profile-api';
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
