import { isMissing } from './api';

export async function awaitMetadata<T>(
  read: () => Promise<T>,
  ready: (value: T) => boolean,
  signal: AbortSignal,
): Promise<T> {
  for (let attempt = 0; attempt < 20; attempt++) {
    signal.throwIfAborted();
    try {
      const value = await read();
      if (ready(value)) return value;
    } catch (error) {
      if (!isMissing(error)) throw error;
    }
    await new Promise<void>((resolve, reject) => {
      const abort = () => {
        clearTimeout(timer);
        reject(signal.reason);
      };
      const timer = setTimeout(() => {
        signal.removeEventListener('abort', abort);
        resolve();
      }, 1000);
      signal.addEventListener('abort', abort, { once: true });
      if (signal.aborted) abort();
    });
  }
  throw new Error('Metadata refresh is still pending. Try again shortly.');
}
