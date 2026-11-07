# AGENTS.md — Documentation Authoring Rules

This directory contains the Glyim documentation site (Astro Starlight). Any agent or human adding pages here follows the rules below.

## Information architecture (Diátaxis)

One type per page. Mixing types on one page is forbidden.

| Type | Folder | Purpose |
|------|--------|---------|
| Tutorial | `src/content/docs/tutorials/` | Learning-oriented: one golden path, zero assumptions. |
| How-to | `src/content/docs/guides/` | Task-oriented: solve a specific problem for a reader who knows the basics. |
| Reference | `src/content/docs/reference/` | Information-oriented: complete, precise, table-first, no narrative. |
| Explanation | `src/content/docs/explanation/` | Understanding-oriented: design rationale and background. |

Every page's frontmatter carries `title`, `description` (one sentence, SEO/AI-ready), and `type` (`tutorial`, `how-to`, `reference`, or `explanation`).

## Source-of-truth policy

- Never invent features, flags, options, or diagnostics. Verify every claim against the source code in the Glyim repository before writing it.
- The repository's `README.md` "Project Status" section reflects current reality. Files under `docs/design/` are proposals or historical notes; `docs/TECHSTACK.md` is stale (it names crates that do not exist, e.g. `salsa`, `egg`, `glyim-egraph`, `glyim-server`). When they disagree with code or README, code wins.
- Mark experimental features (GSX) and tracked gaps explicitly with `<Aside type="caution">`.

## Writing style

- Second person ("you"), present tense, active voice.
- Imperative for instructions ("Run `pnpm build`").
- Never use the words "simply", "just", "easy", or "obviously".
- Every page ends with a "Next steps" section using `<LinkCard>`.
- Code blocks always specify a language tag (`bash`, `glyim`, `toml`, `rust`, `json`).
- Reference pages: signature, type, default, required?, description, example — as tables.

## Tooling

- Package manager of record: **pnpm**. No npm or Yarn lockfiles.
- `pnpm build` must pass: broken internal links fail the build (`starlight-links-validator`).
- `pnpm check` must pass: TypeScript is pinned to **6.x** because `astro check` does not support TypeScript 7 (see `docs/decisions/0001-typescript-version.md`).
- `pnpm lint` runs markdownlint over all docs.
- `pnpm build` regenerates `llms.txt` and `llms-full.txt` at the site root via `scripts/generate-llms.mjs`.

## Architecture decisions

Record any non-obvious tooling or content decision as an ADR in `docs/decisions/` (numbered `NNNN-kebab-case-title.md`). Never edit an accepted ADR; supersede it with a new one.
