"""POSIX command ownership; retain the unreaped leader until group cleanup.
Adapted from the relay's offline acceptance process owner, with cwd/env support.
"""
import os
import select
import signal
import subprocess
import time


def run(argv, cwd, env, timeout=240):
    process = subprocess.Popen(argv, cwd=cwd, env=env, stdin=subprocess.DEVNULL,
                               start_new_session=True)
    events = None
    observed = None
    deadline = time.monotonic() + timeout
    try:
        if hasattr(select, "kqueue"):
            events = select.kqueue()
            events.control([select.kevent(process.pid, filter=select.KQ_FILTER_PROC,
                flags=select.KQ_EV_ADD | select.KQ_EV_ONESHOT,
                fflags=select.KQ_NOTE_EXIT | 0x04000000)], 0, 0)
        elif not hasattr(os, "waitid"):
            raise RuntimeError("unreaped process observation unavailable")
        while observed is None:
            if events is not None:
                ready = events.control(None, 1, 0)
                if ready:
                    if ready[0].flags & select.KQ_EV_ERROR:
                        raise RuntimeError("process observation failed")
                    observed = os.waitstatus_to_exitcode(ready[0].data)
            else:
                ready = os.waitid(os.P_PID, process.pid, os.WEXITED | os.WNOHANG | os.WNOWAIT)
                if ready is not None:
                    observed = ready.si_status if ready.si_code == os.CLD_EXITED else -ready.si_status
            if time.monotonic() >= deadline:
                raise TimeoutError("owned command deadline")
            if observed is None:
                time.sleep(0.02)
        if observed != 0:
            raise subprocess.CalledProcessError(observed, argv)
    finally:
        gone = False
        for sig in (signal.SIGTERM, signal.SIGKILL):
            if not gone:
                try:
                    os.killpg(process.pid, sig)
                except ProcessLookupError:
                    gone = True
                except PermissionError:
                    # Darwin may deny signaling a group containing only zombies.
                    if process.poll() is None:
                        raise
                    gone = True
            if sig == signal.SIGTERM:
                time.sleep(0.15)
        process.wait(timeout=3)
        if events is not None:
            events.close()
