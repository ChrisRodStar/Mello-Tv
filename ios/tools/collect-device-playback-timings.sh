#!/bin/sh
set -eu
umask 077
root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
output=${1:-$(mktemp -d /tmp/family-stream-phone-timings.XXXXXX)}
mkdir -p "$output/raw"
chmod 700 "$output" "$output/raw"
device_id=${2:-${DEVICE_ID:-booted}}
if ! xcrun devicectl device copy from \
  --device "$device_id" \
  --domain-type appDataContainer --domain-identifier com.streamtv.familystream \
  --source Library/Caches/PlaybackTiming/ --destination "$output/raw" \
  --timeout 30 > "$output/device-copy.log" 2>&1; then
  cat "$output/device-copy.log" >&2
  exit 1
fi
# devicectl may preserve the source directory name; flatten only the parser's
# search input by locating its two known JSONL files, never the app's other data.
python3 "$root/tools/summarize-playback-timings.py" "$output/raw" --output "$output"
printf '%s\n' "$output/summary.md"
