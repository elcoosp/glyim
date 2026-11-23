#!/bin/bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
GLYIM_DIR="$(cd "$SCRIPT_DIR/../.." && pwd)"
SRC_DIR="$SCRIPT_DIR/sources"
SVG_DIR="$SCRIPT_DIR/svg"
PUBLIC_DIR="$GLYIM_DIR/glyim-docs/public"
WRAITH_BIN="${WRAITH_BIN:-$HOME/Documents/Repos/wraith/target/release/wraith}"

mkdir -p "$SVG_DIR" "$PUBLIC_DIR"

if [ ! -x "$WRAITH_BIN" ]; then
  echo "❌ wraith binary not found at $WRAITH_BIN" >&2
  exit 1
fi

count=0
for src_png in "$SRC_DIR"/mascot*.png; do
  [ -f "$src_png" ] || continue
  base="$(basename "$src_png" ".png")"
  out_svg="$SVG_DIR/${base}.svg"

  "$WRAITH_BIN" "$src_png" \
    -m color \
    --engine legacy \
    --hierarchy stacked \
    -Q quality \
    --color-precision 7 \
    --layer-difference 12 \
    --corner-angle 28 \
    --precision 2 \
    --background "#ffffff" \
    --bg-tolerance 14 \
    -o "$out_svg"

  cp "$out_svg" "$PUBLIC_DIR/${base}.svg"
  sz_kb=$(awk "BEGIN {printf \"%.1f\", $(wc -c < "$out_svg") / 1024}")
  echo "✓ ${base}.svg (${sz_kb} KB) -> docs/branding/svg/ & glyim-docs/public/"
  count=$((count + 1))
done

echo "✓ Vectorized all ${count} mascot assets."
