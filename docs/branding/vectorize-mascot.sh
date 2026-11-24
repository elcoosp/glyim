#!/bin/bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE}")" && pwd)"
GLYIM_DIR="$(cd "$SCRIPT_DIR/../.." && pwd)"
SRC_DIR="$SCRIPT_DIR/sources"
SVG_DIR="$SCRIPT_DIR/svg"
PUBLIC_DIR="$GLYIM_DIR/glyim-docs/public"
ASSETS_DIR="$GLYIM_DIR/glyim-docs/src/assets"
WRAITH_BIN="${WRAITH_BIN:-$HOME/Documents/Repos/wraith/target/release/wraith}"

mkdir -p "$SVG_DIR" "$PUBLIC_DIR" "$ASSETS_DIR"

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
    --engine geometric \
    --gradients off \
    --no-primitives \
    --colors 16 \
    --merge 0.042 \
    --min-area 18 \
    --corner-angle 24 \
    --tolerance 0.20 \
    --smooth 3 \
    --trap 1.15 \
    --refine 2 \
    --precision 2 \
    --background "#ffffff" \
    --bg-tolerance 16 \
    -o "$out_svg"

  swift - "$out_svg" "$PUBLIC_DIR/${base}.svg" "$ASSETS_DIR/${base}.svg" << 'SWIFT'
import AppKit
import Foundation

let args = Array(CommandLine.arguments.dropFirst())
let outPath = args.first!
let targets = Array(args.dropFirst())
let url = URL(fileURLWithPath: outPath)

var raw = try String(contentsOf: url, encoding: .utf8)

if let img = NSImage(contentsOf: url) {
    let size = 512
    if let rep = NSBitmapImageRep(
        bitmapDataPlanes: nil,
        pixelsWide: size,
        pixelsHigh: size,
        bitsPerSample: 8,
        samplesPerPixel: 4,
        hasAlpha: true,
        isPlanar: false,
        colorSpaceName: .deviceRGB,
        bytesPerRow: size * 4,
        bitsPerPixel: 32
    ) {
        NSGraphicsContext.saveGraphicsState()
        NSGraphicsContext.current = NSGraphicsContext(bitmapImageRep: rep)
        NSColor.clear.setFill()
        NSRect(x: 0, y: 0, width: size, height: size).fill()
        img.draw(in: NSRect(x: 0, y: 0, width: size, height: size))
        NSGraphicsContext.restoreGraphicsState()

        if let data = rep.bitmapData {
            var minX = size, minY = size, maxX = -1, maxY = -1
            for y in 0..<size {
                for x in 0..<size {
                    if data[(y * size + x) * 4 + 3] > 8 {
                        if x < minX { minX = x }
                        if x > maxX { maxX = x }
                        if y < minY { minY = y }
                        if y > maxY { maxY = y }
                    }
                }
            }
            if maxX >= minX && maxY >= minY {
                let scale = 2
                let pad = 24
                let vx = max(0, minX * scale - pad)
                let vy = max(0, minY * scale - pad)
                let vw = min(1024 - vx, (maxX - minX + 1) * scale + pad * 2)
                let vh = min(1024 - vy, (maxY - minY + 1) * scale + pad * 2)
                raw = raw.replacingOccurrences(
                    of: #"<svg([^>]*?)width="\d+"\s+height="\d+"\s+viewBox="[^"]+""#,
                    with: "<svg$1width=\"\(vw)\" height=\"\(vh)\" viewBox=\"\(vx) \(vy) \(vw) \(vh)\"",
                    options: .regularExpression
                )
            }
        }
    }
}

try raw.write(to: url, atomically: true, encoding: .utf8)
for targetPath in targets {
    if targetPath.hasSuffix("/mascot.svg") || targetPath.contains("/public/") {
        try raw.write(to: URL(fileURLWithPath: targetPath), atomically: true, encoding: .utf8)
    }
}
SWIFT

  sz_kb=$(awk "BEGIN {printf \"%.1f\", $(wc -c < "$out_svg") / 1024}")
  echo "✓ ${base}.svg (${sz_kb} KB)"
  count=$((count + 1))
done

echo "✓ Vectorized all ${count} crisp cel-shaded mascot SVGs."
