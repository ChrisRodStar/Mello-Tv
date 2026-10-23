#!/usr/bin/env python3
"""Test-owned future-API orchestration. Only loopback provider/relay HTTP clients.

Secrets, lookup bodies, selections, receipts and grants stay in private files.
Run as an owned client of acceptance_run.py; independent provider egress needs
its own allowance. This tool does not launch a decoder or establish player proof.
"""
import argparse
import base64
import http.client
import ipaddress
import json
import os
from pathlib import Path
import subprocess
import signal
import sys
import tempfile
import time
import uuid

from acceptance_run import write_mailbox, launch, exited, stop_all, seal_provider_allowance
from acceptance_probe import Probe, Rejected, private_file, wall_deadline, references, resource_path


def encode(value):
    return base64.urlsafe_b64encode(value).rstrip(b'=').decode('ascii')


class OwnedProvider:
    """A dedicated direct child, inside the outer acceptance owner's group."""
    def __init__(self, config):
        self.config, self.process = config, None

    def start(self, api):
        argv = self.config['provider_command']
        if (not isinstance(argv, list) or not argv or len(argv) > 16
                or any(not isinstance(value, str) or not value or '\0' in value for value in argv)):
            raise Rejected()
        env = dict(os.environ)
        ready = self.config.get('provider_ready_file')
        if ready:
            Path(ready).unlink(missing_ok=True)
            env['FAMILY_STREAM_SYNTHETIC_READY'] = ready
        if self.config.get('provider_attest_command'):
            subprocess.run(self.config['provider_attest_command'], stdin=subprocess.DEVNULL,
                           stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, timeout=25, check=True)
        self.process = subprocess.Popen(argv, stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL,
                                        stderr=subprocess.DEVNULL, env=env)
        for _ in range(20):
            if self.process.poll() is not None:
                raise Rejected()
            if ready and Path(ready).exists():
                api.config['provider_port'] = json.loads(private_file(ready, 1024))['port']
            if not ready or Path(ready).exists():
                try:
                    bearer = private_file(self.config['provider_bearer_file'], 4096).decode('ascii')
                    api.request(api.config['provider_port'], 'GET', '/health/readyz', bearer=bearer)
                    return
                except (OSError, Rejected):
                    pass
            time.sleep(0.1)
        raise Rejected()

    def stop(self):
        if self.process is None:
            return
        self.process.terminate()
        try:
            self.process.wait(timeout=5)
        except subprocess.TimeoutExpired:
            self.process.kill()
            self.process.wait(timeout=3)
        finally:
            self.process = None


class API:
    def __init__(self, config, seconds=120, requests=32):
        self.config = config
        self.deadline = time.monotonic() + seconds
        self.remaining_requests = requests
        self.stream_requests = 192
        self.stream_bytes = 32*1024*1024
        for name in ('provider_port', 'control_port', 'stream_port'):
            if not isinstance(config[name], int) or not 1 <= config[name] <= 65535:
                raise Rejected()
        if not ipaddress.ip_address(config.get('host', '127.0.0.1')).is_loopback:
            raise Rejected()
        self.host = config.get('host', '127.0.0.1')

    def request(self, port, method, path, body=None, bearer=None, key=None):
        if self.remaining_requests <= 0 or time.monotonic() >= self.deadline:
            raise Rejected()
        self.remaining_requests -= 1
        connection = http.client.HTTPConnection(self.host, port, timeout=min(35, self.deadline-time.monotonic()))
        try:
            headers = {'Connection': 'close'}
            if bearer:
                headers['Authorization'] = 'Bearer ' + bearer
            if body is not None:
                body = json.dumps(body, separators=(',', ':')).encode()
                if len(body) > 4*1024*1024:
                    raise Rejected()
                headers['Content-Type'] = 'application/json'
            if key:
                headers['Idempotency-Key'] = key
            connection.request(method, path, body=body, headers=headers)
            response = connection.getresponse()
            data = response.read(2*1024*1024+1)
            if len(data) > 2*1024*1024 or response.status not in (200, 201, 204):
                raise Rejected()
            return json.loads(data) if data else None
        finally:
            connection.close()

    def resolve(self, request):
        bearer = private_file(self.config['provider_bearer_file'], 4096).decode('ascii')
        selection = self.request(self.config['provider_port'], 'POST', '/internal/v1/resolve', request, bearer)
        if selection.get('version') != 'providers.v1' or not selection.get('selected'):
            raise Rejected()
        if {status['provider'] for status in selection.get('providers', [])} != {'vidcore', 'vidlink', 'vidsrc', 'vidfast'}:
            raise Rejected()
        retained = [selection['selected']] + (selection.get('backups') or [])
        for ranked in retained:
            if ranked['candidate']['source']['title'] != request['lookup']['title']:
                raise Rejected()
        return selection

    def control(self, method, path, body=None, key=None):
        bearer = private_file(self.config['control_bearer_file'], 4096).decode('ascii')
        return self.request(self.config['control_port'], method, path, body, bearer, key)

    def wait_relay_ready(self):
        # Owner acknowledgment means the process was spawned, not that its
        # listeners/store/guard are ready. Every retry uses the same allowance.
        for _ in range(min(16, self.remaining_requests)):
            if time.monotonic() >= self.deadline:
                break
            try:
                result = self.control('GET', '/health/readyz')
                if result.get('status') == 'ready':
                    return
            except (OSError, Rejected):
                pass
            time.sleep(0.1)
        raise Rejected()

    def grant(self, receipt):
        now = int(time.time())
        header = encode(json.dumps({'alg': 'EdDSA', 'kid': self.config['kid']}, separators=(',', ':')).encode())
        claims = encode(json.dumps({'iss': 'family-stream-api', 'aud': 'family-stream-relay',
            'sub': self.config['owner_id'], 'sid': receipt['session_id'], 'arv': receipt['auth_revision'],
            'iat': now, 'nbf': now, 'exp': now+600, 'jti': 'acceptance-'+uuid.uuid4().hex}, separators=(',', ':')).encode())
        message = (header+'.'+claims).encode()
        # Validate permissions before handing a local key path to OpenSSL. Only
        # this test-owned signer uses the private key; the relay has public keys.
        private_file(self.config['signing_key_file'], 16384)
        with tempfile.TemporaryDirectory(prefix='acceptance-sign-') as directory:
            message_path, signature_path = Path(directory)/'message', Path(directory)/'signature'
            descriptor = os.open(message_path, os.O_CREAT | os.O_EXCL | os.O_WRONLY, 0o600)
            with os.fdopen(descriptor, 'wb') as target:
                target.write(message)
            result = subprocess.run([self.config['openssl'], 'pkeyutl', '-sign', '-rawin',
                '-inkey', self.config['signing_key_file'], '-keyform', 'DER', '-in', str(message_path),
                '-out', str(signature_path)], stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL,
                stderr=subprocess.DEVNULL, timeout=3, check=False)
            if result.returncode or signature_path.stat().st_size != 64:
                raise Rejected()
            return message.decode()+'.'+encode(signature_path.read_bytes())

    def probe(self, receipt, grant):
        seconds = min(30, self.deadline-time.monotonic())
        if seconds <= 0:
            raise Rejected()
        probe = Probe(self.host, self.config['stream_port'], receipt['session_id'], grant,
                      seconds=seconds, requests=min(64, self.stream_requests), byte_limit=min(8*1024*1024, self.stream_bytes))
        if self.config.get('sample_graph') and receipt['format'] == 'hls':
            pending = [resource_path(receipt['entry_path'], receipt['session_id'], grant)]
            seen = set()
            while pending:
                path = pending.pop(0)
                if path in seen:
                    continue
                seen.add(path)
                _, _, body, manifest = probe.fetch(path)
                if manifest:
                    probe.manifests += 1
                    # Prefix access only: deliberately do not certify graph closure.
                    for reference in references(body)[:4]:
                        child = resource_path(reference, receipt['session_id'], grant)
                        if child not in seen and child not in pending and len(pending)+len(seen) < 24:
                            pending.append(child)
                else:
                    probe.samples += 1
            if not probe.manifests or not probe.samples:
                raise Rejected()
            report = {'evidence': 'sampled_relay_byte_access', 'graph_complete': False,
                      'requests': probe.requests, 'bytes_read': probe.bytes,
                      'media_prefixes': probe.samples}
        else:
            report = probe.run(receipt['entry_path'], receipt['format'] == 'mp4')
        self.stream_requests -= probe.requests
        self.stream_bytes -= probe.bytes
        return report


def run(config):
    api = API(config)
    request = json.loads(private_file(config['lookup_file'], 32768))
    provider = OwnedProvider(config) if config.get('provider_command') else None
    receipt = None
    try:
        if provider:
            provider.start(api)
        selection = api.resolve(request)
        receipt = api.control('POST', '/internal/v1/sessions', {
            'contract_version': 'relay.v1', 'owner_id': config['owner_id'],
            'expires_at': time.strftime('%Y-%m-%dT%H:%M:%SZ', time.gmtime(time.time()+900)),
            'selection': selection}, 'acceptance-'+uuid.uuid4().hex)
        grant = api.grant(receipt)
        report = api.probe(receipt, grant)
        actions = []
        if provider and config.get('provider_lifecycle', True):
            provider.stop()
            api.probe(receipt, grant)
            actions.append('provider_outage_existing_session_bytes')
            provider.start(api)
            api.probe(receipt, grant)
            actions.append('provider_restart_existing_session_bytes')
        if config.get('failover'):
            previous = receipt
            receipt = api.control('POST', '/internal/v1/sessions/'+receipt['session_id']+'/failover',
                {'owner_id': config['owner_id'], 'expected_revision': receipt['revision']})
            if not receipt.get('resume_required') or receipt['source_generation'] <= previous['source_generation']:
                raise Rejected()
            api.probe(receipt, api.grant(receipt))
            actions.append('backup_generation_bytes')
        if config.get('refresh'):
            previous = receipt
            request['lookup']['refresh'] = True
            selection = api.resolve(request)
            receipt = api.control('POST', '/internal/v1/sessions/'+receipt['session_id']+'/source',
                {'owner_id': config['owner_id'], 'expected_revision': receipt['revision'], 'selection': selection})
            if not receipt.get('resume_required') or receipt['source_generation'] <= previous['source_generation']:
                raise Rejected()
            api.probe(receipt, api.grant(receipt))
            actions.append('source_refresh_bytes')
        if config.get('restart_file'):
            mailbox = Path(config['restart_file'])
            if json.loads(private_file(mailbox, 1024)) != {'request': 0}:
                raise Rejected()
            write_mailbox(mailbox, {'request': 1})
            for _ in range(350):
                if time.monotonic() >= api.deadline:
                    raise Rejected()
                try:
                    if json.loads(private_file(mailbox, 1024)) == {'completed': 1}:
                        break
                except (ValueError, Rejected):
                    pass
                time.sleep(0.1)
            else:
                raise Rejected()
            api.wait_relay_ready()
            current = api.control('GET', '/internal/v1/sessions/'+receipt['session_id'])
            if current['auth_revision'] != receipt['auth_revision'] or current['source_generation'] != receipt['source_generation']:
                raise Rejected()
            api.probe(receipt, api.grant(receipt))
            actions.append('relay_restart_persisted_session_bytes')
        return {'evidence': 'provider_http_to_production_relay' , 'format': receipt['format'],
                'graph': report, 'actions': actions, 'decoded': False, 'player_verified': False,
                'temporal_seek': False, 'rendered_subtitles': False}
    finally:
        try:
            if receipt is not None:
                api.control('DELETE', '/internal/v1/sessions/'+receipt['session_id'])
        finally:
            if provider:
                provider.stop()
            if config.get('provider_budget_file'):
                seal_provider_allowance(config['provider_budget_file'])


def run_config(config):
    cases = config.get('cases')
    if cases is None:
        with wall_deadline(120):
            return run(config)
    if not isinstance(cases, list) or not 1 <= len(cases) <= 4 or any(not isinstance(case, dict) for case in cases):
        raise Rejected()
    # One manifest client, four sequential cases, one original Go allowance.
    # Individual cases must not seal a shared ledger between replacements.
    if any(case.get('provider_budget_file') for case in cases):
        raise Rejected()
    reports = []
    try:
        for case in cases:
            with wall_deadline(120):
                reports.append(run(case))
        return {'evidence':'bounded_service_acceptance_batch', 'cases':reports,
                'decoded':False, 'player_verified':False}
    finally:
        if config.get('provider_budget_file'):
            seal_provider_allowance(config['provider_budget_file'])


def own_client(argv, seconds):
    # Reuse the corrected owner's unreaped-leader/process-group cleanup. The
    # tempfile contains only the trusted client's fixed aggregate JSON report.
    def interrupted(_signal, _frame):
        raise Rejected()
    previous = signal.signal(signal.SIGTERM, interrupted)
    processes = []
    try:
        with tempfile.TemporaryFile() as output:
            try:
                with wall_deadline(seconds):
                    process = launch(argv, stdout=output)
                    processes.append(process)
                    while exited(process) is None:
                        time.sleep(0.02)
                    if exited(process) != 0:
                        raise Rejected()
            finally:
                stop_all(processes)
            output.seek(0)
            data = output.read(8193)
            if len(data) > 8192:
                raise Rejected()
            report = json.loads(data)
            if report.get('evidence') != 'provider_http_to_production_relay':
                raise Rejected()
            return report
    finally:
        signal.signal(signal.SIGTERM, previous)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--opt-in', required=True, action='store_true')
    parser.add_argument('--config', required=True)
    parser.add_argument('--owned-test-client', action='store_true')
    args = parser.parse_args()
    try:
        if args.owned_test_client:
            private_file(args.config, 32768)
            report = own_client([sys.executable, str(Path(__file__).resolve()), '--opt-in', '--config', args.config], 10)
        else:
            report = run_config(json.loads(private_file(args.config, 32768)))
        print(json.dumps(report, sort_keys=True))
        return 0
    except Exception:
        print('{"evidence":"incomplete","error":"service_acceptance_rejected"}')
        return 1


if __name__ == '__main__':
    raise SystemExit(main())
