#!/usr/bin/env python3
"""Offline: disposable copy of the relay with a cfg(test) API integration harness.
No production destination/guard policy changes; no sibling checkout is edited.
"""
import os
import pathlib
import shutil
import signal
from process_owner import run
import tempfile

ROOT = pathlib.Path(__file__).resolve().parents[1]
WORKSPACE = ROOT.parent

def main():
    env = dict(os.environ, GOPROXY="off", GOSUMDB="off")
    with tempfile.TemporaryDirectory(prefix="family-stream-api-integration-") as temporary:
        work = pathlib.Path(temporary)
        api_binary = work / "api-tests"
        provider_binary = work / "provider-tests"
        run(["go", "test", "-race", "-c", "-o", str(api_binary), "./internal/api"], ROOT, env)
        run(["go", "test", "-race", "-c", "-o", str(provider_binary), "./internal/transport/http"], WORKSPACE / "family-stream-providers", env)
        relay = work / "relay"
        shutil.copytree(WORKSPACE / "family-stream-relay", relay, ignore=shutil.ignore_patterns(".git", "target", ".local", "__pycache__"))
        harness = relay / "src/network/transport/tests/stage4.rs"
        source = harness.read_text()
        # Give the existing synthetic store owner its own cancellation token so
        # this fixture can stop/join/reopen SQLite while retaining the origin.
        source = source.replace("struct Harness {", "struct Harness {\n    api_storage_stop: CancellationToken,")
        source = source.replace("let storage = tokio::spawn({\n            let stop = stop.clone();", "let api_storage_stop = CancellationToken::new();\n        let storage = tokio::spawn({\n            let stop = api_storage_stop.clone();")
        source = source.replace("jobs: vec![storage, server, net],", "jobs: vec![storage, server, net], api_storage_stop,")
        source = source.replace("async fn close(self) {\n        self.stop.cancel();", "async fn close(self) {\n        self.api_storage_stop.cancel();\n        self.stop.cancel();")
        harness.write_text(source)
        guard = relay / "src/network/guard.rs"
        guard.write_text(guard.read_text() + "\n#[cfg(test)] impl Guard { pub(crate) fn api_fixture_tick(&self) { if let Ok(mut state)=self.0.lock() { if let Some(epoch)=state.as_mut() {epoch.checked=Instant::now();} } } }\n")
        test = relay / "src/network/transport/tests/stage4/stage6_service.rs"
        test.write_text(test.read_text() + (ROOT / "tools/relay_api_fixture.rs").read_text())
        env.update(FAMILY_STREAM_API_TEST_BINARY=str(api_binary), FAMILY_STREAM_PROVIDER_TEST_BINARY=str(provider_binary), CARGO_TARGET_DIR=str(WORKSPACE / "family-stream-relay/target"))
        run(["cargo", "test", "--locked", "--offline", "--lib", "public_api_production_listeners", "--", "--ignored", "--nocapture"], relay, env)

if __name__ == "__main__":
    def interrupted(_signum, _frame):
        raise RuntimeError("integration owner interrupted")
    signal.signal(signal.SIGTERM, interrupted)
    main()
