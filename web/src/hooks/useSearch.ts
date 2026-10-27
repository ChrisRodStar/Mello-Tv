import { apiJSON, isMissing, isPending, APIError } from '../lib/api';
import { recordWire, titleWire, viewTitle } from '../lib/wire';
import { rememberTitle } from '../lib/catalog';
import { z } from 'zod';
import { startTransition, useEffect, useState } from 'react';

import { searchResultsSchema, type SearchResults } from '../model';

// Decode the first visible posters before swapping grids, with a bounded wait.
async function preparePosters(data: SearchResults) {
  let timer: ReturnType<typeof setTimeout>;
  await Promise.race([
    Promise.all(
      data.titles.slice(0, 8).map(async (title) => {
        if (!title.posterUrl) return;
        const image = new Image();
        image.src = title.posterUrl;
        try {
          await image.decode();
        } catch {
          /* Artwork failure must not block search. */
        }
      }),
    ),
    new Promise<void>((resolve) => {
      timer = setTimeout(resolve, 1200);
    }),
  ]);
  clearTimeout(timer!);
}

type SearchData = SearchResults & { query: string; pageSize: number };
type Request = { query: string; page: number; attempt: number; pageSize: number };

export function useSearch(query: string) {
  const [data, setData] = useState<SearchData | null>(null);
  const [request, setRequest] = useState<Request>({
    query: '',
    page: 1,
    attempt: 0,
    pageSize: 20,
  });
  const [pending, setPending] = useState(false);
  const [failure, setFailure] = useState<{
    query: string;
    page: number;
  } | null>(null);

  useEffect(() => {
    if (!query) {
      setData(null);
      setFailure(null);
      setRequest({ query: '', page: 1, attempt: 0, pageSize: 20 });
      return;
    }
    const timer = setTimeout(() => setRequest({ query, page: 1, attempt: 0, pageSize: 20 }), 350);
    return () => clearTimeout(timer);
  }, [query]);

  useEffect(() => {
    // A keystroke cancels the old request immediately, before the debounce ends.
    if (!query || request.query !== query) return;
    const controller = new AbortController();
    let retryTimer: ReturnType<typeof setTimeout> | undefined;
    setPending(true);
    setFailure(null);
    async function load() {
      try {
        const offset = (request.page - 1) * request.pageSize;
        const params = new URLSearchParams({ q: request.query, offset: String(offset) });
        const signal = AbortSignal.any([controller.signal, AbortSignal.timeout(12000)]);
        const response = await apiJSON(
          `/api/v1/search?${params}`,
          z.object({
            scope: z.literal('cached_titles'),
            records: z.array(recordWire(titleWire)),
            offset: z.number(),
            limit: z.number(),
            status: z.enum(['pending', 'ready', 'unavailable', 'failed']).optional(),
            reason: z.string().nullish(),
          }),
          { signal },
        );
        if (response.status === 'pending') throw new APIError(202, 'pending');
        const found = response.records.map((r) => rememberTitle(viewTitle(r.data)));
        // The discovery endpoint is an independently refreshed first-page cache,
        // not a made-up upstream pagination API. Merge only on page one.
        if (request.page === 1) {
          try {
            const discovery = await apiJSON(
              `/api/v1/search-results?q=${encodeURIComponent(request.query)}`,
              recordWire(z.array(titleWire)).extend({
                status: z.enum(['pending', 'ready', 'unavailable', 'failed']).optional(),
                reason: z.string().nullish(),
              }),
              { signal },
            );
            if (discovery.status === 'pending') throw new APIError(202, 'pending');
            found.push(...discovery.data.map((t) => rememberTitle(viewTitle(t))));
          } catch (error) {
            if (isPending(error)) throw error;
            if (!isMissing(error)) {
              signal.throwIfAborted();
              if (!found.length) throw error;
            }
          }
        }
        const next = {
          titles: found,
          page: request.page,
          pageSize: response.limit,
          totalPages:
            response.records.length === response.limit && offset < 10000 ? request.page + 1 : request.page,
        };
        if (controller.signal.aborted) return;
        await preparePosters(next);
        if (controller.signal.aborted) return;
        startTransition(() => {
          setData((previous) => {
            const merged =
              request.page === 1 || previous?.query !== query
                ? next.titles
                : [...previous.titles, ...next.titles];
            const seen = new Set<string>();
            const titles = merged.filter((title) => {
              const key = `${title.type}-${title.id}`;
              if (seen.has(key)) return false;
              seen.add(key);
              return true;
            });
            return { ...next, titles, query };
          });
          setPending(false);
        });
      } catch (cause) {
        if (!controller.signal.aborted && isPending(cause) && request.attempt < 6) {
          const delays = [1500, 3000, 5000, 8000, 12000, 12000];
          retryTimer = setTimeout(() => {
            if (!controller.signal.aborted)
              setRequest({ ...request, attempt: request.attempt + 1 });
          }, delays[request.attempt]!);
        } else if (!controller.signal.aborted) {
          setFailure({ query, page: request.page });
          setPending(false);
        }
      }
    }
    void load();
    return () => {
      controller.abort();
      clearTimeout(retryTimer);
    };
  }, [query, request]);

  const error = failure?.query === query ? (failure.page > 1 ? 'more' : 'search') : null;
  const loading = Boolean(query) && !error && (pending || data?.query !== query);
  return {
    titles: query && data?.query === query ? data.titles : [],
    loading,
    hasLoaded: Boolean(query && data?.query === query),
    loadingMore: loading && request.query === query && request.page > 1,
    error,
    hasMore: Boolean(data && data.page < data.totalPages),
    loadMore: () => {
      if (!loading && data?.query === query)
        setRequest({ query, page: data.page + 1, attempt: 0, pageSize: data.pageSize });
    },
    retry: () =>
      setRequest((value) => ({
        query,
        page: error === 'more' ? value.page : 1,
        attempt: value.attempt + 1,
        pageSize: error === 'more' ? value.pageSize : 20,
      })),
  };
}
