# ADR-0009: Mid-end optimizer architecture

- **ADR ID:** ADR-0009
- **Status:** Proposed
- **Date:** 2026-11-20
- **Decide by:** see the master plan §8 register row ADR-0009
- **Source:** `docs/roadmaps/glyim-master-plan.md` §8 (decision register with proposed defaults)

## Context

M10 builds the optimizer. Two families exist: the classic pass suite (inlining, SCCP, GVN, DCE — predictable, debuggable, per-pass verifiable) and e-graph based equality saturation (ægraph) — powerful for superoptimization-shaped rewrites but harder to budget, debug, and gate per-pass. The plan needs per-pass verification windows and predictable compile-time budgets (R8).

## Decision

Ship the **classic suite first**; run **ægraph as an experimental pass behind an explicit gate**, evaluated against the §4.4 PF-32 budget before it ever becomes default.

This is the master plan's **proposed default** recorded at M0.4. The ADR is
*proposed*, not accepted: dependent work items must not start before the
decision is accepted with evidence (plan §7.2 rule 5), except where a
maintainer explicitly skips the spike and accepts the default as written.

## Consequences

M10.1–M10.6 port passes with verify-after-each-pass; translation validation windows stay small. Falsified if the gate shows ægraph beats the classic suite inside the PF-32 budget without verification blowup — then it is promoted per-pass with numbers.
