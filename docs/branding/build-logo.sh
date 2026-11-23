#!/bin/bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE}")" && pwd)"
GLYIM_DIR="$(cd "$SCRIPT_DIR/../.." && pwd)"
FONTS_DIR="$SCRIPT_DIR/fonts"
SVG_DIR="$SCRIPT_DIR/svg"
FAV_DIR="$SCRIPT_DIR/favicons"
PUBLIC_DIR="$GLYIM_DIR/glyim-docs/public"

mkdir -p "$SVG_DIR" "$FAV_DIR" "$PUBLIC_DIR"

FONT_PATH=""
for pattern in "*Bold*.ttf" "*Bold*.otf" "*wght*.ttf" "*.ttf" "*.otf"; do
  FONT_PATH=$(find "$FONTS_DIR" -maxdepth 1 -iname "$pattern" 2>/dev/null | head -n 1 || true)
  [ -n "$FONT_PATH" ] && break
done

if [ -z "$FONT_PATH" ]; then
  echo "❌ No Space Grotesk font found in $FONTS_DIR" >&2
  exit 1
fi

export FONT_PATH SVG_DIR FAV_DIR PUBLIC_DIR

swift - << 'SWIFT'
import Foundation
import CoreText
import CoreGraphics
import AppKit

let env = ProcessInfo.processInfo.environment
guard let fontPath = env["FONT_PATH"],
      let svgDirPath = env["SVG_DIR"],
      let favDirPath = env["FAV_DIR"],
      let publicDirPath = env["PUBLIC_DIR"] else {
    fatalError("Missing required environment variables")
}

let svgDir = URL(fileURLWithPath: svgDirPath)
let favDir = URL(fileURLWithPath: favDirPath)
let publicDir = URL(fileURLWithPath: publicDirPath)
let fontURL = URL(fileURLWithPath: fontPath) as CFURL

guard let descriptors = CTFontManagerCreateFontDescriptorsFromURL(fontURL) as? [CTFontDescriptor],
      let baseDesc = descriptors.first else {
    fatalError("Failed to load font descriptor from \(fontPath)")
}

let wghtTag: UInt32 = (UInt32(Character("w").asciiValue!) << 24)
    | (UInt32(Character("g").asciiValue!) << 16)
    | (UInt32(Character("h").asciiValue!) << 8)
    | UInt32(Character("t").asciiValue!)
let boldDesc = CTFontDescriptorCreateCopyWithVariation(baseDesc, NSNumber(value: wghtTag), 700.0)

struct PathResult {
    let d: String
    let width: CGFloat
}

func svgPath(for text: String, size: CGFloat, tracking: CGFloat, rightAlignAt: CGFloat?, leftAt: CGFloat, baselineY: CGFloat) -> PathResult {
    let font = CTFontCreateWithFontDescriptor(boldDesc, size, nil)
    let chars = Array(text.utf16)
    var glyphs = Array(repeating: CGGlyph(0), count: chars.count)
    CTFontGetGlyphsForCharacters(font, chars, &glyphs, chars.count)

    var advances = Array(repeating: CGSize.zero, count: chars.count)
    CTFontGetAdvancesForGlyphs(font, .horizontal, glyphs, &advances, chars.count)

    let totalWidth = advances.reduce(0.0) { $0 + $1.width } + tracking * CGFloat(max(0, chars.count - 1))
    var cursorX = rightAlignAt != nil ? (rightAlignAt! - totalWidth) : leftAt

    var d = ""
    func fmt(_ v: CGFloat) -> String {
        String(format: "%.2f", Double(v))
    }

    for (glyph, advance) in zip(glyphs, advances) {
        if let cgPath = CTFontCreatePathForGlyph(font, glyph, nil) {
            cgPath.applyWithBlock { elemPtr in
                let el = elemPtr.pointee
                let pts = UnsafeBufferPointer(start: el.points, count: 3)
                switch el.type {
                case .moveToPoint:
                    let p = pts.first!
                    d += "M\(fmt(cursorX + p.x)),\(fmt(baselineY - p.y)) "
                case .addLineToPoint:
                    let p = pts.first!
                    d += "L\(fmt(cursorX + p.x)),\(fmt(baselineY - p.y)) "
                case .addQuadCurveToPoint:
                    let c = pts.first!
                    let p = pts.dropFirst().first!
                    d += "Q\(fmt(cursorX + c.x)),\(fmt(baselineY - c.y)) \(fmt(cursorX + p.x)),\(fmt(baselineY - p.y)) "
                case .addCurveToPoint:
                    let c1 = pts.first!
                    let c2 = pts.dropFirst().first!
                    let p = pts.dropFirst(2).first!
                    d += "C\(fmt(cursorX + c1.x)),\(fmt(baselineY - c1.y)) \(fmt(cursorX + c2.x)),\(fmt(baselineY - c2.y)) \(fmt(cursorX + p.x)),\(fmt(baselineY - p.y)) "
                case .closeSubpath:
                    d += "Z "
                @unknown default:
                    break
                }
            }
        }
        cursorX += advance.width + tracking
    }
    return PathResult(d: d.trimmingCharacters(in: .whitespaces), width: totalWidth)
}

let gl = svgPath(for: "GL", size: 232, tracking: -6, rightAlignAt: 452, leftAt: 0, baselineY: 448)
let wordLeft: CGFloat = 585
let word = svgPath(for: "glyim", size: 325, tracking: -10, rightAlignAt: nil, leftAt: wordLeft, baselineY: 372)
let totalW = Int(ceil(wordLeft + word.width + 48))
let svgNS = ["http", ":", "//", "www", ".w3.org", "/2000/svg"].joined()

let logoSVG = """
<svg xmlns="\(svgNS)" viewBox="0 0 512 512" width="512" height="512">
  <defs>
    <clipPath id="glyim-badge-clip">
      <rect x="0" y="0" width="512" height="512" rx="112" ry="112"/>
    </clipPath>
  </defs>
  <g clip-path="url(#glyim-badge-clip)">
    <rect x="0" y="0" width="512" height="512" fill="#059669"/>
    <polygon points="0,512 512,185 512,512" fill="#06b6d4"/>
    <polygon points="0,0 230,0 0,230" fill="#f59e0b"/>
    <path d="\(gl.d)" fill="#ffffff"/>
  </g>
</svg>
"""

func makeLockupSVG(textFill: String, clipId: String) -> String {
    return """
<svg xmlns="\(svgNS)" viewBox="0 0 \(totalW) 512" width="\(totalW)" height="512">
  <defs>
    <clipPath id="\(clipId)">
      <rect x="0" y="0" width="512" height="512" rx="112" ry="112"/>
    </clipPath>
  </defs>
  <g clip-path="url(#\(clipId))">
    <rect x="0" y="0" width="512" height="512" fill="#059669"/>
    <polygon points="0,512 512,185 512,512" fill="#06b6d4"/>
    <polygon points="0,0 230,0 0,230" fill="#f59e0b"/>
    <path d="\(gl.d)" fill="#ffffff"/>
  </g>
  <path d="\(word.d)" fill="\(textFill)"/>
</svg>
"""
}

let lockupLight = makeLockupSVG(textFill: "#0f172a", clipId: "glyim-lockup-light")
let lockupDark = makeLockupSVG(textFill: "#f8fafc", clipId: "glyim-lockup-dark")

for dir in [svgDir, publicDir] {
    try logoSVG.write(to: dir.appendingPathComponent("logo.svg"), atomically: true, encoding: .utf8)
    try lockupLight.write(to: dir.appendingPathComponent("logo-wordmark.svg"), atomically: true, encoding: .utf8)
    try lockupDark.write(to: dir.appendingPathComponent("logo-wordmark-dark.svg"), atomically: true, encoding: .utf8)
}

let logoFile = svgDir.appendingPathComponent("logo.svg")
guard let nsImg = NSImage(contentsOf: logoFile) else {
    fatalError("Could not render logo.svg")
}

func writePNG(size: Int, filename: String) throws {
    let target = NSSize(width: size, height: size)
    let rep = NSBitmapImageRep(
        bitmapDataPlanes: nil,
        pixelsWide: size,
        pixelsHigh: size,
        bitsPerSample: 8,
        samplesPerPixel: 4,
        hasAlpha: true,
        isPlanar: false,
        colorSpaceName: .deviceRGB,
        bytesPerRow: 0,
        bitsPerPixel: 0
    )!
    rep.size = target
    NSGraphicsContext.saveGraphicsState()
    NSGraphicsContext.current = NSGraphicsContext(bitmapImageRep: rep)
    nsImg.draw(in: NSRect(origin: .zero, size: target))
    NSGraphicsContext.restoreGraphicsState()
    let data = rep.representation(using: .png, properties: [:])!
    try data.write(to: favDir.appendingPathComponent(filename))
    try data.write(to: publicDir.appendingPathComponent(filename))
}

try writePNG(size: 32, filename: "favicon-32x32.png")
try writePNG(size: 180, filename: "apple-touch-icon.png")
print("✓ Built logo.svg, logo-wordmark.svg, logo-wordmark-dark.svg, favicon-32x32.png, apple-touch-icon.png")
SWIFT

python3 - << PYEOF
from PIL import Image
im = Image.open("$FAV_DIR/favicon-32x32.png")
im.save("$FAV_DIR/favicon.ico", format="ICO", sizes=[(16, 16), (32, 32)])
im.save("$PUBLIC_DIR/favicon.ico", format="ICO", sizes=[(16, 16), (32, 32)])
print("✓ Built favicon.ico (16x16 & 32x32)")
PYEOF
