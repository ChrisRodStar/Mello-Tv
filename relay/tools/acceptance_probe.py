#!/usr/bin/env python3
"""Opt-in byte-access probe of a production loopback relay; never a media proxy.

Run with --help. Reports only aggregate synthetic/source byte evidence, never
URLs, grants, response bodies, codec/language claims or player acceptance.
"""
import argparse
from contextlib import contextmanager
import http.client
import ipaddress
import json
import os
import re
import signal
import stat
import time
from urllib.parse import parse_qsl, urlencode, urlsplit


class Rejected(Exception):
    """Intentionally carries no sensitive context."""


@contextmanager
def wall_deadline(seconds):
    # Socket idle waits alone do not bound a slow-drip status/header/chunk line.
    # This CLI is single-threaded on supported Unix hosts; interrupt all blocking
    # parsing as well as body reads at one absolute process deadline.
    def expired(_signum, _frame):
        raise Rejected()
    previous = signal.signal(signal.SIGALRM, expired)
    signal.setitimer(signal.ITIMER_REAL, seconds)
    try:
        yield
    finally:
        signal.setitimer(signal.ITIMER_REAL, 0)
        signal.signal(signal.SIGALRM, previous)


def private_file(path, limit):
    fd = os.open(path, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK)
    with os.fdopen(fd, "rb") as source:
        info = os.fstat(source.fileno())
        if not stat.S_ISREG(info.st_mode) or stat.S_IMODE(info.st_mode) != 0o600:
            raise Rejected()
        data = source.read(limit + 1)
        if not data or len(data) > limit:
            raise Rejected()
        return data


def resource_path(value, session, grant):
    # Do not resolve arbitrary URI references, normalize paths or follow redirects.
    # Production manifests must contain only opaque same-session relay paths.
    uri = urlsplit(value)
    if uri.scheme or uri.netloc or uri.fragment:
        raise Rejected()
    prefix = f"/stream/{session}/"
    if not re.fullmatch(re.escape(prefix) + r"(?:entry|r/[A-Za-z0-9_-]+)", uri.path):
        raise Rejected()
    if uri.query and parse_qsl(uri.query, keep_blank_values=True) != [("st", grant)]:
        raise Rejected()
    return uri.path + "?" + urlencode({"st": grant})


def references(body):
    text = body.decode("utf-8")
    if not text.startswith("#EXTM3U\n"):
        raise Rejected()
    refs = []
    for line in text.splitlines():
        if line and not line.startswith("#"):
            refs.append(line)
        elif line.startswith("#"):
            # Production parser owns tag semantics; this checks fetch URI closure.
            refs.extend(re.findall(r'(?:[:,])URI="([^"\r\n]*)"', line))
    return refs


class Probe:
    def __init__(self, host, port, session, grant, seconds=120, requests=64,
                 byte_limit=8 * 1024 * 1024):
        if not ipaddress.ip_address(host).is_loopback or not 1 <= port <= 65535:
            raise Rejected()
        if not re.fullmatch(r"[A-Za-z0-9_-]+", session):
            raise Rejected()
        if not re.fullmatch(r"[A-Za-z0-9_.-]{1,4096}", grant):
            raise Rejected()
        self.host, self.port, self.session, self.grant = host, port, session, grant
        self.deadline = time.monotonic() + seconds
        self.max_requests, self.max_bytes = requests, byte_limit
        self.requests = self.bytes = self.manifests = self.samples = 0

    def remaining(self):
        seconds = self.deadline - time.monotonic()
        if seconds <= 0:
            raise Rejected()
        return min(10, seconds)

    def fetch(self, path, method="GET", byte_range=None):
        path = resource_path(path, self.session, self.grant)
        if self.requests >= self.max_requests:
            raise Rejected()
        self.requests += 1
        conn = http.client.HTTPConnection(self.host, self.port, timeout=self.remaining())
        try:
            headers = {"Connection": "close"}
            if byte_range is not None:
                headers["Range"] = byte_range
            conn.request(method, path, headers=headers)
            socket = conn.sock
            socket.settimeout(self.remaining())
            response = conn.getresponse()
            if response.status not in (200, 206):
                raise Rejected()
            mime = response.getheader("Content-Type", "").split(";")[0].lower()
            manifest = mime in ("application/vnd.apple.mpegurl", "application/x-mpegurl")
            declared_length = response.getheader("Content-Length")
            limit = 1048576 if manifest else 65536
            body = bytearray()
            if method != "HEAD":
                while len(body) < limit + int(manifest):
                    socket.settimeout(self.remaining())
                    remaining = min(limit + int(manifest) - len(body), self.max_bytes - self.bytes)
                    if remaining <= 0:
                        raise Rejected()
                    chunk = response.read1(min(16384, remaining))
                    if not chunk:
                        break
                    body.extend(chunk)
                    self.bytes += len(chunk)
                if manifest and (len(body) > limit or not body
                                 or (declared_length is not None
                                     and int(declared_length) != len(body))):
                    # Cannot certify closure from a truncated manifest.
                    raise Rejected()
                if not body:
                    raise Rejected()
            return response.status, {key.lower(): value for key, value in response.getheaders()}, bytes(body), manifest
        finally:
            conn.close()  # Prefix sampling deliberately cancels large media bodies.

    def run(self, entry, mp4):
        entry = resource_path(entry, self.session, self.grant)
        if mp4:
            _, headers, _, _ = self.fetch(entry, "HEAD")
            total = int(headers.get("content-length", "0"))
            if total <= 0:
                raise Rejected()
            # Start, middle and end are byte-range evidence, not temporal seeks.
            for start in (0, total // 2, max(0, total - 65536)):
                end = min(total - 1, start + 65535)
                status, headers, body, manifest = self.fetch(
                    entry, byte_range=f"bytes={start}-{end}")
                if (status != 206 or manifest or len(body) != end - start + 1
                        or headers.get("content-length") != str(end - start + 1)
                        or headers.get("content-range") != f"bytes {start}-{end}/{total}"):
                    raise Rejected()
                self.samples += 1
        else:
            pending, seen = [entry], set()
            while pending:
                path = pending.pop(0)
                if path in seen:
                    continue
                seen.add(path)
                _, _, body, manifest = self.fetch(path)
                if manifest:
                    self.manifests += 1
                    for ref in references(body):
                        child = resource_path(ref, self.session, self.grant)
                        if child not in seen and child not in pending:
                            pending.append(child)
                        if len(seen) + len(pending) > self.max_requests:
                            raise Rejected()
                else:
                    self.samples += 1
            if not self.manifests or not self.samples:
                raise Rejected()
        return {"evidence": "bounded_relay_byte_access", "requests": self.requests,
                "bytes_read": self.bytes, "manifests": self.manifests,
                "media_prefixes_or_ranges": self.samples,
                "decoded": False, "player_verified": False}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--opt-in", action="store_true", required=True,
                        help="requires separate live authorization when using real sources")
    parser.add_argument("--host", default="127.0.0.1", help="literal loopback IP only")
    parser.add_argument("--port", type=int, default=8092)
    parser.add_argument("--receipt", required=True, help="mode-0600 relayctl receipt")
    parser.add_argument("--grant-file", required=True, help="mode-0600 JWT, never on command line")
    args = parser.parse_args()
    try:
        with wall_deadline(120):
            receipt = json.loads(private_file(args.receipt, 16384))
            grant = private_file(args.grant_file, 4097).decode("ascii").removesuffix("\n")
            if receipt["contract_version"] != "relay.v1" or receipt["format"] not in ("hls", "mp4"):
                raise Rejected()
            probe = Probe(args.host, args.port, receipt["session_id"], grant)
            print(json.dumps(probe.run(receipt["entry_path"], receipt["format"] == "mp4")))
        return 0
    except Exception:
        # Exceptions may contain a path, request URL, JWT or response body.
        print('{"evidence":"incomplete","error":"acceptance_probe_rejected"}')
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
