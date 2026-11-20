# ADR-0010: Fast debug backend

- **ADR ID:** ADR-0010
- **Status:** Proposed
- **Date:** 2026-11-20
- **Decide by:** see the master plan §8 register row ADR-0010
- **Source:** `docs/roadmaps/glyim-master-plan.md` §8 (decision register with proposed defaults)

## Context

LLVM is the production backend but its compile speed floor is high; Cranelift exists exactly for fast debug-grade codegen. Adopting a second codegen doubles ABI/layout surface, so the decision needs a rubric, not enthusiasm: the debug backend must be worth its maintenance cost.

## Decision

Run a **Cranelift spike behind a rubric**: adopt as the fast debug backend if it delivers ≥2× backend speed vs LLVM-O0 and ≥25% e2e compile-time win on the bench corpus, with correct-enough debug output (ADR-014 line tables). Otherwise LLVM stays the only backend.

This is the master plan's **proposed default** recorded at M0.4. The ADR is
*proposed*, not accepted: dependent work items must not start before the
decision is accepted with evidence (plan §7.2 rule 5), except where a
maintainer explicitly skips the spike and accepts the default as written.

## Consequences

A second backend enters the ledger's parity matrix only if adopted. Falsified if the rubric numbers miss — the spike is closed and the ADR records why.
