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
        -m color --engine geometric \
        --gradients off --no-primitives \
        --colors 18 --merge 0.036 \
        --min-area 20 --corner-angle 24 \
        --tolerance 0.22 --smooth 3 \
        --trap 2 --refine 2 --field-blur 1 \
        --precision 2 \
        --background "#ffffff" --bg-tolerance 16 \
        -o "$out_svg"

    python3 - "$out_svg" <<'PY'
import re, sys

p = sys.argv[1]
s = open(p, encoding="utf-8").read()
m = re.search(r'<svg[^>]*?width="(\d+)"[^>]*?height="(\d+)"', s)
W, H = (int(m.group(1)), int(m.group(2))) if m else (1024, 1024)

# Supprimer un éventuel rectangle pleine page
s = re.sub(rf'<path[^>]*d="M0 0h{W}v{H}H0Z"[^>]*/?>', '', s)

# Calcul rapide de la bounding box via les points de départ M(x,y) et coordonnées absolues
minx, miny, maxx, maxy = float(W), float(H), 0.0, 0.0
found = False
for d in re.findall(r'd="([^"]+)"', s):
    # Extraire tous les M x y (point d'ancrage exact de chaque sous-tracé)
    for mx, my in re.findall(r'M\s*(-?\d+(?:\.\d+)?)[,\s]+(-?\d+(?:\.\d+)?)', d):
        x, y = float(mx), float(my)
        if 0 <= x <= W and 0 <= y <= H:
            minx, miny = min(minx, x), min(miny, y)
            maxx, maxy = max(maxx, x), max(maxy, y)
            found = True

if found and maxx > minx and maxy > miny:
    pad = 24
    vx = max(0, int(minx) - pad)
    vy = max(0, int(miny) - pad)
    vw = min(W - vx, int(maxx - minx) + pad * 2 + 1)
    vh = min(H - vy, int(maxy - miny) + pad * 2 + 1)
    s = re.sub(
        r'<svg([^>]*?)width="\d+"\s+height="\d+"\s+viewBox="[^"]+"',
        rf'<svg\1width="{vw}" height="{vh}" viewBox="{vx} {vy} {vw} {vh}"',
        s,
        count=1,
    )

open(p, "w", encoding="utf-8").write(s)
PY

    cp "$out_svg" "$PUBLIC_DIR/${base}.svg"
    if [ "$base" = "mascot" ]; then
        cp "$out_svg" "$ASSETS_DIR/${base}.svg"
    fi

    echo "✓ ${base}.svg ($(du -k "$out_svg" | cut -f1) KB)"
    count=$((count + 1))
done
echo "✓ Vectorized ${count} mascot SVGs."
