#!/usr/bin/env bash
set -euo pipefail

INPUT="${1:-eosin-demo.webm}"
OUTPUT="${2:-eosin-demo.gif}"

if [[ ! -f "$INPUT" ]]; then
    echo "Input file '$INPUT' not found."
    echo "Usage: $0 input.webm [output.gif]"
    exit 1
fi

# Tunable knobs
FPS=10          # lower fps => smaller file; 8–12 is usually fine
WIDTH=480       # reduce width a bit; 400–600 is a good range
DITHER="bayer:bayer_scale=5"  # lighter-weight dithering than floyd_steinberg

# 1) Generate palette
ffmpeg -y -i "$INPUT" \
  -vf "fps=${FPS},scale=${WIDTH}:-1:flags=lanczos,palettegen" \
  palette.png

# 2) Use palette to create optimized GIF
ffmpeg -y -i "$INPUT" -i palette.png \
  -lavfi "fps=${FPS},scale=${WIDTH}:-1:flags=lanczos[x];[x][1:v]paletteuse=dither=${DITHER}" \
  -loop 0 \
  "$OUTPUT"

echo "Done. Output: $OUTPUT"