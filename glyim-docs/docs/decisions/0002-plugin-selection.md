# ADR 0002: Starlight plugin selection

**Status:** Accepted
**Date:** 2026-11-07

## Context

The docs-architecture mandate is to install the HiDeoo-maintained plugin set "bells and whistles" where relevant for Starlight 0.42.x / Astro 7.4.x, and to record the reason for every omitted plugin.

Verified compatibility at execution time (peer dependency ranges accept `astro >=7.2.10` / `@astrojs/starlight >=0.42.0`):

| Plugin | Version |
|--------|---------|
| `starlight-links-validator` | 0.27.0 |
| `starlight-image-zoom` | 0.16.2 |
| `starlight-heading-badges` | 0.8.0 |

## Decision

**Installed:**

- `starlight-links-validator` — the build fails on broken internal links. This is a release blocker for every `pnpm build`.
- `starlight-image-zoom` — click-to-zoom for the pipeline and architecture diagrams.
- `starlight-heading-badges` — badges in reference headings (used to mark GSX as experimental).

**Omitted, with reasons:**

- `starlight-typedoc` — Glyim is a Rust project; there is no TypeScript API surface to generate reference docs from.
- `starlight-blog` — a compiler documentation site has no editorial blog; the repository's `CHANGELOG.md` is the changelog of record. A changelog page can be added later as regular content.
- `starlight-openapi` — the repository contains no OpenAPI specification.
- `starlight-versions` — Glyim is pre-1.0 with a single docs version; versioning adds navigation cost with no reader benefit today. Revisit when versioned docs exist.
- DocSearch (`@astrojs/starlight-docsearch`) — no approved Algolia DocSearch application exists for this project. Pagefind (built into Starlight) is the search solution; it is kept enabled and was verified to index the built site.

## Consequences

- `pnpm build` fails on any broken internal link (intentional).
- Images in docs are click-to-zoom; GSX status is badged in headings.
- If an Algolia DocSearch application is approved later, add the plugin per its guidance and supersede this ADR.
