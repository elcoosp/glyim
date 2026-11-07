# Glyim Documentation Site

Production documentation for [Glyim](https://github.com/elcoosp/glyim) — a modular, from-scratch compiler for a Rust-like systems programming language, written in Rust.

Built with [Astro Starlight](https://starlight.astro.build) (Astro 7.4, Starlight 0.42), organized following the [Diátaxis](https://diataxis.fr) framework.

## Quickstart

Requires Node.js 22+ and pnpm 10.

```bash
pnpm install
pnpm dev      # local dev server
pnpm build    # production build + llms.txt generation + link validation
pnpm check    # astro check (TypeScript 6.x)
pnpm lint     # markdownlint over all docs
pnpm preview  # serve the production build
```

## Layout

```
src/content/docs/        Diátaxis content
├── index.mdx            Landing page (splash)
├── tutorials/           Learning-oriented (golden path)
├── guides/              Task-oriented how-tos
├── reference/           Information-oriented reference
└── explanation/         Understanding-oriented design docs
src/assets/              Logo (light/dark variants)
src/styles/custom.css    Brand accent overrides
public/                  favicon.png, robots.txt
scripts/                 generate-llms.mjs (llms.txt / llms-full.txt)
docs/AGENTS.md           Authoring rules for agents and humans
docs/decisions/          Architecture decision records (ADRs)
```

## Tooling

- **Search**: Pagefind (built into Starlight, zero config).
- **Link checking**: `starlight-links-validator` fails `pnpm build` on broken internal links.
- **Plugins**: `starlight-image-zoom`, `starlight-heading-badges`.
- **AI discoverability**: `pnpm build` regenerates `dist/llms.txt` and `dist/llms-full.txt`.
- **TypeScript**: pinned to 6.x — `astro check` does not support TypeScript 7 (see `docs/decisions/0001-typescript-version.md`).

## Content rules

See `docs/AGENTS.md`. Short version: one Diátaxis type per page, every claim verified against the compiler's source code, never invent features, mark experimental features and tracked gaps explicitly.
