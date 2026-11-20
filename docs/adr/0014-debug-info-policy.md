# ADR-0014: Debug-info policy

- **ADR ID:** ADR-0014
- **Status:** Proposed
- **Date:** 2026-11-20
- **Decide by:** see the master plan §8 register row ADR-0014
- **Source:** `docs/roadmaps/glyim-master-plan.md` §8 (decision register with proposed defaults)

## Context

Debug info generation is a speed/size tax on every compile and a compatibility surface (DWARF vs CodeView) per target. The compiler needs a minimum bar (stack traces and line tables must always be right — panics without line numbers are class-a diagnostics bugs) and an opt-in bar (full variable info for debuggers) without making the default build carry the whole cost.

## Decision

**DWARF/CodeView line tables always emitted** (per-target format); full variable info only at `-g`. The always-on part is sized to stay inside the compile-time budgets (§4.4).

This is the master plan's **proposed default** recorded at M0.4. The ADR is
*proposed*, not accepted: dependent work items must not start before the
decision is accepted with evidence (plan §7.2 rule 5), except where a
maintainer explicitly skips the spike and accepts the default as written.

## Consequences

Backtraces and crash reports are debuggable by default; `-g` remains the explicit opt-in for heavy debug info. Falsified if line-table emission measurably breaks the §4.4 budgets — then the default is revisited with numbers.
