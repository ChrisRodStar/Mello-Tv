#!/usr/bin/env python3
"""Offline regressions for phase separation and incomplete device exports."""
import importlib.util
import json
import tempfile
from pathlib import Path
spec = importlib.util.spec_from_file_location("summary", Path(__file__).with_name("summarize-playback-timings.py"))
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)
with tempfile.TemporaryDirectory() as temp:
    root = Path(temp)
    rows = []
    def event(trace, action, stage, elapsed, **extra):
        rows.append(dict(schema=1, traceID=trace, action=action, stage=stage, elapsedMS=elapsed,
                         unixMS=100_000 + elapsed, **extra))
    event("prep", "preparation", "began", 0)
    event("prep", "preparation", "authorizationStarted", 10)
    event("prep", "preparation", "authorizationFinished", 5010)
    event("tap", "play", "began", 0)
    event("tap", "play", "presentationStarted", 20)
    event("tap", "play", "preparationClaimStarted", 21, relatedTraceID="prep")
    event("tap", "play", "preparationClaimed", 22, relatedTraceID="prep", flag=True)
    event("tap", "play", "firstFrameReadyForDisplay", 90, width=1920, height=1080)
    event("fallback", "presentation", "began", 0)
    event("fallback", "presentation", "firstFrameReadyForDisplay", 40)
    event("failed", "retry", "began", 0)
    event("failed", "retry", "failed", 10)
    (root / "events.jsonl").write_text("\n".join(json.dumps(row) for row in rows) + '\n{"schema":')
    result = module.summarize(root)
    lookup = {r["trace_id"]: r for r in result["playback_attempts"]}
    assert lookup["tap"]["tap_to_first_frame_ms"] == 90
    assert lookup["tap"]["claim_wait_ms"] == 1
    assert lookup["tap"]["authorization_ms"] is None
    assert lookup["tap"]["preparation_authorization_ms"] == 5000
    assert lookup["fallback"]["tap_to_first_frame_ms"] is None
    assert lookup["failed"]["tap_to_first_frame_ms"] is None and lookup["failed"]["failed"]
    assert result["ignored_lines"] == 1
    assert "No playback attempts" not in module.markdown(result)
print("PASS separate preparation/tap timelines, missing milestones, failed attempts and partial snapshots")
