import { apiJSON, noContent, profilePath } from '../lib/api';
import { useEffect } from 'react';

export function useProfileActivity(profileId?: string) {
  useEffect(() => {
    if (!profileId) return;
    let lastRecorded = 0;
    let pending: AbortController | undefined;
    const record = () => {
      if (document.visibilityState !== 'visible' || Date.now() - lastRecorded < 15 * 60_000)
        return;
      lastRecorded = Date.now();
      pending?.abort();
      pending = new AbortController();
      void apiJSON(`${profilePath(profileId)}/activity`, noContent, {
        method: 'POST',
        signal: AbortSignal.any([pending.signal, AbortSignal.timeout(7000)]),
      }).catch(() => {});
    };
    record();
    const interval = setInterval(record, 15 * 60_000);
    document.addEventListener('visibilitychange', record);
    window.addEventListener('focus', record);
    return () => {
      pending?.abort();
      clearInterval(interval);
      document.removeEventListener('visibilitychange', record);
      window.removeEventListener('focus', record);
    };
  }, [profileId]);
}
