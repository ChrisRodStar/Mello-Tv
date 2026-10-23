import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import time
import unittest
from unittest.mock import patch, Mock

import playback_acceptance as playback
import playback_operator as operator
from acceptance_run import status, provider_status


class PlaybackTests(unittest.TestCase):
    def test_decoder_rejects_source_or_injected_input_before_launch(self):
        for url in ('https://source.invalid/a', 'http://127.0.0.1:80@source.invalid/a', "http://127.0.0.1:8092/entry'\nfile '/secret'"):
            with patch.object(playback.subprocess, 'Popen') as launch:
                with self.assertRaises(playback.Rejected):
                    playback.decode('/unused', url)
                launch.assert_not_called()

    def test_owned_player_reports_cannot_pass_without_frames_seek_and_view(self):
        with tempfile.TemporaryDirectory() as directory:
            player = playback.Player({'stream_port':8092}, directory)
            player.process = subprocess.Popen([sys.executable, '-c', 'import time;time.sleep(60)'])
            player.output = tempfile.TemporaryFile()
            report = dict(event='played', sequence=1, seek_completed=True,
                          view_ready=True, frames_observed=0, position_s=10, started_s=0)
            player.output.write(json.dumps(report).encode()+b'\n')
            player.output.flush()
            try:
                with self.assertRaises(playback.Rejected):
                    player.play('http://127.0.0.1:8092/stream/test/entry?st=test', 0, 10)
            finally:
                player.close()
            self.assertIsNotNone(player.process.returncode)

    def test_reading_report_does_not_move_child_output_cursor(self):
        with tempfile.TemporaryDirectory() as directory:
            player = playback.Player({'stream_port':8092}, directory)
            player.process = subprocess.Popen([sys.executable, '-c', 'import time;time.sleep(60)'])
            player.output = tempfile.TemporaryFile()
            report = dict(event='played', sequence=1, seek_completed=True,
                          view_ready=True, frames_observed=24, position_s=2, started_s=0)
            data = json.dumps(report).encode()+b'\n'
            player.output.write(data)
            player.output.flush()
            try:
                result = player.play('http://127.0.0.1:8092/stream/test/entry?st=test', 0, 2)
                self.assertEqual(result['position_s'], 2)
                self.assertEqual(player.output.tell(), len(data))
            finally:
                player.close()

    def test_batch_failure_stops_later_case_and_seals_original_allowance(self):
        with patch.object(playback, 'run_case', side_effect=playback.Rejected()) as case, \
                patch.object(playback, 'seal_provider_allowance') as seal:
            report = playback.run({'cases':[{},{}], 'provider_budget_file':'original'})
        case.assert_called_once()
        seal.assert_called_once_with('original')
        self.assertFalse(report['overall_acceptance'])
        self.assertEqual(report['cases'][0]['evidence'], 'incomplete')

    def test_failed_initial_attestation_seals_once_and_preserves_unrelated_files(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            prepared = {'binaries':{'providers_netcheck':'/synthetic'}, 'hashes':{},
                        'sources':{}, 'heads':{'relay':'synthetic','providers':'synthetic'}}
            operator.write(root/'prepared.json', prepared)
            operator.write(root/'unrelated.txt', b'keep')
            with patch.object(operator, 'attest', return_value={'ipv4':False,'ipv6':False,'passed':False}) as attest, \
                    patch.object(operator, 'supervise') as supervise, \
                    patch.object(operator.socket, 'socket', side_effect=lambda: Mock()):
                report = operator.execute(root)
            attest.assert_called_once()
            supervise.assert_not_called()
            self.assertTrue(provider_status(root/'go.jsonl')['stopped'])
            self.assertEqual(status(root/'rust.sqlite')[3], 1)
            self.assertEqual(report['rust_accounting']['reserved_receive_bytes'], 0)
            self.assertEqual((root/'unrelated.txt').read_bytes(), b'keep')
            original = (root/'go.jsonl').read_bytes()
            with self.assertRaises(Exception):
                operator.execute(root)
            self.assertEqual((root/'go.jsonl').read_bytes(), original)

    def test_playback_orchestration_preserves_real_generation_and_cleanup_evidence(self):
        class API:
            def __init__(self, config, **kwargs):
                self.config = config
                self.deadline = time.monotonic()+60
                self.revision = 0
                self.refreshes = []
            def resolve(self, request):
                self.refreshes.append(request['lookup'].get('refresh', False))
                return {'selected':{}, 'backups':[{}]}
            def control(self, method, path, body=None, key=None):
                if method == 'DELETE':
                    return None
                if method == 'POST':
                    self.revision += 1
                return {'session_id':'synthetic', 'entry_path':'/stream/synthetic/entry',
                        'auth_revision':1,'revision':self.revision, 'source_generation':self.revision,
                        'format':'mp4' if self.revision == 2 else 'hls', 'resume_required':True}
            def wait_relay_ready(self):
                pass
        class Player:
            def __init__(self, *args):
                self.position = 0
            def play(self, url, position, seconds, action='load'):
                self.position = position+seconds
                return {'position_s':self.position, 'rendered_subtitles':False}
            def close(self):
                pass
            def restart(self):
                pass
        provider = Mock()
        with tempfile.TemporaryDirectory() as directory:
            lookup, mailbox = Path(directory)/'lookup', Path(directory)/'restart'
            operator.write(lookup, {'lookup':{'title':{'type':'movie','tmdb_id':550}}})
            operator.write(mailbox, {'request':0})
            original_mailbox = playback.write_mailbox
            def acknowledged(path, message):
                original_mailbox(path, {'completed':message['request']})
            with patch.object(playback, 'API', API), patch.object(playback, 'Player', Player), \
                    patch.object(playback, 'OwnedProvider', return_value=provider), \
                    patch.object(playback, 'entry_url', return_value='http://127.0.0.1:8092/stream/synthetic/entry?st=synthetic'), \
                    patch.object(playback, 'existing_entry', return_value={'requests':1,'body_bytes':0}) as access, \
                    patch.object(playback, 'decode', return_value={'synthetic':True}), \
                    patch.object(playback, 'write_mailbox', side_effect=acknowledged):
                report = playback.run_case({'lookup_file':str(lookup), 'owner_id':'synthetic',
                    'expected_format':'hls', 'seek_positions':[600,3600], 'provider_lifecycle':True,
                    'restart_file':str(mailbox), 'restart_sequence':1, 'ffmpeg':'synthetic'})
        self.assertEqual(report['evidence'], 'owned_playback')
        self.assertEqual(access.call_count, 2)
        self.assertEqual(provider.start.call_count, 2)
        self.assertEqual(provider.stop.call_count, 2)
        self.assertEqual(report['cleanup'], ['player_joined','session_revoked','provider_joined'])
        backup = next(item for item in report['player'] if item['phase'] == 'explicit_backup')
        self.assertEqual(backup['format'], 'mp4')
        self.assertEqual(backup['source_generation'], 2)
        self.assertIn('relay_restart_persisted_reload_resume', report['recovery'])

    def test_damaged_go_record_does_not_skip_private_cleanup_or_outcome(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            prepared = {'binaries':{'providers_netcheck':'/synthetic'}, 'hashes':{},
                        'sources':{}, 'heads':{'relay':'synthetic','providers':'synthetic'}}
            operator.write(root/'prepared.json', prepared)
            (root/'secrets').mkdir(mode=0o700)
            operator.write(root/'secrets'/'key', b'synthetic')
            def damage(*_args):
                with (root/'go.jsonl').open('ab') as output:
                    output.write(b'{broken')
                return {'passed':False}
            with patch.object(operator, 'attest', side_effect=damage), \
                    patch.object(operator.socket, 'socket', side_effect=lambda: Mock()):
                report = operator.execute(root)
            self.assertFalse(report['go_accounting']['available'])
            self.assertFalse(report['go_accounting']['seal_confirmed'])
            self.assertFalse((root/'secrets').exists())
            self.assertTrue(report['private_cleanup']['complete'])
            self.assertTrue((root/'outcome.json').exists())

    def test_sigterm_during_initial_attestation_joins_diagnostic_and_seals(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            operator.write(root/'prepared.json', {'binaries':{'providers_netcheck':'/synthetic'},
                'hashes':{}, 'sources':{}, 'heads':{}})
            (root/'secrets').mkdir(mode=0o700)
            operator.write(root/'secrets'/'key', b'synthetic')
            marker = root/'diagnostic-pid'
            script = ("import pathlib,sys,time,os; pathlib.Path(sys.argv[1]).write_text(str(os.getpid())); time.sleep(60)")
            program = ("import sys; sys.path.insert(0,sys.argv[2]); import playback_operator as o; "
                       "original=o.attest; "
                       "\ndef synthetic(argv,seconds,check): return original([sys.executable,'-c',sys.argv[3],sys.argv[4]],seconds,check)"
                       "\no.attest=synthetic; o.execute(sys.argv[1])")
            process = subprocess.Popen([sys.executable,'-c',program,str(root),str(Path(__file__).parent),script,str(marker)],
                                       stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
            try:
                deadline = time.monotonic()+5
                while not marker.exists():
                    if process.poll() is not None or time.monotonic() >= deadline:
                        self.fail('synthetic diagnostic did not start')
                    time.sleep(.02)
                process.terminate()
                process.wait(timeout=5)
            finally:
                if process.poll() is None:
                    process.kill()
                    process.wait(timeout=3)
            child = int(marker.read_text())
            result = subprocess.run(['ps','-o','stat=','-p',str(child)], capture_output=True, text=True)
            self.assertTrue(not result.stdout.strip() or result.stdout.strip().startswith('Z'))
            self.assertTrue(provider_status(root/'go.jsonl')['stopped'])
            self.assertEqual(status(root/'rust.sqlite')[3], 1)
            self.assertFalse((root/'secrets').exists())
            self.assertTrue((root/'outcome.json').exists())

    def test_failed_session_delete_preserves_completed_decode_and_failed_stage(self):
        with tempfile.TemporaryDirectory() as directory:
            lookup = Path(directory)/'lookup'
            operator.write(lookup, {'lookup':{'title':{'type':'movie','tmdb_id':550}}})
            api = Mock(deadline=time.monotonic()+60)
            api.resolve.return_value = {'selected':{},'backups':[]}
            generation = 0
            def control(method, path, *args):
                nonlocal generation
                if method == 'DELETE':
                    raise playback.Rejected()
                generation += 1
                return {'session_id':'synthetic', 'entry_path':'/stream/synthetic/entry',
                        'auth_revision':1,'revision':generation,'source_generation':generation,
                        'format':'hls','resume_required':True}
            api.control.side_effect = control
            player = Mock(position=0)
            player.play.side_effect = [{'position_s':120,'rendered_subtitles':False},playback.Rejected()]
            provider = Mock()
            with patch.object(playback, 'API', return_value=api), \
                    patch.object(playback, 'Player', return_value=player), \
                    patch.object(playback, 'OwnedProvider', return_value=provider), \
                    patch.object(playback, 'entry_url', return_value='synthetic'), \
                    patch.object(playback, 'decode', return_value={'synthetic_frames':240}):
                report = playback.run_case({'lookup_file':str(lookup), 'owner_id':'synthetic',
                    'expected_format':'hls','seek_positions':[], 'ffmpeg':'synthetic'})
            self.assertEqual(report['evidence'], 'incomplete')
            self.assertEqual(report['stage'], 'source_refresh')
            self.assertEqual(report['decode'], {'synthetic_frames':240})
            self.assertEqual(len(report['player']), 1)
            self.assertIn('session_revoke_failed_isolated_store_cleanup_required', report['cleanup'])
            self.assertIn('provider_joined', report['cleanup'])
            player.close.assert_called_once()
            provider.stop.assert_called_once()

    def test_operator_cli_fails_when_private_cleanup_or_ledger_sealing_is_unconfirmed(self):
        complete = {'owner':{'stop_reason':'clients_complete'},
                    'private_cleanup':{'complete':True},
                    'rust_accounting':{'stopped':True},'go_accounting':{'stopped':True}}
        for failure in (None,'private_cleanup','rust_accounting','go_accounting'):
            report = json.loads(json.dumps(complete))
            if failure == 'private_cleanup':
                report[failure]['complete'] = False
            elif failure:
                report[failure] = {'available':False,'seal_confirmed':False}
            with self.subTest(failure=failure), \
                    patch.object(sys, 'argv', ['operator','run','--opt-in','--directory','/synthetic']), \
                    patch.object(operator, 'execute', return_value=report), patch('builtins.print'):
                self.assertEqual(operator.main(), 0 if failure is None else 1)


if __name__ == '__main__':
    unittest.main()
