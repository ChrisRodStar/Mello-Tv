"""Offline ownership/deadline/restart tests. No source/media network access."""
import json
import os
from pathlib import Path
import signal
import sqlite3
import sys
import tempfile
import time
import unittest
from unittest.mock import patch

from acceptance_run import Rejected, initialize, status, supervise

PYTHON = sys.executable
SLEEP = [PYTHON, '-c', 'import time; time.sleep(60)']


class RunTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.root = Path(self.directory.name)
        self.root.chmod(0o700)
        self.state = self.root / 'budget.sqlite'

    def new(self, byte_limit=100000, seconds=2):
        initialize(self.state, byte_limit, seconds)

    def test_stalled_and_sigterm_ignoring_clients_are_killed_joined_at_deadline(self):
        self.new(seconds=0.2)
        before = time.monotonic()
        report = supervise(self.state, SLEEP, [SLEEP, [PYTHON, '-c',
                           'import signal,time; signal.signal(signal.SIGTERM,signal.SIG_IGN); time.sleep(60)']])
        self.assertEqual(report['stop_reason'], 'deadline')
        self.assertEqual(report['owned_processes_joined'], 3)
        self.assertLess(time.monotonic()-before, 2)
        self.assertEqual(status(self.state)[3], 1)
        self.assertNotIn(str(self.root), json.dumps(report))

    def test_byte_stop_cancels_all_clients(self):
        self.new()
        program = ('import sqlite3,sys,time; '
                   'd=sqlite3.connect(sys.argv[1]); '
                   'd.execute("UPDATE acceptance SET remaining=0,stopped=1"); d.commit(); time.sleep(60)')
        report = supervise(self.state, SLEEP, [[PYTHON, '-c', program, str(self.state)], SLEEP])
        self.assertEqual(report['stop_reason'], 'byte_limit')
        self.assertEqual(report['reserved_receive_bytes'], 100000)
        self.assertEqual(report['owned_processes_joined'], 3)

    def test_restart_preserves_remaining_and_original_deadline(self):
        self.new(seconds=0.3)
        original_deadline = status(self.state)[2]
        relay = [PYTHON, '-c', 'import sqlite3,sys; d=sqlite3.connect(sys.argv[1]); '
                 'd.execute("UPDATE acceptance SET remaining=remaining-1000"); d.commit()', str(self.state)]
        report = supervise(self.state, relay, [SLEEP], restarts=1)
        self.assertIn(report['stop_reason'], ('deadline', 'relay_exit'))
        self.assertEqual(status(self.state)[1], 98000)
        self.assertEqual(status(self.state)[2], original_deadline)
        self.assertEqual(report['owned_processes_joined'], 3)

    def test_partial_launch_failure_still_stops_and_joins_relay(self):
        self.new()
        from acceptance_run import launch
        processes = []
        def tracking(argv):
            process = launch(argv)
            processes.append(process)
            return process
        with patch('acceptance_run.launch', side_effect=tracking):
            with self.assertRaises(FileNotFoundError):
                supervise(self.state, SLEEP, [['/nonexistent/synthetic-client']])
        self.assertTrue(all(p.poll() is not None for p in processes))
        self.assertEqual(status(self.state)[3], 1)

    def test_success_still_joins_relay_and_does_not_leak_output(self):
        self.new()
        report = supervise(self.state, SLEEP, [[PYTHON, '-c', 'print("private-url-grant")']])
        self.assertEqual(report['stop_reason'], 'clients_complete')
        self.assertNotIn('private-url-grant', json.dumps(report))
        self.assertEqual(report['owned_processes_joined'], 2)

    def test_no_reset_or_symlink_or_expired_resume(self):
        self.new(seconds=0.01)
        with self.assertRaises(FileExistsError):
            initialize(self.state, 200000, 60)
        alias = self.root / 'link'
        alias.symlink_to(self.state)
        with self.assertRaises(Rejected):
            status(alias)
        time.sleep(0.02)
        with self.assertRaises(Rejected):
            supervise(self.state, SLEEP, [SLEEP])

    def test_completion_observation_keeps_pid_pinned_until_group_cleanup(self):
        from acceptance_run import launch, exited, stop_all
        process = launch([PYTHON, '-c', 'raise SystemExit(7)'])
        try:
            end = time.monotonic() + 2
            while exited(process) is None and time.monotonic() < end:
                time.sleep(0.01)
            self.assertEqual(exited(process), 7)
            self.assertIsNone(process.returncode)
        finally:
            stop_all([process])
        self.assertEqual(process.returncode, 7)

    def test_cleanup_attempts_every_group_and_join_after_one_denial(self):
        from acceptance_run import stop_all
        from unittest.mock import Mock
        first, second = Mock(pid=101, acceptance_events=None), Mock(pid=102, acceptance_events=None)
        first.poll.return_value = None
        def deny_first(pid, sig):
            if pid == 101:
                raise PermissionError()
        with patch('acceptance_run.os.killpg', side_effect=deny_first) as kill:
            with self.assertRaises(Rejected):
                stop_all([first, second])
        self.assertEqual(kill.call_count, 4)
        first.wait.assert_called_once()
        second.wait.assert_called_once()

    def test_exited_leader_cannot_leave_running_grandchild(self):
        self.new(seconds=0.3)
        marker = self.root / 'child'
        program = ('import subprocess,sys; '
                   'p=subprocess.Popen([sys.executable,"-c","import time; time.sleep(60)"]); '
                   'open(sys.argv[1],"w").write(str(p.pid))')
        from acceptance_run import exited, stop_all
        observed = []
        def cleanup_after_observation(processes):
            observed.append(exited(processes[1]))
            stop_all(processes)
        with patch('acceptance_run.stop_all', side_effect=cleanup_after_observation):
            # A second client stays active after the first leader exits.
            report = supervise(self.state, SLEEP, [[PYTHON, '-c', program, str(marker)], SLEEP])
        self.assertEqual(observed, [0])
        self.assertEqual(report['stop_reason'], 'deadline')
        self.assertEqual(report['owned_processes_joined'], 3)
        pid = int(marker.read_text())
        # An orphan may briefly be a zombie; a killed process must not be running.
        import subprocess
        result = subprocess.run(['ps', '-o', 'stat=', '-p', str(pid)], capture_output=True, text=True)
        self.assertTrue(not result.stdout.strip() or result.stdout.strip().startswith('Z'))

    def test_request_exhaustion_stops_owner_without_resetting_bytes(self):
        self.new(seconds=2)
        program = ('import sqlite3,sys,time; d=sqlite3.connect(sys.argv[1]); '
                   'd.execute("UPDATE acceptance_requests SET remaining=0"); d.commit(); time.sleep(60)')
        report = supervise(self.state, SLEEP, [[PYTHON, '-c', program, str(self.state)]])
        self.assertEqual(report['stop_reason'], 'request_limit')
        self.assertEqual(status(self.state)[1], 100000)
        self.assertEqual(status(self.state)[3], 1)

    def test_owned_client_requests_one_restart_without_exporting_pid(self):
        self.new(seconds=2)
        mailbox = self.root / 'restart.json'
        mailbox.write_text('{"request":0}')
        mailbox.chmod(0o600)
        program = ('import json,pathlib,sys,time; p=pathlib.Path(sys.argv[1]); '
                   'time.sleep(.1); p.write_text(\'{"request":1}\'); '
                   '\nwhile json.loads(p.read_text()) != {"completed":1}: time.sleep(.02)')
        report = supervise(self.state, SLEEP, [[PYTHON, '-c', program, str(mailbox)]], restarts=1, restart_file=mailbox)
        self.assertEqual(report['stop_reason'], 'clients_complete')
        self.assertEqual(report['owned_processes_joined'], 3)
        self.assertEqual(json.loads(mailbox.read_text()), {'completed': 1})

    def test_go_exhaustion_stops_owned_media_and_seals_after_join(self):
        self.new(seconds=2)
        provider_ledger = self.root/'go.jsonl'
        state = {'remaining_bytes':1000,'remaining_requests':5,
                 'deadline_ms':int(time.time()*1000)+2000,'stopped':False}
        provider_ledger.write_text(json.dumps(state)+'\n')
        provider_ledger.chmod(0o600)
        program = ('import json,sys,time; p=sys.argv[1]; '
                   's=json.loads(open(p).readlines()[-1]); s["remaining_requests"]=0; '
                   'open(p,"a").write(json.dumps(s)+"\\n"); time.sleep(60)')
        report = supervise(self.state, SLEEP, [[PYTHON,'-c',program,str(provider_ledger)],SLEEP],
                           provider_budget_file=provider_ledger)
        self.assertEqual(report['stop_reason'], 'provider_limit')
        self.assertEqual(report['owned_processes_joined'], 3)
        last = json.loads(provider_ledger.read_text().splitlines()[-1])
        self.assertTrue(last['stopped'])
        self.assertEqual(last['remaining_requests'], 0)
        self.assertEqual(last['remaining_bytes'], 1000)

    def test_numbered_restarts_reuse_one_ledger_for_multiple_cases(self):
        self.new(seconds=4)
        original_deadline = status(self.state)[2]
        mailbox = self.root/'restart.json'
        mailbox.write_text('{"request":0}')
        mailbox.chmod(0o600)
        program = ("import json,pathlib,sys,time; sys.path.insert(0,sys.argv[2]); "
                   "from acceptance_run import write_mailbox; p=pathlib.Path(sys.argv[1]); "
                   "\nfor number in (1,2):\n write_mailbox(p,{'request':number})\n"
                   " while json.loads(p.read_text()) != {'completed':number}: time.sleep(.02)")
        report = supervise(self.state, SLEEP, [[PYTHON,'-c',program,str(mailbox),str(Path(__file__).parent)]],
                           restarts=2, restart_file=mailbox)
        self.assertEqual(report['stop_reason'], 'clients_complete')
        self.assertEqual(report['owned_processes_joined'], 4)
        self.assertEqual(json.loads(mailbox.read_text()), {'completed':2})
        self.assertEqual(status(self.state)[2], original_deadline)
        self.assertEqual(status(self.state)[1], 100000)


if __name__ == '__main__':
    unittest.main()
