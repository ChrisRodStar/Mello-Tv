import json
import os
from pathlib import Path
import tempfile
import sys
import time
import subprocess
import unittest
from unittest.mock import patch

import service_acceptance as service


class FakeAPI:
    last = None
    fail = False
    unchanged = False

    def __init__(self, config):
        self.calls = []
        self.config = config
        FakeAPI.last = self

    def resolve(self, request):
        self.calls.append(('resolve', request['lookup'].get('refresh', False)))
        return {'version': 'providers.v1', 'selected': {}}

    def control(self, method, path, body=None, key=None):
        self.calls.append((method, path, body, key))
        if method == 'DELETE':
            return None
        generation = 3 if FakeAPI.unchanged else 3 + sum(1 for call in self.calls if call[0] == 'POST')
        return {'session_id': 'synthetic-session', 'auth_revision': 2, 'revision': 4,
                'source_generation': generation, 'entry_path': '/stream/synthetic-session/entry',
                'format': 'hls', 'resume_required': True}

    def grant(self, receipt):
        return 'synthetic-grant'

    def probe(self, receipt, grant):
        if FakeAPI.fail:
            raise service.Rejected()
        return {'evidence': 'bounded_relay_byte_access'}


class ServiceAcceptanceTests(unittest.TestCase):
    def setUp(self):
        FakeAPI.fail = False
        FakeAPI.unchanged = False
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.lookup = Path(self.directory.name)/'lookup.json'
        self.lookup.write_text(json.dumps({'lookup': {'title': {'type': 'movie', 'tmdb_id': 550}}}))
        os.chmod(self.lookup, 0o600)

    def test_refresh_is_explicit_and_session_is_revoked(self):
        with patch.object(service, 'API', FakeAPI):
            report = service.run({'lookup_file': str(self.lookup), 'owner_id': 'synthetic-owner', 'refresh': True})
        calls = FakeAPI.last.calls
        self.assertIn(('resolve', False), calls)
        self.assertIn(('resolve', True), calls)
        self.assertEqual(calls[-1][0], 'DELETE')
        self.assertFalse(report['decoded'])
        self.assertFalse(report['temporal_seek'])
        self.assertFalse(report['rendered_subtitles'])
        self.assertNotIn('synthetic-grant', json.dumps(report))

    def test_failed_probe_still_revokes_session(self):
        FakeAPI.fail = True
        with patch.object(service, 'API', FakeAPI), self.assertRaises(service.Rejected):
            service.run({'lookup_file': str(self.lookup), 'owner_id': 'synthetic-owner'})
        self.assertEqual(FakeAPI.last.calls[-1][0], 'DELETE')

    def test_nonloopback_service_configuration_rejected(self):
        with self.assertRaises(service.Rejected):
            service.API({'provider_port': 8090, 'control_port': 8091, 'stream_port': 8092,
                         'host': '192.168.1.1'})

    def test_refresh_rejects_unchanged_generation_and_revokes(self):
        FakeAPI.unchanged = True
        with patch.object(service, 'API', FakeAPI), self.assertRaises(service.Rejected):
            service.run({'lookup_file': str(self.lookup), 'owner_id': 'synthetic-owner', 'refresh': True})
        self.assertEqual(FakeAPI.last.calls[-1][0], 'DELETE')

    def test_restart_readiness_retries_without_resetting_request_allowance(self):
        api = service.API({'provider_port':8090,'control_port':8091,'stream_port':8092})
        def ready(port, method, path, body=None, bearer=None, key=None):
            api.remaining_requests -= 1
            if api.remaining_requests == 31:
                raise ConnectionRefusedError()
            if api.remaining_requests == 30:
                raise service.Rejected()
            return {'status':'ready'}
        with patch.object(api, 'control', side_effect=lambda method,path: ready(None,method,path)):
            api.wait_relay_ready()
        self.assertEqual(api.remaining_requests, 29)

    def test_timed_out_owned_test_client_stops_its_provider_descendant(self):
        marker = Path(self.directory.name)/'child'
        program = ('import subprocess,sys,time; '
                   'p=subprocess.Popen([sys.executable,"-c","import signal,time; signal.signal(signal.SIGTERM,signal.SIG_IGN); time.sleep(60)"]); '
                   'open(sys.argv[1],"w").write(str(p.pid)); time.sleep(60)')
        before = time.monotonic()
        with self.assertRaises(service.Rejected):
            service.own_client([sys.executable,'-c',program,str(marker)], .3)
        self.assertLess(time.monotonic()-before, 2)
        child = int(marker.read_text())
        result = subprocess.run(['ps','-o','stat=','-p',str(child)], capture_output=True, text=True)
        self.assertTrue(not result.stdout.strip() or result.stdout.strip().startswith('Z'))

    def test_batch_runs_sequentially_and_seals_shared_allowance_once(self):
        observed = []
        def execute(case):
            observed.append(case['case'])
            return {'evidence':'synthetic'}
        with patch.object(service, 'run', side_effect=execute), patch.object(service, 'seal_provider_allowance') as seal:
            report = service.run_config({'cases':[{'case':1},{'case':2}], 'provider_budget_file':'private-ledger'})
        self.assertEqual(observed, [1,2])
        self.assertEqual(len(report['cases']), 2)
        seal.assert_called_once_with('private-ledger')

    def test_batch_failure_still_seals_without_starting_later_cases(self):
        with patch.object(service, 'run', side_effect=service.Rejected()) as execute, patch.object(service, 'seal_provider_allowance') as seal:
            with self.assertRaises(service.Rejected):
                service.run_config({'cases':[{},{}], 'provider_budget_file':'private-ledger'})
        self.assertEqual(execute.call_count, 1)
        seal.assert_called_once_with('private-ledger')


if __name__ == '__main__':
    unittest.main()
