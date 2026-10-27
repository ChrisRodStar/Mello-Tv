import { loadCatalog } from '../lib/catalog';
import { apiJSON } from '../lib/api';
import { z } from 'zod';
import { useEffect, useRef, useState } from 'react';

import {
  catalogSchema,
  serviceHealthSchema,
  type Catalog,
  type Profile,
  type ServiceHealth,
} from '../model';

import type { Category } from '../types';

export function useCatalog(category: Category = 'all', profile?: Profile) {
  const enabled = !!profile?.preferences.completed;
  const profileId = profile?.preferences.completed ? profile.id : '';
  const key = `${category}:${profileId}:${JSON.stringify(profile?.preferences)}`;
  const cache = useRef(new Map<string, { expires: number; data: Catalog }>());
  const [result, setResult] = useState<{
    key: string;
    data: Catalog;
  } | null>(null);
  const [failure, setFailure] = useState<{ key: string; message: string } | null>(null);
  const error = enabled && failure?.key === key ? failure.message : '';
  const [health, setHealth] = useState<ServiceHealth | null>(null);
  const [checking, setChecking] = useState(false);
  const [attempt, setAttempt] = useState(0);

  useEffect(() => {
    if (!enabled) {
      setChecking(false);
      return;
    }
    const controller = new AbortController();
    async function load() {
      const cached = cache.current.get(key);
      if (cached && cached.expires > Date.now()) {
        setResult({ key, data: cached.data });
        setFailure(null);
        setChecking(false);
        return;
      }
      setChecking(true);
      setHealth(null);
      try {
        const data = await loadCatalog(
          profileId,
          category,
          AbortSignal.any([controller.signal, AbortSignal.timeout(120000)]),
        );
        if (controller.signal.aborted) return;
        cache.current.set(key, { expires: Date.now() + 30_000, data });
        setResult({ key, data });
        setFailure(null);
        setHealth(null);
      } catch (cause) {
        if (controller.signal.aborted) return;
        setFailure({
          key,
          message: cause instanceof Error ? cause.message : 'Catalog unavailable. Try again.',
        });
        setHealth(null);
        try {
          const result = await apiJSON(
            '/api/v1/status',
            z.object({
              version: z.literal('api.v1'),
              catalog_snapshot: z.enum(['missing', 'retained']),
            }),
            { signal: AbortSignal.any([controller.signal, AbortSignal.timeout(7000)]) },
          );
          if (!controller.signal.aborted)
            setHealth({ api: 'ok', catalog: result.catalog_snapshot });
        } catch {
          // An unreachable API cannot tell us whether TMDB is available.
        }
      } finally {
        if (!controller.signal.aborted) setChecking(false);
      }
    }
    void load();
    return () => controller.abort();
  }, [attempt, category, key, profileId, enabled]);

  useEffect(() => {
    if (!enabled) return;
    const refresh = () => {
      if (document.visibilityState === 'visible') setAttempt((value) => value + 1);
    };
    const timer = setInterval(refresh, 5 * 60_000);
    window.addEventListener('focus', refresh);
    document.addEventListener('visibilitychange', refresh);
    return () => {
      clearInterval(timer);
      window.removeEventListener('focus', refresh);
      document.removeEventListener('visibilitychange', refresh);
    };
  }, [enabled]);

  useEffect(() => {
    if (!error || checking) return;
    const timer = setTimeout(() => setAttempt((value) => value + 1), 30000);
    return () => clearTimeout(timer);
  }, [error, checking]);

  return {
    catalog: enabled && result?.key === key ? result.data : null,
    error,
    health,
    checking,
    retry: () => {
      cache.current.delete(key);
      setAttempt((value) => value + 1);
    },
  };
}
