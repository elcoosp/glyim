# ADR-0013: Evaluation & drop order

- **ADR ID:** ADR-0013
- **Status:** Proposed
- **Date:** 2026-11-20
- **Decide by:** see the master plan §8 register row ADR-0013
- **Source:** `docs/roadmaps/glyim-master-plan.md` §8 (decision register with proposed defaults)

## Context

HIR-29-class bugs come from unspecified evaluation order: operand evaluation and temporary drops that differ by opt level or by code path. The GAM (M9) cannot be written down at all without a fixed order, and differential testing needs the interpreter and LLVM to agree on evaluation order by construction, not by luck.

## Decision

**Fully specified left-to-right evaluation**; temporaries drop at the end of the enclosing statement. The rule is written into the GAM spec (M9.1) and enforced by tests across const-eval, interpreter, and LLVM backends.

This is the master plan's **proposed default** recorded at M0.4. The ADR is
*proposed*, not accepted: dependent work items must not start before the
decision is accepted with evidence (plan §7.2 rule 5), except where a
maintainer explicitly skips the spike and accepts the default as written.

## Consequences

The double-evaluation bug class is eliminated at the semantics level, not per-bug. Falsified if existing fixtures encode a different order — the fixtures are fixed (they were relying on unspecified behavior), not the rule.
