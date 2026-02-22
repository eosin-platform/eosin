#!/usr/bin/env bash
set -euo pipefail

INPUT="${1:-eosin-demo.webm}"
OUTPUT="${2:-eosin-demo.gif}"

if [[ ! -f "$INPUT" ]]; then
    echo "Input file '$INPUT' not found."
    echo "Usage: $0 input.webm [output.gif]"
    exit 1
fi

# 1. Generate palette for best quality/size
ffmpeg -y -i "$INPUT" \
    -vf "fps=15,scale=640:-1:flags=lanczos,palettegen" \
    palette.png

# 2. Apply palette to create optimized GIF
ffmpeg -y -i "$INPUT" -i palette.png \
    -lavfi "fps=15,scale=640:-1:flags=lanczos[p];[p][1:v]paletteuse" \
    "$OUTPUT"

echo "Done. Output: $OUTPUT"