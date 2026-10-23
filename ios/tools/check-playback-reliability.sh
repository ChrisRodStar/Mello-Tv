#!/bin/sh
set -eu
root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
build_dir=$(mktemp -d /tmp/family-stream-playback-checks.XXXXXX)
trap 'rm -rf "$build_dir"' EXIT
xcrun swiftc -D DEBUG -parse-as-library -o "$build_dir/checks" \
  "$root/FamilyStream/Core/API/APIModels.swift" \
  "$root/FamilyStream/Core/API/FamilyStreamAPI.swift" \
  "$root/FamilyStream/Core/Configuration/ServerAddress.swift" \
  "$root/FamilyStream/Features/Playback/PlaybackTiming.swift" \
  "$root/FamilyStream/Features/Playback/PlaybackModels.swift" \
  "$root/FamilyStream/Features/Playback/PreparedPlaybackService.swift" \
  "$root/tools/PlaybackReliabilityChecks.swift"
"$build_dir/checks"
