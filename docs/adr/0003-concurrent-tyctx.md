# ADR-0003: Concurrent `TyCtx`

- **ADR ID:** ADR-0003
- **Status:** Proposed
- **Date:** 2026-11-20
- **Decide by:** see the master plan §8 register row ADR-0003
- **Source:** `docs/roadmaps/glyim-master-plan.md` §8 (decision register with proposed defaults)

## Context

M7 parallelizes typeck, borrowck, MIR build/opt, and monomorphization. Today's `TyCtx` assumes single-threaded access: a shared interner, shared arenas, interior mutability without a discipline. Rust gives three ways to share mutable state across threads — locks (simple, contended), sharding (fast, more code), and unsafe wrappers (fast, unsound unless proven). The compiler's hot paths make coarse locks measurably slow, and unsound wrappers are exactly the bug class this plan is trying to eliminate (constitution C6).

## Decision

Concurrent `TyCtx<'tcx>` uses a **sharded interner + append-only arenas** with a `'tcx` lifetime tying all allocations to the compilation session, and `Ty(u32)`-style index handles instead of shared `Rc<RefCell<..>>` graphs. Loom tests cover the sharded structures; `unsafe` stays inside a tiny, audited core or disappears.

This is the master plan's **proposed default** recorded at M0.4. The ADR is
*proposed*, not accepted: dependent work items must not start before the
decision is accepted with evidence (plan §7.2 rule 5), except where a
maintainer explicitly skips the spike and accepts the default as written.

## Consequences

M7.3+ stages parallelize against this shape; determinism (chaos mode, RG-04) is testable because shard inserts are order-independent. Falsified if loom reports a race the design cannot fix, or if perf results show sharding overhead beats the win — then the concurrency model is revised before M7 lands.
