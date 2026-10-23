import { settleProgress } from '../lib/progress-ledger';
import { useCallback, useEffect, useRef, useState } from 'react';
import { APIError, requestID } from '../lib/api';
import {
  changePlayback,
  resetPlaybackRequest,
  recoverPlayback,
  resolvePlaybackSource,
  revokePlayback,
  type PlaybackRequest,
  type PlaybackResolver,
  type PlaybackSource,
} from '../lib/playback';

type Operation = { kind: 'backup' | 'refresh' | 'renew'; key: string; source: PlaybackSource };
export function usePlaybackSession(request: PlaybackRequest, resolver?: PlaybackResolver) {
  const [source, setSource] = useState<PlaybackSource | null>(null);
  const [status, setStatus] = useState<'loading' | 'unavailable' | 'ready' | 'error'>(
    'loading',
  );
  const [error, setError] = useState('');
  const [busy, setBusy] = useState(false);
  const [attempt, setAttempt] = useState(0);
  const current = useRef<PlaybackSource | null>(null);
  const pending = useRef<Operation | null>(null);
  const lock = useRef(false);
  const controller = useRef<AbortController | null>(null);
  const owner = useRef(request);
  const restart = useRef(false);
  const retryPosition = useRef<number | null>(null);
  useEffect(() => {
    if (owner.current !== request) {
      current.current = null;
      pending.current = null;
      owner.current = request;
      retryPosition.current = null;
    }
    const abort = new AbortController();
    controller.current = abort;
    setStatus('loading');
    setError('');
    setBusy(false);
    lock.current = false;
    const previous = current.current;
    const work = previous
      ? recoverPlayback(request, previous.session.id, abort.signal)
      : resolvePlaybackSource(
          request,
          AbortSignal.any([abort.signal, AbortSignal.timeout(40000)]),
          resolver,
        );
    void work
      .then((result) => {
        if (abort.signal.aborted) return;
        if (result && previous && retryPosition.current !== null)
          result.session.resume_seconds = retryPosition.current;
        current.current = result;
        setSource(result);
        setStatus(result ? 'ready' : 'unavailable');
      })
      .catch((e) => {
        if (!abort.signal.aborted) {
          if (e instanceof APIError && e.status === 410) restart.current = true;
          setError(e instanceof APIError ? e.message : 'Couldn’t start playback. Try again.');
          setStatus('error');
        }
      });
    return () => abort.abort();
  }, [request, resolver, attempt]);
  useEffect(() => {
    return () => {
      controller.current?.abort();
      const last = current.current;
      if (last)
        queueMicrotask(() => {
          void settleProgress(last.session.id)
            .then(() => revokePlayback(request, last.session.id))
            .catch(() => {});
        });
    };
  }, [request]);
  const mediaError = useCallback(() => {
    setStatus('error');
    setError('Playback stopped. Retry or choose another source.');
  }, []);
  async function mutate(kind: Operation['kind'], position: number) {
    const base = current.current;
    if (!base || lock.current) return;
    lock.current = true;
    setBusy(true);
    setError('');
    const abort = new AbortController();
    controller.current?.abort();
    controller.current = abort;
    try {
      let operation = pending.current;
      if (operation && operation.kind !== kind) {
        setError('Retry the pending playback change first.');
        return;
      }
      if (!operation) {
        operation = { kind, key: requestID(), source: base };
        pending.current = operation;
      }
      const next = await changePlayback(
        request,
        operation.source,
        kind,
        operation.key,
        AbortSignal.any([abort.signal, AbortSignal.timeout(40000)]),
      );
      if (abort.signal.aborted) return;
      // A source/lease change replaces the playback item at the locally recorded
      // position. Cumulative measured time is retained under the same API ID.
      next.session.resume_seconds = position;
      pending.current = null;
      current.current = next;
      setSource(next);
      setStatus('ready');
    } catch (e) {
      if (abort.signal.aborted) return;
      if (
        e instanceof APIError &&
        [
          'source_refresh_required',
          'invalid_input',
          'invalid_selection',
          'unsupported_format',
          'renew_too_early',
        ].includes(e.code)
      )
        pending.current = null;
      setError(
        e instanceof APIError ? e.message : 'Couldn’t change playback. Retry the same action.',
      );
    } finally {
      lock.current = false;
      if (!abort.signal.aborted) setBusy(false);
    }
  }
  return {
    source,
    status,
    error,
    busy,
    mediaError,
    retry: (position: number) => {
      retryPosition.current = position;
      if (restart.current) {
        current.current = null;
        pending.current = null;
        resetPlaybackRequest(request);
        restart.current = false;
      }
      setAttempt((v) => v + 1);
    },
    mutate,
  };
}
