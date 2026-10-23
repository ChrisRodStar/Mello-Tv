import { useCallback, useEffect, useRef, useState } from 'react';
import type { ProfileLibrary, Title, WatchProgress } from '../model';
import { apiJSON, noContent, profilePath, combineSignal } from '../lib/api';
import { loadLibrary, loadHistory } from '../lib/library';

export function useProfileLibrary(id: string) {
  const [state, setState] = useState<
    ProfileLibrary & { id: string; ready: boolean; hasMore: boolean; nextOffset: number }
  >({ id, list: [], progress: [], ready: false, hasMore: false, nextOffset: 0 });
  const [error, setError] = useState('');
  const [saving, setSaving] = useState(false);
  const busy = useRef(false);
  const revision = useRef(0);
  const pending = useRef<AbortController | null>(null);
  const refresh = useCallback(
    async (signal?: AbortSignal) => {
      pending.current?.abort();
      const controller = new AbortController();
      pending.current = controller;
      const token = ++revision.current;
      try {
        const result = await loadLibrary(
          id,
          combineSignal(
            signal ? AbortSignal.any([signal, controller.signal]) : controller.signal,
          ),
        );
        if (!controller.signal.aborted && token === revision.current) {
          setState({ id, ...result, ready: true });
          setError('');
        }
      } catch {
        if (!controller.signal.aborted && token === revision.current)
          setError('Couldn’t load your library. Try again.');
      }
    },
    [id],
  );
  useEffect(() => {
    const controller = new AbortController();
    void refresh(controller.signal);
    const update = () => {
      if (document.visibilityState === 'visible') void refresh(controller.signal);
    };
    const progress = (event: Event) => {
      const detail = (event as CustomEvent<{ profileId: string; progress: WatchProgress }>)
        .detail;
      if (detail?.profileId !== id) return;
      setState((previous) =>
        previous.id !== id
          ? previous
          : {
              ...previous,
              progress: [
                detail.progress,
                ...previous.progress.filter(
                  (p) =>
                    !(
                      p.title.type === detail.progress.title.type &&
                      p.title.id === detail.progress.title.id &&
                      p.season === detail.progress.season &&
                      p.episode === detail.progress.episode
                    ),
                ),
              ],
            },
      );
    };
    window.addEventListener('family-stream-library', update);
    window.addEventListener('focus', update);
    window.addEventListener('family-stream-progress', progress);
    return () => {
      controller.abort();
      pending.current?.abort();
      revision.current++;
      window.removeEventListener('family-stream-library', update);
      window.removeEventListener('focus', update);
      window.removeEventListener('family-stream-progress', progress);
    };
  }, [id, refresh]);
  async function toggle(title: Title) {
    if (busy.current || state.id !== id || !state.ready) return;
    busy.current = true;
    setSaving(true);
    setError('');
    const inList = state.list.some((t) => t.type === title.type && t.id === title.id);
    try {
      await apiJSON(`${profilePath(id)}/library/${title.type}/${title.id}`, noContent, {
        method: inList ? 'DELETE' : 'PUT',
      });
      setState((p) =>
        p.id !== id
          ? p
          : {
              ...p,
              list: inList
                ? p.list.filter((t) => t.type !== title.type || t.id !== title.id)
                : [...p.list, title],
            },
      );
      window.dispatchEvent(new Event('family-stream-library'));
    } catch {
      setError('Couldn’t update My List. Try again.');
    } finally {
      busy.current = false;
      setSaving(false);
    }
  }
  async function loadMore() {
    if (busy.current || state.id !== id || !state.hasMore) return;
    busy.current = true;
    setSaving(true);
    const controller = new AbortController();
    pending.current = controller;
    const token = ++revision.current;
    try {
      const next = await loadHistory(id, state.nextOffset, combineSignal(controller.signal));
      if (token === revision.current && !controller.signal.aborted)
        setState((p) =>
          p.id !== id
            ? p
            : {
                ...p,
                progress: [
                  ...p.progress,
                  ...next.progress.filter(
                    (n) =>
                      !p.progress.some(
                        (v) =>
                          v.title.type === n.title.type &&
                          v.title.id === n.title.id &&
                          v.season === n.season &&
                          v.episode === n.episode,
                      ),
                  ),
                ],
                hasMore: next.hasMore,
                nextOffset: next.nextOffset,
              },
        );
    } catch {
      if (!controller.signal.aborted) setError('Couldn’t load more history. Try again.');
    } finally {
      busy.current = false;
      if (!controller.signal.aborted) setSaving(false);
    }
  }
  const current =
    state.id === id ? state : { list: [], progress: [], ready: false, hasMore: false };
  return { ...current, error, saving, refresh, toggle, loadMore };
}
