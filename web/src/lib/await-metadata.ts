import { isMissing, isPending } from './api';

export async function awaitMetadata<T>(
  read: () => Promise<T>,
  ready: (value: T) => boolean,
  signal: AbortSignal,
): Promise<T> {
  const delays = [1000, 2000, 3000, 5000, 8000, 12000, 12000];
  for (let attempt = 0; attempt < delays.length; attempt++) {
    signal.throwIfAborted();
    try {
      const value = await read();
      if (ready(value)) return value;
    } catch (error) {
      if (!isMissing(error) && !isPending(error)) throw error;
    }
    await new Promise<void>((resolve, reject) => {
      const abort = () => {
        clearTimeout(timer);
        reject(signal.reason);
      };
      const timer = setTimeout(() => {
        signal.removeEventListener('abort', abort);
        resolve();
      }, delays[attempt]!);
      signal.addEventListener('abort', abort, { once: true });
      if (signal.aborted) abort();
    });
  }
  throw new Error('Metadata refresh is still pending. Try again shortly.');
}
