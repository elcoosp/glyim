#!/bin/bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
GLYIM_DIR="$(cd "$SCRIPT_DIR/../.." && pwd)"
SRC_DIR="$SCRIPT_DIR/sources"
SVG_DIR="$SCRIPT_DIR/svg"
PUBLIC_DIR="$GLYIM_DIR/glyim-docs/public"
ASSETS_DIR="$GLYIM_DIR/glyim-docs/src/assets"
WRAITH_BIN="${WRAITH_BIN:-$HOME/Documents/Repos/wraith/target/release/wraith}"

mkdir -p "$SVG_DIR" "$PUBLIC_DIR" "$ASSETS_DIR"
[ -x "$WRAITH_BIN" ] || {
    echo "❌ wraith binary not found at $WRAITH_BIN" >&2
    exit 1
}

count=0
for src_png in "$SRC_DIR"/mascot*.png; do
    [ -f "$src_png" ] || continue
    base="$(basename "$src_png" .png)"
    out_svg="$SVG_DIR/${base}.svg"

    "$WRAITH_BIN" "$src_png" \
        --color \
        --preset cel-art \
        --background "#ffffff" \
        --bg-tolerance 14 \
        --colors 12 \
        --merge 0.048 \
        --blotch-fraction 0.025 \
        --sever-bridges 1 \
        --min-area 22 \
        --smooth 4 \
        --tolerance 0.35 \
        --precision 2 \
        --trim \
        -o "$out_svg"

    cp "$out_svg" "$PUBLIC_DIR/${base}.svg"
    if [ "$base" = "mascot" ]; then
        cp "$out_svg" "$ASSETS_DIR/${base}.svg"
    fi

    echo "✓ ${base}.svg ($(du -k "$out_svg" | cut -f1) KB)"
    count=$((count + 1))
done
echo "✓ Vectorized ${count} mascot SVGs."
