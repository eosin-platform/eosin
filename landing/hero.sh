#!/usr/bin/env bash
set -euo pipefail

if [[ $# -lt 1 ]]; then
  echo "Usage: $0 INPUT [OUTPUT]"
  exit 1
fi

INPUT="$1"
OUTPUT="${2:-hero.webm}"

# Tunables via env vars
BITRATE="${BITRATE:-3M}"   # target video bitrate
FPS="${FPS:-30}"           # output frame rate
WIDTH="${WIDTH:-1280}"     # output width (height auto to keep aspect)

echo "Input:  $INPUT"
echo "Output: $OUTPUT"
echo "Width:  $WIDTH"
echo "FPS:    $FPS"
echo "BR:     $BITRATE"

# 2-pass VP9 encode for good quality on the landing page hero
# Pass 1: analysis only
ffmpeg -y -i "$INPUT" \
  -vf "scale=${WIDTH}:-2,fps=${FPS}" \
  -c:v libvpx-vp9 \
  -b:v "$BITRATE" \
  -pass 1 \
  -an \
  -f webm /dev/null

# Pass 2: actual encode
ffmpeg -y -i "$INPUT" \
  -vf "scale=${WIDTH}:-2,fps=${FPS}" \
  -c:v libvpx-vp9 \
  -b:v "$BITRATE" \
  -pass 2 \
  -pix_fmt yuv420p \
  -an \
  "$OUTPUT"

# Clean up pass logs
rm -f ffmpeg2pass-0.log ffmpeg2pass-0.log.mbtree

echo "Done: $OUTPUT"
