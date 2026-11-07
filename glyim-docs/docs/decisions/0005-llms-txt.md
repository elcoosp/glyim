# ADR 0005: Generate `llms.txt` and `llms-full.txt` with a build script

**Status:** Accepted
**Date:** 2026-11-07

## Context

AI-agent discoverability requires `llms.txt` (a curated index of pages with descriptions) and `llms-full.txt` (the concatenated content) at the site root. Community plugins exist, but the docs site's content structure is small and stable, and a custom script avoids an extra dependency while giving full control over grouping (Diátaxis sections) and ordering (sidebar order).

## Decision

`scripts/generate-llms.mjs` runs after every `pnpm build` (the `build` script is `astro build && node scripts/generate-llms.mjs`). It:

1. Walks `src/content/docs/**/*.{md,mdx}`.
2. Reads each page's frontmatter `title` and `description`.
3. Emits `dist/llms.txt` — an H1, a site summary, then one section per Diátaxis type (Tutorials, How-to Guides, Reference, Explanation) listing `[title](URL): description`.
4. Emits `dist/llms-full.txt` — the same index followed by the full Markdown/MDX body of every page (frontmatter stripped, MDX import/component statements stripped) so a single fetch carries the whole site.

URLs are rooted at `/` (Starlight's directory structure maps 1:1 to routes; the landing page maps to `/`).

## Consequences

- Both files land in `dist/` on every build and deploy with the site root.
- No additional dependency; the script uses only the Node standard library.
- The script is content-agnostic: new pages appear automatically on the next build.
