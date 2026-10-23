#!/usr/bin/env python3
"""Prepare offline, then explicitly opt in to one approved local playback run.

Fixed plan in docs/http-integration-live-plan.md. No allowances exist at prepare;
run creates them once. A run directory is single use, including failed attestation.
"""
import argparse
import base64
import hashlib
import json
import os
from pathlib import Path
import shutil
import signal
from contextlib import contextmanager
import socket
import subprocess
import sys
import tempfile
import time

from acceptance_probe import Rejected, private_file
from acceptance_run import (initialize, status, request_status, stop_ledger,
                            seal_provider_allowance, supervise, launch, exited, stop_all)

SECONDS = 1800
RUST_BYTES = 1536*1024*1024
RUST_REQUESTS = 8192
GO_BYTES = 384*1024*1024
GO_REQUESTS = 512
REPO = Path(__file__).resolve().parent.parent
PROVIDERS = REPO.parent/'family-stream-providers'


def write(path, value):
    data = value if isinstance(value, bytes) else json.dumps(value).encode()
    descriptor = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
    with os.fdopen(descriptor, 'wb') as output:
        output.write(data)


def b64(data):
    return base64.urlsafe_b64encode(data).rstrip(b'=').decode('ascii')


def digest(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def prepare(player, ffmpeg, openssl):
    root = Path(tempfile.mkdtemp(prefix='family-stream-playback-'))
    try:
        secret = root/'secrets'
        state = root/'state'
        secret.mkdir(mode=0o700)
        state.mkdir(mode=0o700)
        binaries = {'player':str(Path(player).resolve()), 'ffmpeg':str(Path(ffmpeg).resolve()),
                    'openssl':str(Path(openssl).resolve()),
                    'providers':str(PROVIDERS/'.local/acceptance/providers'),
                    'providers_netcheck':str(PROVIDERS/'.local/acceptance/providers-netcheck'),
                    'relay':str(REPO/'target/debug/family-stream-relay'),
                    'relay_netcheck':str(REPO/'target/debug/relay-netcheck')}
        hashes = {name:digest(path) for name, path in binaries.items()}
        key = secret/'signer.der'
        subprocess.run([binaries['openssl'], 'genpkey', '-algorithm', 'ED25519', '-outform', 'DER', '-out', str(key)],
                       stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, timeout=5, check=True)
        key.chmod(0o600)
        public = subprocess.run([binaries['openssl'], 'pkey', '-inform', 'DER', '-in', str(key), '-pubout', '-outform', 'DER'],
                                stdin=subprocess.DEVNULL, capture_output=True, timeout=5, check=True).stdout
        # RFC 8410 Ed25519 SubjectPublicKeyInfo has this exact fixed prefix.
        if public[:12] != bytes.fromhex('302a300506032b6570032100') or len(public) != 44:
            raise Rejected()
        write(secret/'public.json', {'keys':[{'kid':'acceptance', 'public_key':b64(public[12:])}]})
        write(secret/'encryption.json', {'active_key_id':'acceptance-key-01',
            'keys':[{'kid':'acceptance-key-01', 'key':b64(os.urandom(32))}]})
        for name in ('provider-bearer', 'control-bearer'):
            write(secret/name, b64(os.urandom(32)).encode())
        write(root/'providers.json', {'listen':'127.0.0.1:18090',
            'bearer_file':str(secret/'provider-bearer'), 'dns_address':'1.1.1.1:53',
            'acceptance_budget_file':str(root/'go.jsonl')})
        config = (REPO/'config/relay.example.toml').read_text()
        substitutions = {'127.0.0.1:8091':'127.0.0.1:18091', '127.0.0.1:8092':'127.0.0.1:18092',
            '/run/secrets/relay-control-bearer':str(secret/'control-bearer'),
            '/run/secrets/api-playback-public-keys.json':str(secret/'public.json'),
            '/run/secrets/relay-session-encryption-key':str(secret/'encryption.json'),
            'state/relay.sqlite':str(state/'relay.sqlite')}
        for old, new in substitutions.items():
            config = config.replace(old, new)
        write(root/'relay.toml', config.encode())
        write(root/'restart.json', {'request':0})
        cases = []
        titles = [('movie', {'type':'movie','tmdb_id':550}, 'hls', [600,3600]),
                  ('tv', {'type':'tv','tmdb_id':60625,'season':1,'episode':1}, 'hls', [600,1200]),
                  ('anime', {'type':'tv','tmdb_id':1429,'season':1,'episode':1,'my_anime_list_id':16498}, 'hls', [600,1200]),
                  ('mp4', {'type':'movie','tmdb_id':550}, 'mp4', [600,3600])]
        for index, (label, title, format_name, positions) in enumerate(titles):
            lookup = root/(label+'-lookup.json')
            write(lookup, {'version':'providers.v1', 'context_key':'owned-acceptance-household',
                'lookup':{'title':title, 'preferences':{},
                          'device':{'formats':[format_name], 'codecs':['avc1','mp4a'], 'max_height':720}}})
            case = {'provider_port':18090, 'control_port':18091, 'stream_port':18092,
                'provider_command':[binaries['providers'], str(root/'providers.json')],
                'provider_attest_command':[binaries['providers_netcheck'], str(root/'go.jsonl')],
                'provider_bearer_file':str(secret/'provider-bearer'), 'control_bearer_file':str(secret/'control-bearer'),
                'lookup_file':str(lookup), 'owner_id':'acceptance-owner-01', 'kid':'acceptance',
                'signing_key_file':str(key), 'openssl':binaries['openssl'], 'expected_format':format_name,
                'seek_positions':positions, 'provider_lifecycle':True,
                'player':binaries['player'], 'ffmpeg':binaries['ffmpeg']}
            case['restart_file'] = str(root/'restart.json')
            case['restart_sequence'] = index+1
            cases.append(case)
        write(root/'report.json', {})
        write(root/'playback.json', {'cases':cases, 'provider_budget_file':str(root/'go.jsonl'),
                                   'report_file':str(root/'report.json'), 'owner_seals_provider':True})
        heads = {name:subprocess.check_output(['git','rev-parse','HEAD'], cwd=repo, text=True).strip()
                 for name, repo in (('providers',PROVIDERS),('relay',REPO))}
        sources = {str(path.relative_to(REPO)):digest(path) for path in (REPO/'tools').glob('*.py')}
        sources['tools/playback_player.swift'] = digest(REPO/'tools/playback_player.swift')
        write(root/'prepared.json', {'binaries':binaries, 'hashes':hashes, 'heads':heads, 'sources':sources})
        return root
    except Exception:
        shutil.rmtree(root)
        raise


@contextmanager
def shutdown_owner():
    interrupted = False
    def request_stop(_signum, _frame):
        nonlocal interrupted
        # A flag cannot interrupt launch before the new child is registered.
        interrupted = True
    def check():
        if interrupted:
            raise Rejected()
    previous = {number:signal.signal(number, request_stop) for number in (signal.SIGINT,signal.SIGTERM)}
    try:
        yield check
    finally:
        for number, handler in previous.items():
            signal.signal(number, handler)


def attest(argv, seconds, check=lambda: None):
    processes = []
    try:
        check()
        with tempfile.TemporaryFile() as output:
            process = launch(argv, stdout=output, stderr=output)
            processes.append(process)
            deadline = time.monotonic()+seconds
            while exited(process) is None:
                check()
                if time.monotonic() >= deadline:
                    raise Rejected()
                time.sleep(.02)
            # Diagnostics emit only their fixed family outcomes. Do not copy raw
            # output; the per-family assertions below are independently allowlisted.
            data = os.pread(output.fileno(), 2049, 0)
            ipv4 = b'ip4: guarded_transport_mullvad_exit=true' in data or b'ipv4: guarded_transport_mullvad_exit=true' in data
            ipv6 = b'ip6: guarded_transport_mullvad_exit=true' in data or b'ipv6: guarded_transport_mullvad_exit=true' in data
            categories = ['guard setup unavailable', 'guarded client unavailable', 'attestation request rejected',
                          'egress not attested', 'upstream_Guard', 'upstream_Dns', 'upstream_Connect',
                          'upstream_Tls', 'upstream_Timeout', 'upstream_Protocol', 'upstream_Limit']
            return {'ipv4':ipv4, 'ipv6':ipv6, 'passed':exited(process) == 0 and ipv4 and ipv6,
                    'failure_categories':[category for category in categories if category.encode() in data]}
    finally:
        stop_all(processes)


def execute(root):
    with shutdown_owner() as check:
        return execute_owned(root, check)


def execute_owned(root, check):
    root = Path(root)
    prepared = json.loads(private_file(root/'prepared.json', 65536))
    binaries = prepared['binaries']
    if any(digest(binaries[name]) != expected for name, expected in prepared['hashes'].items()):
        raise Rejected()
    if any(digest(REPO/path) != expected for path, expected in prepared['sources'].items()):
        raise Rejected()
    # Reserve ports offline; never displace another user's process.
    sockets = []
    try:
        for port in (18090,18091,18092):
            listener = socket.socket()
            sockets.append(listener)
            listener.bind(('127.0.0.1', port))
    finally:
        for listener in sockets:
            listener.close()
    check()
    write(root/'run-started', b'single use')
    rust_created = go_created = False
    report = {'evidence':'incomplete', 'heads':prepared['heads'],
              'executable_sha256':prepared['hashes'], 'tool_sha256':prepared['sources'],
              'overall_acceptance':False}
    try:
        rust_created = True
        initialize(root/'rust.sqlite', RUST_BYTES, SECONDS, RUST_REQUESTS)
        go_created = True
        write(root/'go.jsonl', (json.dumps({'remaining_bytes':GO_BYTES, 'remaining_requests':GO_REQUESTS,
              'deadline_ms':status(root/'rust.sqlite')[2], 'stopped':False})+'\n').encode())
        check()
        report['go_attestation'] = attest([binaries['providers_netcheck'], str(root/'go.jsonl')], 25, check)
        if not report['go_attestation']['passed']:
            return report
        report['rust_attestation'] = attest([binaries['relay_netcheck'], str(root/'relay.toml'),
                                            '--acceptance-budget', str(root/'rust.sqlite')], 45, check)
        if not report['rust_attestation']['passed']:
            return report
        check()
        report['evidence'] = 'bounded_live_playback_attempt'
        report['owner'] = supervise(root/'rust.sqlite',
            [binaries['relay'], str(root/'relay.toml'), '--acceptance-budget', str(root/'rust.sqlite')],
            [[sys.executable, str(REPO/'tools/playback_acceptance.py'), '--opt-in', '--config', str(root/'playback.json')]],
            restarts=4, restart_file=root/'restart.json', provider_budget_file=root/'go.jsonl', cancellation_check=check)
        report['playback'] = json.loads(private_file(root/'report.json', 32768))
        return report
    except Exception:
        report['run_error'] = 'operator_rejected'
        return report
    finally:
        # All diagnostic/supervised processes have already been stopped and
        # joined. A ledger failure must not skip the other ledger or cleanup.
        try:
            if rust_created:
                try:
                    stop_ledger(root/'rust.sqlite')
                    limit, remaining, _, stopped = status(root/'rust.sqlite')
                    quota = request_status(root/'rust.sqlite')
                    report['rust_accounting'] = {'reserved_receive_bytes':limit-remaining,
                        'http_exchanges':quota[0]-quota[1], 'byte_limit':limit, 'stopped':bool(stopped)}
                except Exception:
                    report['rust_accounting'] = {'available':False,'seal_confirmed':False}
            if go_created:
                try:
                    seal_provider_allowance(root/'go.jsonl')
                    ledger = json.loads(private_file(root/'go.jsonl', 1024*1024).splitlines()[-1])
                    report['go_accounting'] = {'reserved_response_allowance_bytes':GO_BYTES-ledger['remaining_bytes'],
                        'http_doh_requests':GO_REQUESTS-ledger['remaining_requests'], 'byte_limit':GO_BYTES,
                        'stopped':ledger['stopped']}
                except Exception:
                    report['go_accounting'] = {'available':False,'seal_confirmed':False}
        finally:
            owned = ['secrets','state','providers.json','relay.toml','restart.json','prepared.json',
                     'report.json','playback.json','movie-lookup.json','tv-lookup.json','anime-lookup.json','mp4-lookup.json']
            failures = []
            for name in owned:
                child = root/name
                try:
                    if child.is_dir() and not child.is_symlink():
                        shutil.rmtree(child)
                    else:
                        child.unlink(missing_ok=True)
                except Exception:
                    failures.append(name)  # fixed owned filenames, no private values
            report['private_cleanup'] = {'complete':not failures,'failed_owned_entries':failures}
            write(root/'outcome.json', report)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('action', choices=['prepare','run'])
    parser.add_argument('--player')
    parser.add_argument('--ffmpeg')
    parser.add_argument('--openssl')
    parser.add_argument('--directory')
    parser.add_argument('--opt-in', action='store_true')
    args = parser.parse_args()
    try:
        if args.action == 'prepare':
            print(json.dumps({'private_run_directory':str(prepare(args.player,args.ffmpeg,args.openssl)), 'live_requests':0}))
        else:
            if not args.opt_in:
                raise Rejected()
            report = execute(args.directory)
            print(json.dumps(report, sort_keys=True))
            completed = (report.get('owner', {}).get('stop_reason') == 'clients_complete'
                         and report.get('private_cleanup', {}).get('complete') is True
                         and report.get('rust_accounting', {}).get('stopped') is True
                         and report.get('go_accounting', {}).get('stopped') is True
                         and 'run_error' not in report)
            return 0 if completed else 1
        return 0
    except Exception:
        print('{"evidence":"incomplete","error":"playback_operator_rejected"}')
        return 1


if __name__ == '__main__':
    raise SystemExit(main())
