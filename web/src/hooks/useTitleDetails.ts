import { loadDetails, loadEpisodes } from '../lib/catalog';
import { useEffect, useState } from 'react';

import {
  detailsSchema,
  episodesSchema,
  type Details,
  type Episode,
  type Title,
} from '../model';

export function useTitleDetails(title: Title) {
  const [details, setDetails] = useState<Details | null>(null);
  const [season, setSeason] = useState<number | null>(null);
  const [episodes, setEpisodes] = useState<Episode[] | null>(null);
  const [metadataError, setMetadataError] = useState('');
  const [episodeError, setEpisodeError] = useState('');
  const [retry, setRetry] = useState(0);
  useEffect(() => {
    const controller = new AbortController();
    setMetadataError('');
    setDetails(null);
    async function load() {
      try {
        const data = await loadDetails(
          title,
          AbortSignal.any([controller.signal, AbortSignal.timeout(30000)]),
        );
        if (controller.signal.aborted) return;
        setDetails(data);
        let savedSeason: number | null = null;
        try {
          const saved = sessionStorage.getItem(
            `family-stream.web.v1.season.${title.type}.${title.id}`,
          );
          if (saved !== null && data.seasons.some((s) => s.number === Number(saved)))
            savedSeason = Number(saved);
        } catch {
          /* Use the first available season without browser storage. */
        }
        setSeason((current) =>
          data.seasons.some((s) => s.number === current)
            ? current
            : (savedSeason ??
              data.seasons.find((s) => s.number > 0)?.number ??
              data.seasons[0]?.number ??
              null),
        );
      } catch {
        if (!controller.signal.aborted)
          setMetadataError('Details are not cached yet. Try again shortly.');
      }
    }
    void load();
    return () => controller.abort();
  }, [title.id, title.type, retry]);
  useEffect(() => {
    if (season === null) return;
    try {
      sessionStorage.setItem(
        `family-stream.web.v1.season.${title.type}.${title.id}`,
        String(season),
      );
    } catch {
      /* Season selection remains available without storage. */
    }
  }, [season, title.id, title.type]);
  useEffect(() => {
    if (season === null) return;
    const controller = new AbortController();
    setEpisodes(null);
    setEpisodeError('');
    async function load() {
      try {
        const data = await loadEpisodes(
          title.id,
          season!,
          AbortSignal.any([controller.signal, AbortSignal.timeout(30000)]),
        );
        if (!controller.signal.aborted) setEpisodes(data);
      } catch {
        if (!controller.signal.aborted)
          setEpisodeError('Episodes are not cached yet. Try again shortly.');
      }
    }
    void load();
    return () => controller.abort();
  }, [title.id, season, retry]);
  return {
    details,
    season,
    setSeason: (value: number) => {
      setEpisodes(null);
      setEpisodeError('');
      setSeason(value);
    },
    episodes,
    metadataError,
    episodeError,
    retry: () => setRetry((value) => value + 1),
  };
}
