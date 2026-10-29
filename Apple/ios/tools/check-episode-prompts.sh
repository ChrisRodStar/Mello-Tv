#!/bin/sh
set -eu
root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
swift build --package-path "$root/../Packages/MelloKit" >/dev/null 2>&1
build_dir=$(mktemp -d /tmp/mello-episode-prompt-checks.XXXXXX)
trap 'rm -rf "$build_dir"' EXIT
xcrun swiftc -D DEBUG -parse-as-library \
  -I "$root/../Packages/MelloKit/.build/debug" \
  -L "$root/../Packages/MelloKit/.build/debug" \
  -lMelloKit \
  -o "$build_dir/checks" \
  "$root/MelloTV/Services/MelloTVAPI.swift" \
  "$root/MelloTV/Services/ServerAddress.swift" \
  "$root/MelloTV/Features/Playback/PlaybackTiming.swift" \
  "$root/MelloTV/Features/Playback/PlaybackModels.swift" \
  "$root/MelloTV/Features/Playback/PlaybackSegments.swift" \
  "$root/MelloTV/Features/Playback/PreparedPlaybackService.swift" \
  "$root/tools/EpisodePromptChecks.swift"
"$build_dir/checks" "$@"
