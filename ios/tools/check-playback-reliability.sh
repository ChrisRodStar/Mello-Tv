#!/bin/sh
set -eu
root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
build_dir=$(mktemp -d /tmp/family-stream-playback-checks.XXXXXX)
trap 'rm -rf "$build_dir"' EXIT
xcrun swiftc -D DEBUG -parse-as-library -o "$build_dir/checks" \
  "$root/MelloTV/Core/API/APIModels.swift" \
  "$root/MelloTV/Core/API/MelloTVAPI.swift" \
  "$root/MelloTV/Core/Configuration/ServerAddress.swift" \
  "$root/MelloTV/Features/Playback/PlaybackTiming.swift" \
  "$root/MelloTV/Features/Playback/PlaybackModels.swift" \
  "$root/MelloTV/Features/Playback/PlaybackSegments.swift" \
  "$root/MelloTV/Features/Playback/PreparedPlaybackService.swift" \
  "$root/tools/PlaybackReliabilityChecks.swift"
"$build_dir/checks"
