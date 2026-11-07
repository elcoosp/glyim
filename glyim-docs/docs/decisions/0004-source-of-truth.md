# ADR 0004: Source-of-truth policy for content

**Status:** Accepted
**Date:** 2026-11-07

## Context

Exploration of the Glyim repository surfaced contradictory documentation:

- `docs/TECHSTACK.md` names crates that do not exist in the workspace (`glyim-lex`, `glyim-parse`, `glyim-egraph`, `glyim-server`, `glyim-config`) and dependencies the code does not use (`salsa`, `chumsky`, `egg`). It reads as an aspirational specification, not a description of the code.
- `docs/design/closures.md` documents closures as unimplemented (verified at an old commit); the current README and code ship closure support with capture analysis.
- `docs/design/borrowck.md` is a research overview of rustc's approach rather than a description of Glyim's implementation.
- The README documents `glyim`'s flags but omits `--with-stdlib` and the lint flags `-A/-W/-D` (including the `-W help` listing), which exist in `crates/glyim-cli/src/lib.rs`.
- GSX is implemented as a library stage (`glyim-gsx`) with flagship end-to-end tests, but the `glyim-cli` driver does not run the GSX stage yet — a naive reading of the GSX design doc would overstate what a user can run today.

## Decision

Documentation content follows this precedence, in order:

1. **Source code** (`crates/**`) — the argument parsers, lint registry, manifest schema, and public APIs are verified file-by-file before being documented.
2. **README.md**, in particular its "Project Status" section — the maintainer's current-reality statement.
3. **`docs/design/` and other repository docs** — treated as proposals or history; used only where they agree with code or are explicitly labeled as design intent, and never presented as shipped behavior.

The documentation-debt items found during exploration (undocumented CLI flags, stale TECHSTACK.md, GSX driver wiring) are documented honestly in the site: the CLI reference includes the undocumented flags; the GSX pages carry an experimental caution and describe the library-stage usage path.

## Consequences

- No feature is documented that the source does not implement.
- `docs/TECHSTACK.md` is not used as a source; a future sweep may either update it to match code or retire it (repository-side decision, out of scope for the docs site).
- When the CLI driver gains GSX stage wiring, the GSX pages' caution blocks must be updated — the `Status and scope` section in the GSX reference is the single place tracking this.
