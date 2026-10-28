#!/usr/bin/env python3
"""Read bounded diagnostic JSONL; never access the network or raw playback URLs."""
import argparse
import json
from pathlib import Path

ACTIONS = {"play", "resume", "restart", "nextEpisode", "retry"}

def summarize(directory):
    grouped = {}
    incomplete = 0
    for path in sorted(directory.rglob("events*.jsonl")):
        for line in path.read_text().splitlines():
            try:
                event = json.loads(line)
                if event.get("schema") != 1 or not isinstance(event.get("traceID"), str):
                    incomplete += 1
                    continue
                grouped.setdefault(event["traceID"], []).append(event)
            except (ValueError, TypeError, AttributeError):
                incomplete += 1  # A snapshot may end during the last append.
    reports = []
    for trace, events in grouped.items():
        events.sort(key=lambda e: e["elapsedMS"])
        stages = {e["stage"]: e for e in events}
        first = events[0]
        action = first["action"]
        if action in {"launch", "preparation"}:
            continue
        def at(name):
            return stages.get(name, {}).get("elapsedMS")
        def interval(start, end):
            a, b = at(start), at(end)
            return b - a if a is not None and b is not None and b >= a else None
        related = stages.get("preparationClaimed", {}).get("relatedTraceID") or stages.get("preparationClaimStarted", {}).get("relatedTraceID")
        preparation = {e["stage"]: e for e in grouped.get(related, [])}
        def prepared_interval(start, end):
            a, b = preparation.get(start), preparation.get(end)
            return b["elapsedMS"] - a["elapsedMS"] if a and b else None
        frame = at("firstFrameReadyForDisplay")
        report = {
            "trace_id": trace, "action": action, "title_id": first.get("titleID"),
            "media_type": first.get("mediaType"), "season": first.get("season"), "episode": first.get("episode"),
            "started_unix_ms": first["unixMS"] - first["elapsedMS"],
            "first_frame_from_action_ms": frame,
            "tap_to_first_frame_ms": frame if action in ACTIONS else None,
            "presentation_to_first_frame_ms": interval("presentationStarted", "firstFrameReadyForDisplay"),
            "authorization_ms": interval("authorizationStarted", "authorizationFinished"),
            "write_barrier_ms": interval("writeBarrierStarted", "writeBarrierFinished"),
            "resume_metadata_ms": interval("resumeMetadataStarted", "resumeMetadataFinished"),
            "resume_seek_ms": interval("resumeSeekStarted", "resumeSeekFinished"),
            "claim_wait_ms": interval("preparationClaimStarted", "preparationClaimed"),
            "prepared_trace_id": related,
            "offered_rendition_count": stages.get("renditionsOffered", {}).get("value"),
            "prepared_player": stages.get("preparationClaimed", {}).get("flag"),
            "preparation_authorization_ms": prepared_interval("authorizationStarted", "authorizationFinished"),
            "preroll_finished": preparation.get("prerollFinished", {}).get("flag"),
            "preparation_resume_seek_ms": prepared_interval("warmupSeekStarted", "warmupSeekFinished"),
            "quality_changes": [{"elapsed_ms": e["elapsedMS"], "width": e.get("width"), "height": e.get("height")}
                                for e in events if e["stage"] == "qualityChanged"],
            "quality_limits": [{"elapsed_ms": e["elapsedMS"], "maximum_height": e.get("value")}
                               for e in events if e["stage"] == "qualityLimitChanged"],
            "delivery_samples": [{"elapsed_ms": e["elapsedMS"], "observed_bitrate": e.get("value"), "stall_observed": e.get("flag")}
                                 for e in events if e["stage"] == "deliverySample"],
            "player_to_first_frame_ms": interval("playerCreated", "firstFrameReadyForDisplay"),
            "surface_to_first_frame_ms": interval("surfaceAttached", "firstFrameReadyForDisplay"),
            "first_playing_ms": at("playing"), "first_observed_advancement_ms": at("playbackAdvancing"),
            "observed_width": max((e.get("width", 0) for e in events), default=0),
            "observed_height": max((e.get("height", 0) for e in events), default=0),
            "failed": "failed" in stages or "authorizationFailed" in stages,
            "cancelled": "cancelled" in stages,
            "closed": "closed" in stages,
            "milestones_ms": {k: e["elapsedMS"] for k, e in stages.items()},
        }
        reports.append(report)
    reports.sort(key=lambda r: r["started_unix_ms"])
    return {"schema": 1, "playback_attempts": reports, "trace_count": len(grouped), "ignored_lines": incomplete,
            "notes": ["Times use the phone's monotonic clock; wall time is for identification only.",
                      "Null means not observed, not zero. Preparation and tap timelines remain separate.",
                      "First frame is AVPlayerViewController.isReadyForDisplay KVO, not a camera measurement.",
                      "Playback advancement is detected by the existing 0.5-second periodic observer.",
                      "Authorization includes HTTP retries and response decoding; backend sub-stages are not measured here.",
                      "Presentation action is a fallback without a recorded tap; it is never labeled tap-to-frame."]}

def markdown(summary):
    def val(value): return f"{value:.1f}" if isinstance(value, (int, float)) else "—"
    text = "# Physical-device playback timing\n\n"
    text += "| Trace | Action | Title / episode | Tap → frame ms | Authorization ms | Prepared claim ms | Player → frame ms | Resolution |\n|---|---|---|---:|---:|---:|---:|---|\n"
    for r in summary["playback_attempts"]:
        coordinates = f"S{r['season']}E{r['episode']}" if r['season'] is not None else ""
        text += f"| {r['trace_id'][:8]} | {r['action']} | {r['title_id']} {coordinates} | {val(r['tap_to_first_frame_ms'])} | {val(r['authorization_ms'])} | {val(r['claim_wait_ms'])} | {val(r['player_to_first_frame_ms'])} | {r['observed_width']:g}×{r['observed_height']:g} |\n"
    if not summary["playback_attempts"]:
        text += "\nNo playback attempts recorded yet.\n"
    text += "\n" + "\n".join("- " + note for note in summary["notes"]) + "\n"
    return text

if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("directory", type=Path)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    summary = summarize(args.directory)
    args.output.mkdir(parents=True, exist_ok=True)
    (args.output / "summary.json").write_text(json.dumps(summary, indent=2) + "\n")
    (args.output / "summary.md").write_text(markdown(summary))
    print(json.dumps({"playback_attempts": len(summary["playback_attempts"]), "trace_count": summary["trace_count"], "output": str(args.output)}))
