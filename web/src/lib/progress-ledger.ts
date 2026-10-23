// Close/replacement waits for this playback's final cumulative saves before
// revocation, including the effect-cleanup flush on episode changes.
const pending = new Map<string, Set<Promise<unknown>>>();
export function trackProgress<T>(id: string, promise: Promise<T>): Promise<T> {
  let requests = pending.get(id);
  if (!requests) {
    requests = new Set();
    pending.set(id, requests);
  }
  requests.add(promise);
  void promise
    .finally(() => {
      requests!.delete(promise);
      if (!requests!.size) pending.delete(id);
    })
    .catch(() => {});
  return promise;
}
export async function settleProgress(id: string) {
  await Promise.allSettled([...(pending.get(id) ?? [])]);
}
