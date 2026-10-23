#!/usr/bin/env python3
"""Explicit offline native-player/decoder smoke test. Synthetic localhost only.

Not a production relay or live acceptance result. Generates tiny test media in a
private temporary fixture directory; never stores or transcodes source media.
"""
import argparse
from functools import partial
import http.server
import json
import io
import re
from pathlib import Path
import subprocess
import tempfile
import os
import threading

from playback_acceptance import Player, decode
from acceptance_probe import Rejected


class QuietHandler(http.server.SimpleHTTPRequestHandler):
    entry_format = 'mp4'

    def log_message(self, *_args):
        pass

    def send_head(self):
        path = Path(self.translate_path(self.path))
        if path.name == 'entry':
            path = path.with_suffix('.'+self.entry_format)
        if not path.is_file():
            self.send_error(404)
            return None
        body = path.read_bytes()
        total = len(body)
        range_value = self.headers.get('Range')
        self.send_response(206 if range_value else 200)
        if range_value:
            match = re.fullmatch(r'bytes=(\d+)-(\d*)', range_value)
            if not match:
                raise ValueError('synthetic range syntax')
            start = int(match[1])
            end = min(int(match[2]) if match[2] else total-1, total-1)
            self.send_header('Content-Range', f'bytes {start}-{end}/{total}')
            body = body[start:end+1]
        mime = self.guess_type(str(path))
        if body.startswith(b'#EXTM3U'):
            mime = 'application/vnd.apple.mpegurl'
        elif body.startswith(b'WEBVTT'):
            mime = 'text/vtt'
        elif path.name.startswith('opaque-'):
            mime = 'video/mp2t'
        self.send_header('Content-Type', mime)
        self.send_header('Accept-Ranges', 'bytes')
        self.send_header('Content-Length', str(len(body)))
        self.end_headers()
        return io.BytesIO(body)


def run(ffmpeg, player_binary):
    subprocess.run([str(Path(player_binary).resolve()), '--self-test-timing'],
                   stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, timeout=5, check=True)
    with tempfile.TemporaryDirectory(prefix='playback-synthetic-') as directory:
        root = Path(directory)
        video = root/'stream'/'synthetic'
        video.mkdir(parents=True)
        common = [ffmpeg, '-hide_banner', '-loglevel', 'error', '-f', 'lavfi', '-i',
                  'testsrc=size=320x180:rate=24', '-f', 'lavfi', '-i',
                  'sine=frequency=440:sample_rate=16000', '-t', '8', '-c:v', 'libx264',
                  '-pix_fmt', 'yuv420p', '-g', '24', '-c:a', 'aac']
        subprocess.run(common+['-movflags', '+faststart', str(video/'entry.mp4')],
                       stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
                       timeout=30, check=True)
        subprocess.run(common+['-vf', 'trim=duration=1', '-movflags', '+faststart', str(video/'entry.frozen.mp4')],
                       stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
                       timeout=30, check=True)
        subprocess.run(common+['-an', '-f', 'hls', '-hls_time', '2', '-hls_playlist_type', 'vod', '-hls_segment_filename', str(video/'opaque-video-%d'), str(video/'video.m3u8')],
                       stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
                       timeout=30, check=True)
        subprocess.run([ffmpeg, '-hide_banner', '-loglevel', 'error', '-f', 'lavfi', '-i',
                        'sine=frequency=440:sample_rate=16000', '-t', '8', '-c:a', 'aac',
                        '-f', 'hls', '-hls_time', '2', '-hls_playlist_type', 'vod', '-hls_segment_filename', str(video/'opaque-audio-%d'), str(video/'audio.m3u8')],
                       stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
                       timeout=30, check=True)
        (video/'subtitle.vtt').write_text('WEBVTT\nX-TIMESTAMP-MAP=LOCAL:00:00:00.000,MPEGTS:126000\n\n00:00.000 --> 00:08.000\nSYNTHETIC SUBTITLE\n')
        (video/'subtitle.m3u8').write_text('#EXTM3U\n#EXT-X-TARGETDURATION:8\n#EXT-X-VERSION:3\n#EXT-X-MEDIA-SEQUENCE:0\n#EXT-X-PLAYLIST-TYPE:VOD\n#EXTINF:8,\nsubtitle.vtt\n#EXT-X-ENDLIST\n')
        (video/'entry.m3u8').write_text('#EXTM3U\n#EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID="audio",NAME="Synthetic audio",DEFAULT=YES,AUTOSELECT=YES,URI="audio.m3u8"\n#EXT-X-MEDIA:TYPE=SUBTITLES,GROUP-ID="subs",NAME="Synthetic",DEFAULT=YES,AUTOSELECT=YES,LANGUAGE="en",URI="subtitle.m3u8"\n#EXT-X-STREAM-INF:BANDWIDTH=500000,AUDIO="audio",SUBTITLES="subs"\nvideo.m3u8\n')
        aliases = {'video.m3u8':'opaque-video-playlist', 'audio.m3u8':'opaque-audio-playlist',
                   'subtitle.m3u8':'opaque-subtitle-playlist', 'subtitle.vtt':'opaque-subtitle-resource'}
        for playlist in video.glob('*.m3u8'):
            text = playlist.read_text()
            for old, new in aliases.items():
                text = text.replace(old, new)
            playlist.write_text(text)
        for old, new in aliases.items():
            (video/old).rename(video/new)
        server = http.server.ThreadingHTTPServer(('127.0.0.1', 0), partial(QuietHandler, directory=directory))
        thread = threading.Thread(target=server.serve_forever)
        thread.start()
        reports = []
        try:
            for kind in ('mp4', 'm3u8'):
                client = Player({'player':str(Path(player_binary).resolve()), 'stream_port':server.server_port}, directory)
                QuietHandler.entry_format = kind
                url = 'http://127.0.0.1:'+str(server.server_port)+'/stream/synthetic/entry?st=synthetic'
                try:
                    decoded = decode(str(Path(ffmpeg).resolve()), url, seconds=2)
                    frames = client.play(url, 0, 2)
                    seek = client.play(url, 3, 1, 'seek')
                    saved_position = client.position
                    client.restart()
                    resumed = client.play(url, saved_position, 1)
                    if kind == 'm3u8' and not (frames['rendered_subtitles'] or seek['rendered_subtitles']):
                        raise ValueError('synthetic subtitle overlay did not render')
                    reports.append({'format':kind, 'decode':decoded, 'startup':frames, 'seek':seek, 'client_relaunch_resume':resumed})
                finally:
                    client.close()
            QuietHandler.entry_format = 'frozen.mp4'
            frozen = Player({'player':str(Path(player_binary).resolve()), 'stream_port':server.server_port}, directory)
            try:
                try:
                    frozen.play(url, 0, 7)
                    raise ValueError('trailing video freeze was accepted')
                except Rejected:
                    report = json.loads(os.pread(frozen.output.fileno(), 16384, 0).splitlines()[-1])
                    if (report.get('event') != 'played' or report['frames_observed'] < 7
                            or report['position_s'] < 6 or report['max_frame_gap_s'] <= 5):
                        raise ValueError('unexpected failure instead of trailing video freeze')
            finally:
                frozen.close()
        finally:
            server.shutdown()
            server.server_close()
            thread.join()
        return {'evidence':'synthetic_player_decoder_only', 'cases':reports, 'trailing_freeze_rejected':True}


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--ffmpeg', required=True)
    parser.add_argument('--player', required=True)
    options = parser.parse_args()
    print(json.dumps(run(options.ffmpeg, options.player), sort_keys=True))
