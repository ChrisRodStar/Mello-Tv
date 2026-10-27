import { useEffect, useState } from 'react';
import { loadBrowse, waitForCatalogReady } from '../../lib/catalog';
import { loadGenreArtwork, type GenreArtwork } from '../../lib/genre-artwork';

export function useGenreArtwork(profileID: string) {
  const [artwork, setArtwork] = useState<GenreArtwork>({});
  useEffect(() => {
    const controller = new AbortController();
    async function load() {
      try {
        await waitForCatalogReady(profileID, AbortSignal.any([controller.signal, AbortSignal.timeout(120000)]));
        const { records } = await loadBrowse(
          AbortSignal.any([controller.signal, AbortSignal.timeout(10000)]),
        );
        const out = await loadGenreArtwork(
          records.map((record) => record.data),
          AbortSignal.any([controller.signal, AbortSignal.timeout(20000)]),
        );
        if (!controller.signal.aborted) setArtwork(out);
      } catch { /* Artwork is optional; onboarding retains its selectable genre list. */ }
    }
    void load();
    return () => {
      controller.abort();
    };
  }, [profileID]);
  return artwork;
}
