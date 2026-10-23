"""Offline tests; loopback fixtures never reach source hosts."""
import http.server
import os
import tempfile
import time
import threading
import unittest
from unittest.mock import patch

from acceptance_probe import Probe, Rejected, private_file, resource_path, wall_deadline


class Fixture(http.server.BaseHTTPRequestHandler):
    mode = "hls"
    paths = []

    def log_message(self, *args):
        pass

    def do_HEAD(self):
        self.send_response(200)
        self.send_header("Content-Length", "100000")
        self.end_headers()

    def do_GET(self):
        Fixture.paths.append(self.path)
        if Fixture.mode == "slow_headers":
            time.sleep(0.3)
        if Fixture.mode == "redirect":
            self.send_response(302)
            self.send_header("Location", "https://upstream.invalid/private")
            self.end_headers()
            return
        if Fixture.mode == "mp4":
            start, end = map(int, self.headers["Range"][6:].split("-"))
            body = b"m" * (end - start + 1)
            self.send_response(206)
            self.send_header("Content-Range", f"bytes {start}-{end}/100000")
            mime = "video/mp4"
        elif self.path.startswith("/stream/sid/entry"):
            body = b'#EXTM3U\n#EXT-X-MEDIA:TYPE=AUDIO,URI="/stream/sid/r/audio?st=grant"\n/stream/sid/r/video?st=grant\n'
            if Fixture.mode == "escape":
                body += b"https://upstream.invalid/private\n"
            if Fixture.mode == "oversized":
                body += b"#" * 1048576
            mime = "application/vnd.apple.mpegurl"
            self.send_response(200)
        else:
            body = b"synthetic-media-prefix"
            mime = "video/mp2t"
            self.send_response(200)
        self.send_header("Content-Type", mime)
        self.send_header("Content-Length", str(len(body) + (100 if Fixture.mode == "early_close" else 0)))
        self.end_headers()
        try:
            self.wfile.write(body)
        except (BrokenPipeError, ConnectionResetError):
            pass


class AcceptanceTests(unittest.TestCase):
    def setUp(self):
        Fixture.paths = []
        Fixture.mode = "hls"
        self.server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Fixture)
        self.thread = threading.Thread(target=self.server.serve_forever)
        self.thread.start()
        self.addCleanup(self.close)

    def close(self):
        self.server.shutdown()
        self.server.server_close()
        self.thread.join()

    def probe(self, **kwargs):
        return Probe("127.0.0.1", self.server.server_port, "sid", "grant", **kwargs)

    def test_graph_fetches_audio_and_video_without_decoding_claim(self):
        result = self.probe().run("/stream/sid/entry", False)
        self.assertEqual(result["requests"], 3)
        self.assertEqual(result["media_prefixes_or_ranges"], 2)
        self.assertFalse(result["decoded"])

    def test_mp4_start_middle_end_are_exact_byte_ranges(self):
        Fixture.mode = "mp4"
        result = self.probe().run("/stream/sid/entry", True)
        self.assertEqual(result["requests"], 4)
        self.assertEqual(result["media_prefixes_or_ranges"], 3)

    def test_upstream_reference_and_redirect_never_followed(self):
        for mode in ("escape", "redirect"):
            Fixture.mode = mode
            with self.assertRaises(Rejected):
                self.probe().run("/stream/sid/entry", False)
        self.assertTrue(all(path.startswith("/stream/sid/") for path in Fixture.paths))

    def test_truncated_manifest_cannot_certify_graph(self):
        Fixture.mode = "oversized"
        with self.assertRaises(Rejected):
            self.probe().run("/stream/sid/entry", False)

    def test_early_close_with_complete_reference_cannot_certify_graph(self):
        Fixture.mode = "early_close"
        with self.assertRaises(Rejected):
            self.probe().run("/stream/sid/entry", False)
        self.assertEqual(len(Fixture.paths), 1)

    def test_request_byte_and_deadline_budgets(self):
        for kwargs in ({"requests": 1}, {"byte_limit": 10}, {"seconds": 0}):
            with self.assertRaises(Rejected):
                self.probe(**kwargs).run("/stream/sid/entry", False)
        probe = self.probe(seconds=1)
        with patch("acceptance_probe.time.monotonic", return_value=probe.deadline + 1):
            with self.assertRaises(Rejected):
                probe.fetch("/stream/sid/entry")

    def test_uri_credentials_and_session_isolation(self):
        for path in ("//evil/stream/sid/entry", "http://127.0.0.1/stream/sid/entry",
                     "/stream/other/entry", "/stream/sid/../entry",
                     "/stream/sid/entry?st=wrong", "/stream/sid/entry?st=grant&st=grant",
                     "/stream/sid/entry#fragment", "/stream/sid/r/%2e%2e"):
            with self.assertRaises(Rejected):
                resource_path(path, "sid", "grant")
        with self.assertRaises(Rejected):
            Probe("8.8.8.8", 8092, "sid", "grant")

    def test_absolute_deadline_interrupts_blocking_header_parser(self):
        Fixture.mode = "slow_headers"
        began = time.monotonic()
        with self.assertRaises(Rejected):
            with wall_deadline(0.05):
                self.probe().run("/stream/sid/entry", False)
        self.assertLess(time.monotonic() - began, 0.25)

    def test_private_inputs_reject_symlinks_permissions_and_oversize(self):
        with tempfile.TemporaryDirectory() as directory:
            path = os.path.join(directory, "secret")
            with open(path, "wb") as out:
                out.write(b"secret")
            os.chmod(path, 0o600)
            self.assertEqual(private_file(path, 6), b"secret")
            with self.assertRaises(Rejected):
                private_file(path, 5)
            os.symlink(path, path + "-link")
            with self.assertRaises(OSError):
                private_file(path + "-link", 6)
            os.chmod(path, 0o644)
            with self.assertRaises(Rejected):
                private_file(path, 6)


if __name__ == "__main__":
    unittest.main()
