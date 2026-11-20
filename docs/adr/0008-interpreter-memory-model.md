# ADR-0008: Interpreter memory model

- **ADR ID:** ADR-0008
- **Status:** Proposed
- **Date:** 2026-11-20
- **Decide by:** see the master plan §8 register row ADR-0008
- **Source:** `docs/roadmaps/glyim-master-plan.md` §8 (decision register with proposed defaults)

## Context

M9 rebuilds the GAM interpreter on a memory model that can detect UB. The value-model interpreter today cannot express allocation identity, so use-after-free and pointer-identity questions are unrepresentable. Miri's design (byte-addressed memory with allocation provenance) is the proven shape for exactly this job; full aliasing models (Tree-Borrows et al.) are a large increment on top and only matter once `noalias` optimizations are emitted.

## Decision

**Byte-addressed memory + provenance**, Miri-style: every allocation has identity and provenance, loads/stores go through layout, uninit reads and dangling use are detectable. The aliasing model is optional (`--ub-check=aliasing`) and deferred behind PF-18.

This is the master plan's **proposed default** recorded at M0.4. The ADR is
*proposed*, not accepted: dependent work items must not start before the
decision is accepted with evidence (plan §7.2 rule 5), except where a
maintainer explicitly skips the spike and accepts the default as written.

## Consequences

UB-class coverage becomes measurable in the ledger (RG-20); const-eval shares the same engine (M9.5). Falsified if the prototype cannot meet the const-eval speed gate — then the model is kept but the hot paths are reworked before UB detection is layered on.
