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
const sessionEnded = (error: unknown) =>
  error instanceof APIError &&
  (error.status === 410 || ['session_expired', 'revoked'].includes(error.code));
export function usePlaybackSession(request: PlaybackRequest, resolver?: PlaybackResolver) {
  const [source, setSource] = useState<PlaybackSource | null>(null);
  const [status, setStatus] = useState<'loading' | 'unavailable' | 'ready' | 'error'>(
    'loading',
  );
  const [error, setError] = useState('');
  const [busy, setBusy] = useState(false);
  const [recovering, setRecovering] = useState(false);
  const [attempt, setAttempt] = useState(0);
  const current = useRef<PlaybackSource | null>(null);
  const pending = useRef<Operation | null>(null);
  const lock = useRef(false);
  const controller = useRef<AbortController | null>(null);
  const owner = useRef(request);
  const restart = useRef(false);
  const retryPosition = useRef<number | null>(null);
  const autoRefreshUsed = useRef(false);
  const failedMedia = useRef(false);
  useEffect(() => {
    if (owner.current !== request) {
      current.current = null;
      pending.current = null;
      owner.current = request;
      retryPosition.current = null;
      restart.current = false;
      autoRefreshUsed.current = false;
      failedMedia.current = false;
      setSource(null);
    }
    const abort = new AbortController();
    controller.current = abort;
    setStatus('loading');
    setError('');
    setBusy(false);
    setRecovering(false);
    lock.current = false;
    const previous = current.current;
    const signal = AbortSignal.any([abort.signal, AbortSignal.timeout(40000)]);
    const work = previous
      ? recoverPlayback(request, previous.session.id, signal)
      : resolvePlaybackSource(request, signal, resolver);
    void work
      .then((result) => {
        if (abort.signal.aborted) return;
        if (result && retryPosition.current !== null)
          result.session.resume_seconds = retryPosition.current;
        retryPosition.current = null;
        current.current = result;
        setSource(result);
        setStatus(result ? 'ready' : 'unavailable');
      })
      .catch((e) => {
        if (!abort.signal.aborted) {
          if (sessionEnded(e)) restart.current = true;
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
  const mutate = useCallback(
    async (kind: Operation['kind'], position: number, recovery = false) => {
      const base = current.current;
      if (owner.current !== request || !base || lock.current) return;
      if (pending.current && pending.current.kind !== kind) {
        setError('Retry the pending playback change first.');
        if (recovery) setStatus('error');
        return;
      }
      lock.current = true;
      setBusy(true);
      if (recovery) {
        setRecovering(true);
        setStatus('loading');
      }
      setError('');
      const abort = new AbortController();
      controller.current?.abort();
      controller.current = abort;
      try {
        let operation = pending.current;
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
        if (
          abort.signal.aborted ||
          controller.current !== abort ||
          owner.current !== request ||
          current.current !== base
        )
          return;
        // A source/lease change replaces the playback item at the locally recorded
        // position. Cumulative measured time is retained under the same API ID.
        next.session.resume_seconds = position;
        pending.current = null;
        current.current = next;
        setSource(next);
        setStatus('ready');
      } catch (e) {
        if (
          abort.signal.aborted ||
          controller.current !== abort ||
          owner.current !== request ||
          current.current !== base
        )
          return;
        if (sessionEnded(e)) {
          restart.current = true;
          pending.current = null;
        }
        if (
          e instanceof APIError &&
          [
            'source_refresh_required',
            'no_source',
            'quality_unavailable',
            'invalid_input',
            'invalid_selection',
            'unsupported_format',
            'renew_too_early',
          ].includes(e.code)
        )
          pending.current = null;
        setError(
          e instanceof APIError
            ? e.message
            : 'Couldn’t change playback. Retry the same action.',
        );
        if (recovery) setStatus('error');
      } finally {
        // An aborted operation can finish after a newer operation has acquired
        // the lock. Only its owner may release it.
        if (controller.current === abort) {
          lock.current = false;
          if (!abort.signal.aborted) setBusy(false);
          if (!abort.signal.aborted) setRecovering(false);
        }
      }
    },
    [request],
  );
  const mediaError = useCallback(
    (position = 0) => {
      if (owner.current !== request || !current.current) return;
      if (lock.current && failedMedia.current) return;
      failedMedia.current = true;
      setStatus('error');
      setError('Playback stopped. Retry or choose another source.');
      if (!lock.current && !autoRefreshUsed.current) {
        autoRefreshUsed.current = true;
        void mutate('refresh', position, true);
      }
    },
    [request, mutate],
  );
  return {
    source: owner.current === request ? source : null,
    status: owner.current === request ? status : ('loading' as const),
    error,
    busy,
    recovering,
    mediaError,
    retry: (position: number) => {
      retryPosition.current = position;
      if (restart.current) {
        current.current = null;
        pending.current = null;
        resetPlaybackRequest(request);
        restart.current = false;
        failedMedia.current = false;
      } else if (failedMedia.current && current.current) {
        void mutate(pending.current?.kind ?? 'refresh', position, true);
        return;
      }
      setAttempt((v) => v + 1);
    },
    mutate,
  };
}
