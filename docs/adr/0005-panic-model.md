# ADR-0005: Panic model: abort only

- **ADR ID:** ADR-0005
- **Status:** Proposed
- **Date:** 2026-11-20
- **Decide by:** see the master plan §8 register row ADR-0005
- **Source:** `docs/roadmaps/glyim-master-plan.md` §8 (decision register with proposed defaults)

## Context

KNOWN_GAPS §2/§4 record the current unwind story: `glyim_panic` already exits 101, SEH cleanup edges are unreachable dead code, and the interpreter's unwind path is off and breaks 14 tests. Keeping half-working unwinding means the compiler emits landing pads and cleanup edges nothing exercises, and the language semantics carry a drop-on-unwind layer the oracle cannot verify. Implementing unwinding *completely* is a large deferred project with no current user asking for it.

## Decision

**Abort only.** Panics abort the process; the unwinding machinery (landing pads, cleanup edges, drop-on-unwind semantics) is deleted. `catch_unwind`-style semantics are not part of the language. The alternative — implementing unwinding completely — stays deferred and would need its own ADR.

This is the master plan's **proposed default** recorded at M0.4. The ADR is
*proposed*, not accepted: dependent work items must not start before the
decision is accepted with evidence (plan §7.2 rule 5), except where a
maintainer explicitly skips the spike and accepts the default as written.

## Consequences

A whole semantic layer (funclets, cleanup edges) leaves codegen and the GAM; the 14 broken interpreter tests lose their xfail cause. Falsified if a real requirement for `catch_unwind` semantics in std/threads appears (decide by M9.8, discussed at M4.2) — then unwinding is designed completely, not resurrected piecemeal.
