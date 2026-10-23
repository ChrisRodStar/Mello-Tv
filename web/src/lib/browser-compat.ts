// Older Safari versions lack these static methods. Install fallbacks before
// React runs effects so profile, catalog, and playback requests remain usable.
if (typeof AbortSignal.timeout !== 'function') {
  AbortSignal.timeout = (milliseconds: number) => {
    const controller = new AbortController();
    setTimeout(
      () => controller.abort(new DOMException('Request timed out', 'TimeoutError')),
      milliseconds,
    );
    return controller.signal;
  };
}
if (typeof AbortSignal.any !== 'function') {
  AbortSignal.any = (signals: AbortSignal[]) => {
    const controller = new AbortController();
    const handlers = new Map<AbortSignal, () => void>();
    const abort = (signal: AbortSignal) => {
      for (const [source, handler] of handlers) source.removeEventListener('abort', handler);
      handlers.clear();
      controller.abort(signal.reason);
    };
    for (const signal of new Set(signals)) {
      if (signal.aborted) {
        abort(signal);
        break;
      }
      const handler = () => abort(signal);
      handlers.set(signal, handler);
      signal.addEventListener('abort', handler, { once: true });
    }
    return controller.signal;
  };
}
if (typeof AbortSignal.prototype.throwIfAborted !== 'function') {
  AbortSignal.prototype.throwIfAborted = function () {
    if (this.aborted) throw this.reason ?? new DOMException('Request aborted', 'AbortError');
  };
}
