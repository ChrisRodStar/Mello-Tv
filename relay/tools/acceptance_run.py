#!/usr/bin/env python3
"""Own one opt-in production-relay acceptance run. Never fetches media itself."""
import argparse
import fcntl
import json
import os
from pathlib import Path
import signal
import select
import sqlite3
import stat
import subprocess
import tempfile
import time


class Rejected(Exception):
    pass


def private(path):
    path = Path(path)
    parent = path.parent.stat()
    info = path.lstat()
    if (not stat.S_ISDIR(parent.st_mode) or stat.S_IMODE(parent.st_mode) != 0o700
            or not stat.S_ISREG(info.st_mode) or stat.S_IMODE(info.st_mode) != 0o600):
        raise Rejected()
    return path


def connect(path):
    private(path)
    db = sqlite3.connect(Path(path).resolve().as_uri() + '?mode=rw', uri=True, timeout=0.1)
    db.execute('PRAGMA synchronous=FULL')
    return db


def initialize(path, byte_limit, seconds, requests=4096):
    if not 0 < byte_limit <= 2**53 or not 0 < seconds <= 86400 or not 0 < requests <= 100000:
        raise Rejected()
    path = Path(path)
    if stat.S_IMODE(path.parent.stat().st_mode) != 0o700:
        raise Rejected()
    fd = os.open(path, os.O_CREAT | os.O_EXCL | os.O_WRONLY, 0o600)
    os.close(fd)
    with connect(path) as db:
        db.execute('CREATE TABLE acceptance_requests (id INTEGER PRIMARY KEY CHECK(id=1), request_limit INTEGER NOT NULL, remaining INTEGER NOT NULL CHECK(remaining>=0))')
        db.execute('INSERT INTO acceptance_requests VALUES (1,?,?)', (requests, requests))
        db.execute('CREATE TABLE acceptance (id INTEGER PRIMARY KEY CHECK(id=1), '
                   'byte_limit INTEGER NOT NULL, remaining INTEGER NOT NULL CHECK(remaining>=0), '
                   'deadline_ms INTEGER NOT NULL, stopped INTEGER NOT NULL CHECK(stopped IN (0,1)))')
        db.execute('INSERT INTO acceptance VALUES (1,?,?,?,0)',
                   (byte_limit, byte_limit, int(time.time()*1000 + seconds*1000)))


def status(path):
    with connect(path) as db:
        row = db.execute('SELECT byte_limit,remaining,deadline_ms,stopped FROM acceptance WHERE id=1').fetchone()
    if row is None or not 0 <= row[1] <= row[0] <= 2**53 or row[3] not in (0, 1):
        raise Rejected()
    return row


def request_status(path):
    with connect(path) as db:
        exists = db.execute("SELECT 1 FROM sqlite_master WHERE name='acceptance_requests'").fetchone()
        if not exists:
            return None
        row = db.execute('SELECT request_limit,remaining FROM acceptance_requests WHERE id=1').fetchone()
    if row is None or not 0 <= row[1] <= row[0] <= 100000 or row[0] == 0:
        raise Rejected()
    return row


def stop_ledger(path):
    with connect(path) as db:
        db.execute('UPDATE acceptance SET stopped=1 WHERE id=1')


def write_mailbox(path, message):
    target = private(path)
    temporary = None
    try:
        with tempfile.NamedTemporaryFile(mode='w', dir=target.parent, delete=False) as output:
            temporary = Path(output.name)
            json.dump(message, output)
            output.flush()
            os.fsync(output.fileno())
        os.replace(temporary, target)
    finally:
        if temporary is not None:
            temporary.unlink(missing_ok=True)


def launch(argv, stdout=None, stderr=None):
    process = subprocess.Popen(argv, stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL if stdout is None else stdout,
                               stderr=subprocess.DEVNULL if stderr is None else stderr, start_new_session=True)
    process.acceptance_exit = None
    process.acceptance_events = None
    try:
        if hasattr(select, 'kqueue'):
            process.acceptance_events = select.kqueue()
            process.acceptance_events.control([select.kevent(
                process.pid, filter=select.KQ_FILTER_PROC,
                flags=select.KQ_EV_ADD | select.KQ_EV_ONESHOT,
                # Darwin sys/event.h NOTE_EXITSTATUS is omitted by Python 3.9.
                fflags=select.KQ_NOTE_EXIT | 0x04000000)], 0, 0)
        elif not hasattr(os, 'waitid'):
            raise Rejected()
    except Exception:
        stop_all([process])
        raise
    return process


def exited(process):
    # Observe completion without waitpid/poll: keep the leader unreaped so its
    # PID/PGID cannot be reused while we still own and may signal this group.
    if process.acceptance_exit is None:
        if process.acceptance_events is not None:
            events = process.acceptance_events.control(None, 1, 0)
            if events:
                if events[0].flags & select.KQ_EV_ERROR:
                    raise Rejected()
                process.acceptance_exit = os.waitstatus_to_exitcode(events[0].data)
        else:
            event = os.waitid(os.P_PID, process.pid, os.WEXITED | os.WNOHANG | os.WNOWAIT)
            if event is not None:
                process.acceptance_exit = event.si_status if event.si_code == os.CLD_EXITED else -event.si_status
    return process.acceptance_exit


def stop_all(processes):
    # Keep groups even after leaders exit: a client may leave children behind.
    failed = False
    gone = set()
    for sig in (signal.SIGTERM, signal.SIGKILL):
        for process in processes:
            if process.pid in gone:
                continue
            try:
                os.killpg(process.pid, sig)
            except ProcessLookupError:
                gone.add(process.pid)
            except PermissionError:
                # Darwin can return EPERM for a group containing only its
                # unreaped zombie leader. Reap it; never ignore a live denial.
                if process.poll() is None:
                    failed = True
                else:
                    gone.add(process.pid)
        if sig == signal.SIGTERM:
            time.sleep(0.15)
    for process in processes:
        try:
            process.wait(timeout=3)
        except subprocess.TimeoutExpired:
            failed = True
        finally:
            events = getattr(process, 'acceptance_events', None)
            if events is not None:
                events.close()
    if failed:
        raise Rejected()


def provider_status(path):
    private(path)
    descriptor = os.open(path, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK)
    with os.fdopen(descriptor, 'rb') as source:
        data = source.read(1024*1024+1)
    if len(data) > 1024*1024 or not data.endswith(b'\n'):
        raise Rejected()
    state = json.loads(data.splitlines()[-1])
    if (set(state) != {'remaining_bytes','remaining_requests','deadline_ms','stopped'}
            or any(type(state[name]) is not int or state[name] < 0
                   for name in ('remaining_bytes','remaining_requests','deadline_ms'))
            or type(state['stopped']) is not bool):
        raise Rejected()
    return state


def seal_provider_allowance(path):
    # Only after joining Go. Never replenishes/recreates a run.
    private(path)
    if Path(path).stat().st_size > 1024*1024:
        raise Rejected()
    descriptor = os.open(path, os.O_RDWR | os.O_APPEND | os.O_NOFOLLOW | os.O_NONBLOCK)
    with os.fdopen(descriptor, 'r+') as ledger:
        fcntl.flock(ledger.fileno(), fcntl.LOCK_EX | fcntl.LOCK_NB)
        lines = ledger.read(1024*1024+1).splitlines()
        if not lines:
            raise Rejected()
        state = json.loads(lines[-1])
        if not state['stopped']:
            state['stopped'] = True
            ledger.write(json.dumps(state, separators=(',', ':'))+'\n')
            ledger.flush()
            os.fsync(ledger.fileno())


def supervise(path, relay_command, clients, restarts=0, restart_file=None, provider_budget_file=None, cancellation_check=None):
    processes = []
    interrupted = False
    previously_joined = 0
    def interrupt(_signum, _frame):
        nonlocal interrupted
        interrupted = True
    handlers = {s: signal.signal(s, interrupt) for s in (signal.SIGINT, signal.SIGTERM)}
    reason = 'rejected'
    restart_requested = False
    completed_restarts = 0
    try:
        if restart_file is not None:
            if restarts < 1 or json.loads(private(restart_file).read_text()) != {'request': 0}:
                raise Rejected()
        limit, remaining, deadline_ms, stopped = status(path)
        # Monotonic bound protects this owner against wall-clock rollback; the
        # persisted absolute UTC deadline protects subsequent daemon restarts.
        end = time.monotonic() + max(0, deadline_ms/1000 - time.time())
        if remaining <= 0 or stopped or time.monotonic() >= end:
            raise Rejected()
        if cancellation_check is not None:
            cancellation_check()
        if interrupted:
            raise Rejected()
        relay = launch(relay_command)
        processes.append(relay)
        owned_clients = []
        for argv in clients:
            if interrupted:
                raise Rejected()
            process = launch(argv)
            processes.append(process)
            owned_clients.append(process)
        while True:
            _, remaining, _, stopped = status(path)
            if interrupted:
                reason = 'interrupted'
                break
            if time.monotonic() >= end or time.time()*1000 >= deadline_ms:
                reason = 'deadline'
                break
            if provider_budget_file is not None:
                provider = provider_status(provider_budget_file)
                if (provider['stopped'] or provider['remaining_bytes'] == 0
                        or provider['remaining_requests'] == 0 or time.time()*1000 >= provider['deadline_ms']):
                    reason = 'provider_limit'
                    break
            quota = request_status(path)
            if quota is not None and quota[1] == 0:
                reason = 'request_limit'
                break
            if remaining == 0 or stopped:
                reason = 'byte_limit' if remaining == 0 else 'stopped'
                break
            if restart_file is not None and not restart_requested:
                mailbox = private(restart_file)
                if mailbox.stat().st_size > 1024:
                    raise Rejected()
                message = json.loads(mailbox.read_text())
                if message == {'request': completed_restarts + 1}:
                    relay.send_signal(signal.SIGTERM)
                    restart_requested = True
                elif message != ({'request': 0} if completed_restarts == 0 else {'completed': completed_restarts}):
                    raise Rejected()
            if exited(relay) is not None:
                if restarts:
                    # Same ledger and deadline; never give a restarted daemon a
                    # fresh budget. Kill any children of the exited leader first.
                    stop_all([relay])
                    processes.remove(relay)
                    previously_joined += 1
                    relay = launch(relay_command)
                    processes.append(relay)
                    restarts -= 1
                    if restart_requested and restart_file is not None:
                        # Numbered intentional drain/restart; no PID is exported.
                        completed_restarts += 1
                        write_mailbox(restart_file, {'completed': completed_restarts})
                        restart_requested = False
                else:
                    reason = 'relay_exit'
                    break
            if all(exited(p) is not None for p in owned_clients):
                reason = 'clients_complete' if all(exited(p) == 0 for p in owned_clients) else 'client_failed'
                break
            time.sleep(0.02)
    finally:
        try:
            stop_ledger(path)
        finally:
            try:
                stop_all(processes)
            finally:
                try:
                    if provider_budget_file is not None:
                        seal_provider_allowance(provider_budget_file)
                finally:
                    for sig, handler in handlers.items():
                        signal.signal(sig, handler)
    limit, remaining, _, _ = status(path)
    quota = request_status(path)
    return {'http_exchange_limit': quota[0] if quota else None,
            'http_exchanges_admitted': quota[0]-quota[1] if quota else None,
            'evidence': 'bounded_acceptance_run', 'stop_reason': reason,
            'reserved_receive_bytes': limit-remaining, 'byte_limit': limit,
            'owned_processes_joined': len(processes) + previously_joined, 'decoded': False, 'player_verified': False}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--opt-in', action='store_true', required=True)
    parser.add_argument('--state', required=True)
    parser.add_argument('--new', action='store_true')
    parser.add_argument('--bytes', type=int)
    parser.add_argument('--seconds', type=float)
    parser.add_argument('--requests', type=int)
    parser.add_argument('--relay', required=True)
    parser.add_argument('--config', required=True)
    parser.add_argument('--clients', required=True, help='private JSON array of argv arrays; no shell')
    parser.add_argument('--relay-restarts', type=int, default=0)
    parser.add_argument('--restart-file')
    parser.add_argument('--provider-budget-file', help='seal original Go allowance after joining all owned processes')
    args = parser.parse_args()
    try:
        if args.new:
            initialize(args.state, args.bytes, args.seconds, args.requests if args.requests is not None else 4096)
        elif args.bytes is not None or args.seconds is not None or args.requests is not None:
            raise Rejected()
        manifest = private(args.clients)
        if manifest.stat().st_size > 65536:
            raise Rejected()
        clients = json.loads(manifest.read_text())
        if (not isinstance(clients, list) or not 1 <= len(clients) <= 16
                or not 0 <= args.relay_restarts <= 4
                or any(not isinstance(argv, list) or not 1 <= len(argv) <= 128
                       or any(not isinstance(v, str) or not v or '\0' in v for v in argv) for argv in clients)):
            raise Rejected()
        report = supervise(args.state,
                           [args.relay, args.config, '--acceptance-budget', str(Path(args.state).resolve())],
                           clients, args.relay_restarts, args.restart_file, args.provider_budget_file)
        print(json.dumps(report, sort_keys=True))
        return 0 if report['stop_reason'] in ('clients_complete', 'byte_limit', 'request_limit', 'deadline') else 1
    except Exception:
        print('{"evidence":"incomplete","error":"acceptance_run_rejected"}')
        return 1


if __name__ == '__main__':
    raise SystemExit(main())
