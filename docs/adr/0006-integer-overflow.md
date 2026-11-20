# ADR-0006: Integer overflow semantics

- **ADR ID:** ADR-0006
- **Status:** Proposed
- **Date:** 2026-11-20
- **Decide by:** see the master plan §8 register row ADR-0006
- **Source:** `docs/roadmaps/glyim-master-plan.md` §8 (decision register with proposed defaults)

## Context

Today overflow behavior is opt-level-dependent (debug checks, release wraps or UB by LLVM's reading), which quietly invalidates the differential oracles: the same program must behave identically at -O0 and -O2 for the comparison to mean anything. KNOWN_GAPS tracks overflow surprises as silent-wrong-behavior class-b gaps. The cost of always-checked arithmetic is real but recoverable: range analysis and bounds-check elimination (PF-30) remove most checks.

## Decision

**Checked arithmetic in all profiles by default**: `+ - * << >>` panic on overflow; `wrapping_*` / `checked_*` / `saturating_*` / `unchecked_*` are the explicit opt-outs; a crate-level `overflow = "wrap"` setting selects *defined* wrapping for code that wants it. Overflow is never opt-level dependent and never UB.

This is the master plan's **proposed default** recorded at M0.4. The ADR is
*proposed*, not accepted: dependent work items must not start before the
decision is accepted with evidence (plan §7.2 rule 5), except where a
maintainer explicitly skips the spike and accepts the default as written.

## Consequences

Differential oracles across -O levels become meaningful; the audit's overflow-class findings get a single semantics to witness against. Falsified if the runtime cost survives BCE/range analysis and is unacceptable (decided at M4.2) — then the default moves to defined-wrap for release with checks in debug, stated in a revised ADR.
