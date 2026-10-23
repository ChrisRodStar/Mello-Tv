// Only this client's namespaced navigation/account selection is disposable.
// Reference-app storage is never migrated, imported or deleted implicitly.
export function clearWebSession() {
  for (const getStorage of [() => localStorage, () => sessionStorage]) {
    try {
      const storage = getStorage();
      const keys = Array.from({ length: storage.length }, (_, i) => storage.key(i));
      for (const key of keys)
        if (key?.startsWith('family-stream.web.v1.')) storage.removeItem(key);
    } catch {
      /* Selection still works in memory. */
    }
  }
}
