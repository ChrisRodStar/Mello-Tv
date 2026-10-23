#!/usr/bin/env python3
"""Owned decoder/native-player checks; only production loopback stream URLs.

Always invoke as a manifest client of acceptance_run.py. No upstream transport,
media storage, browser attachment, format substitution or allowance creation.
"""
import argparse
import json
import os
from pathlib import Path
import select
import subprocess
import tempfile
import time
from urllib.parse import urlsplit, parse_qs

from acceptance_probe import Probe, Rejected, private_file, resource_path, wall_deadline
from acceptance_run import write_mailbox
from service_acceptance import API, OwnedProvider, seal_provider_allowance

SANDBOX = '(version 1)(allow default)(deny network-outbound)(allow network-outbound (remote ip "localhost:*"))'


def entry_url(api, receipt):
    grant = api.grant(receipt)
    return 'http://127.0.0.1:' + str(api.config['stream_port']) + resource_path(
        receipt['entry_path'], receipt['session_id'], grant)


def existing_entry(api, receipt, url):
    grant = parse_qs(urlsplit(url).query)['st'][0]
    probe = Probe(api.host, api.config['stream_port'], receipt['session_id'], grant,
                  seconds=min(10, api.deadline-time.monotonic()), requests=1, byte_limit=1048577)
    probe.fetch(receipt['entry_path'], 'HEAD' if receipt['format'] == 'mp4' else 'GET')
    return {'requests':probe.requests, 'body_bytes':probe.bytes}


def decode(ffmpeg, url, position=0, seconds=10):
    # Concat demuxer accepts the private URL through stdin, including for MP4.
    # No file protocol is admitted, so a manifest cannot read local secrets.
    # inpoint invokes demuxer temporal seeking, not a byte-range sample.
    target = urlsplit(url)
    if (target.scheme != 'http' or target.hostname != '127.0.0.1'
            or target.username or target.password or target.fragment or not target.port
            or not target.path.startswith('/stream/') or any(value in url for value in "'\r\n")
            or not 0 <= position <= 7200 or not 1 <= seconds <= 30):
        raise Rejected()
    playlist = ("ffconcat version 1.0\nfile '" + url + "'\noption allowed_extensions ALL\n"
                + ('inpoint '+str(position)+'\n' if position else '')).encode()
    common = ['/usr/bin/sandbox-exec', '-p', SANDBOX, ffmpeg, '-hide_banner',
              '-loglevel', 'error', '-nostdin', '-protocol_whitelist', 'pipe,http,tcp,crypto',
              '-f', 'concat', '-safe', '0', '-i', 'pipe:0', '-t', str(seconds)]
    # Bounded stdout is decoded PCM or FFmpeg's fixed numeric progress vocabulary.
    report = {}
    for mode in ('video', 'audio'):
        args = (['-map', '0:v:0', '-an', '-frames:v', str(120*seconds), '-progress', 'pipe:1', '-f', 'null', '-']
                if mode == 'video' else ['-map', '0:a:0', '-vn', '-ac', '1', '-ar', '8000',
                                        '-f', 's16le', '-fs', str(seconds*16000), 'pipe:1'])
        process = subprocess.Popen(common + args, stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                   stderr=subprocess.DEVNULL)
        try:
            process.stdin.write(playlist)
            process.stdin.close()
            output = bytearray()
            deadline = time.monotonic() + 45
            while True:
                if time.monotonic() >= deadline:
                    raise Rejected()
                ready, _, _ = select.select([process.stdout], [], [], min(.1, deadline-time.monotonic()))
                if not ready:
                    continue
                chunk = os.read(process.stdout.fileno(), 16384)
                if not chunk:
                    break
                output.extend(chunk)
                if len(output) > seconds*16000+16384:
                    raise Rejected()
            if process.wait(timeout=1) != 0:
                raise Rejected()
            if mode == 'audio':
                if len(output) < (seconds-1)*16000 or len(output) % 2:
                    raise Rejected()
                report.update(decoded_audio_s=len(output)/16000, nonzero_pcm=any(output))
            else:
                values = dict(line.split('=', 1) for line in output.decode('ascii').splitlines() if '=' in line)
                frames = int(values.get('frame', '0'))
                elapsed = int(values.get('out_time_us', '0'))/1000000
                if frames < seconds or elapsed < seconds-1 or values.get('progress') != 'end':
                    raise Rejected()
                report.update(decoded_video_frames=frames, decoded_video_s=elapsed)
        finally:
            if process.poll() is None:
                process.terminate()
                try:
                    process.wait(timeout=.15)
                except subprocess.TimeoutExpired:
                    process.kill()
                    process.wait(timeout=3)
            process.stdout.close()
    report['owned_decoder_processes_joined'] = 2
    return report


class Player:
    def __init__(self, config, directory):
        self.path = Path(directory)/'player-command.json'
        self.path.touch(mode=0o600)
        self.config = config
        self.sequence = 0
        self.process = None
        self.output = None
        self.position = 0

    def play(self, url, position, seconds, action='load'):
        self.sequence += 1
        write_mailbox(self.path, dict(sequence=self.sequence, url=url, position=position,
                                     seconds=seconds, action=action))
        if self.process is None:
            self.output = tempfile.TemporaryFile()
            self.process = subprocess.Popen(['/usr/bin/sandbox-exec', '-p', SANDBOX,
                                   self.config['player'], str(self.path), str(self.config['stream_port'])],
                                   stdin=subprocess.DEVNULL, stdout=self.output, stderr=subprocess.DEVNULL)
        deadline = time.monotonic()+seconds+40
        # Only the fixed safe report is ever retained, no capture/cue text.
        while time.monotonic() < deadline:
            data = os.pread(self.output.fileno(), 16385, 0)
            if len(data) > 16384 or self.process.poll() is not None:
                raise Rejected()
            for line in data.splitlines(keepends=True):
                if not line.endswith(b"\n"):
                    continue
                report = json.loads(line)
                if report.get('event') != 'played':
                    raise Rejected()
                if report.get('sequence') != self.sequence:
                    continue
                actual = report['position_s']
                baseline = report['started_s'] if action == 'observe' else position
                if (not report['seek_completed'] or not report['view_ready'] or report['frames_observed'] < seconds
                        or report.get('max_frame_gap_s', 0) > 5
                        or not baseline+seconds-2 <= actual <= baseline+seconds+10):
                    raise Rejected()
                self.position = actual
                return report
            time.sleep(.05)
        raise Rejected()

    def restart(self):
        # A terminal AVPlayer error may have exited the helper during drain.
        # Join it and create a fresh owned client; retain measured media time.
        self.close()
        self.process = None
        self.output = None
        self.sequence = 0
        self.path.touch(mode=0o600, exist_ok=False)

    def close(self):
        try:
            if self.process is not None:
                self.process.terminate()
                try:
                    self.process.wait(timeout=.15)
                except subprocess.TimeoutExpired:
                    self.process.kill()
                    self.process.wait(timeout=3)
        finally:
            if self.output is not None:
                self.output.close()
            self.path.unlink(missing_ok=True)


def run_case(config):
    # Config contains trusted executable/private-file paths, never source URLs.
    api = API(config, seconds=480, requests=64)
    request = json.loads(private_file(config['lookup_file'], 32768))
    provider = OwnedProvider(config)
    receipt = None
    reports = []
    decoded = None
    recovery = []
    outage_access = []
    cleanup = []
    stage = 'provider_start_attestation'
    with tempfile.TemporaryDirectory(prefix='owned-playback-') as directory:
        player = Player(config, directory)
        def play_at(position, seconds, action='load'):
            observation = player.play(url, position, seconds, action)
            observation.update(phase=stage, format=receipt['format'],
                               source_generation=receipt['source_generation'])
            reports.append(observation)
        try:
            provider.start(api)
            stage = 'provider_resolution'
            selection = api.resolve(request)
            stage = 'session_import'
            receipt = api.control('POST', '/internal/v1/sessions', {
                'contract_version':'relay.v1', 'owner_id':config['owner_id'],
                'expires_at':time.strftime('%Y-%m-%dT%H:%M:%SZ', time.gmtime(time.time()+900)),
                'selection':selection}, 'playback-'+os.urandom(16).hex())
            expected = config['expected_format']
            if receipt['format'] != expected:
                raise Rejected()
            url = entry_url(api, receipt)
            stage = 'decode'
            decoded = decode(config['ffmpeg'], url)
            stage = 'startup_sustained'
            play_at(0, 120)
            stage = 'temporal_seek'
            for position in config['seek_positions']:
                play_at(position, 15, 'seek')
            if config.get('provider_lifecycle'):
                stage = 'provider_outage'
                provider.stop()
                outage_access.append(existing_entry(api, receipt, url))
                play_at(player.position, 10, 'observe')
                stage = 'provider_restart_attestation'
                provider.start(api)  # production attestation before new resolution
                outage_access.append(existing_entry(api, receipt, url))
                play_at(player.position, 10, 'observe')
            stage = 'explicit_backup'
            # Preserve provider ranking. Unavailable backup is explicitly reported.
            if selection.get('backups'):
                previous = receipt
                receipt = api.control('POST', '/internal/v1/sessions/'+receipt['session_id']+'/failover',
                    {'owner_id':config['owner_id'], 'expected_revision':receipt['revision']})
                if not receipt.get('resume_required') or receipt['source_generation'] <= previous['source_generation']:
                    raise Rejected()
                url = entry_url(api, receipt)
                play_at(player.position, 10)
                recovery.append('backup_reload_resume')
            else:
                recovery.append('backup_unavailable')
            stage = 'source_refresh'
            request['lookup']['refresh'] = True
            selection = api.resolve(request)
            previous = receipt
            receipt = api.control('POST', '/internal/v1/sessions/'+receipt['session_id']+'/source',
                {'owner_id':config['owner_id'], 'expected_revision':receipt['revision'], 'selection':selection})
            if not receipt.get('resume_required') or receipt['source_generation'] <= previous['source_generation']:
                raise Rejected()
            url = entry_url(api, receipt)
            play_at(player.position, 10)
            recovery.append('source_refresh_reload_resume')
            if config.get('restart_file'):
                stage = 'relay_restart'
                restart_sequence = config.get('restart_sequence', 1)
                write_mailbox(config['restart_file'], {'request':restart_sequence})
                end = min(api.deadline, time.monotonic()+35)
                while json.loads(private_file(config['restart_file'], 1024)) != {'completed':restart_sequence}:
                    if time.monotonic() >= end:
                        raise Rejected()
                    time.sleep(.1)
                api.wait_relay_ready()
                current = api.control('GET', '/internal/v1/sessions/'+receipt['session_id'])
                if any(current[name] != receipt[name] for name in ('auth_revision','source_generation','entry_path')):
                    raise Rejected()
                player.restart()
                play_at(player.position, 10)
                recovery.append('relay_restart_persisted_reload_resume')
                recovery.append('native_client_relaunched')
            return {'evidence':'owned_playback', 'format':expected, 'decode':decoded,
                    'player':reports, 'recovery':recovery, 'outage_entry_access':outage_access, 'cleanup':cleanup,
                    'rendered_subtitles':any(item.get('rendered_subtitles') for item in reports),
                    'spoken_language':'unknown'}
        except Exception:
            return {'evidence':'incomplete', 'stage':stage, 'decode':decoded,
                    'player':reports, 'recovery':recovery, 'outage_entry_access':outage_access, 'cleanup':cleanup, 'error':'playback_case_rejected'}
        finally:
            try:
                player.close()
                cleanup.append('player_joined')
            finally:
                try:
                    if receipt is not None and time.monotonic() < api.deadline:
                        try:
                            api.control('DELETE', '/internal/v1/sessions/'+receipt['session_id'])
                            cleanup.append('session_revoked')
                        except Exception:
                            cleanup.append('session_revoke_failed_isolated_store_cleanup_required')
                finally:
                    provider.stop()
                    cleanup.append('provider_joined')


def run(config):
    cases = config['cases']
    if not isinstance(cases, list) or not 1 <= len(cases) <= 4:
        raise Rejected()
    results = []
    try:
        for case in cases:
            try:
                with wall_deadline(480):
                    result = run_case(case)
                    results.append(result)
                    if result.get('evidence') != 'owned_playback':
                        break
            except Exception:
                # Stop the batch on failure; never replace a title or allowance.
                results.append({'evidence':'incomplete', 'error':'playback_case_rejected'})
                break
        return {'evidence':'owned_playback_batch', 'cases':results,
                'overall_acceptance':False}
    finally:
        if not config.get('owner_seals_provider'):
            seal_provider_allowance(config['provider_budget_file'])


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--opt-in', action='store_true', required=True)
    parser.add_argument('--config', required=True)
    args = parser.parse_args()
    try:
        config = json.loads(private_file(args.config, 32768))
        report = run(config)
        if config.get('report_file'):
            write_mailbox(config['report_file'], report)
        print(json.dumps(report, sort_keys=True))
        return 0 if all(case.get('evidence') == 'owned_playback' for case in report['cases']) else 1
    except Exception:
        print('{"evidence":"incomplete","error":"owned_playback_rejected"}')
        return 1


if __name__ == '__main__':
    raise SystemExit(main())
