# ADR 0003: Brand assets and accent color

**Status:** Accepted
**Date:** 2026-11-07

## Context

Starlight ships a default blue accent; the mandate forbids shipping it. The project's real brand assets are:

- `docs/logo.png` in the Glyim repository — a 1264×1264 RGBA PNG of a graphite 3-D "G".
- Dominant colors (bucket-sampled from opaque pixels): `#303030`, `#181818`, `#484848`, `#606060` — neutral graphite. The only chroma-carrying pixels are a cool steel tint: `#303048` (indigo-leaning) and `#304848` (teal-leaning).

The logo is dark, so it is invisible on Starlight's dark background without a variant.

## Decision

1. **Logo variants.** Use the original logo (cropped, downscaled to 256 px) as `src/assets/logo-light.png` for the light theme, and generate `src/assets/logo-dark.png` by inverting luminance while preserving alpha — the same mark, readable on dark backgrounds. Both are wired through Starlight's `logo: { light, dark }` config. The logo does not contain the project name, so `replacesTitle` stays off and the text title renders beside the mark.
2. **Favicon.** A 64 px cut of the original logo as `public/favicon.png`, referenced via `favicon: { href: '/favicon.png' }`.
3. **Accent.** Derive the accent from the logo's chroma-carrying pixels: a steel-blue hue (~200°) at contrast-safe lightness levels.

| Variable | Light theme | Dark theme |
|----------|-------------|------------|
| `--sl-color-accent-low` | `hsl(200, 40%, 92%)` | `hsl(200, 42%, 14%)` |
| `--sl-color-accent` | `hsl(200, 48%, 32%)` | `hsl(199, 45%, 58%)` |
| `--sl-color-accent-high` | `hsl(200, 52%, 25%)` | `hsl(197, 48%, 74%)` |

## Verification

WCAG contrast ratios computed programmatically (relative luminance per WCAG 2.x):

| Pair | Ratio | Level |
|------|-------|-------|
| light accent `#2a5f79` on white | 6.97:1 | AA |
| light accent-high `#1f4b61` on white | 9.39:1 | AA |
| dark accent `#64a6c4` on `#181a1e` (Starlight dark bg) | 6.47:1 | AA |
| dark accent-high `#9dcadd` on `#181a1e` | 9.89:1 | AA |

All four pass WCAG AA (4.5:1) for normal-size text in both themes. Starlight's built-in light/dark toggle is kept untouched.

## Consequences

- Both logo variants render correctly per theme; the accent matches the graphite/steel brand instead of Starlight blue.
- The generation script lives at `scripts/prepare_brand_assets.py` (in the authoring workspace) and can be re-run if the upstream logo changes.
