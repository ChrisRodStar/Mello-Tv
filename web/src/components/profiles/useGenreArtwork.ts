import { useEffect, useState } from 'react';
import { loadBrowse } from '../../lib/catalog';
import { loadGenreArtwork, type GenreArtwork } from '../../lib/genre-artwork';

export function useGenreArtwork() {
  const [artwork, setArtwork] = useState<GenreArtwork>({});
  useEffect(() => {
    const controller = new AbortController();
    let retry: ReturnType<typeof setTimeout> | undefined;
    async function load(attempt: number) {
      try {
        const { records } = await loadBrowse(
          AbortSignal.any([controller.signal, AbortSignal.timeout(10000)]),
        );
        const out = await loadGenreArtwork(
          records.map((record) => record.data),
          AbortSignal.any([controller.signal, AbortSignal.timeout(20000)]),
        );
        if (!controller.signal.aborted) {
          setArtwork(out);
          if (Object.keys(out).length < 8 && attempt < 24)
            retry = setTimeout(() => void load(attempt + 1), 5000);
        }
      } catch {
        if (!controller.signal.aborted && attempt < 24)
          retry = setTimeout(() => void load(attempt + 1), 5000);
      }
    }
    void load(0);
    return () => {
      controller.abort();
      clearTimeout(retry);
    };
  }, []);
  return artwork;
}
