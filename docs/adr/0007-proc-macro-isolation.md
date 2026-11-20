# ADR-0007: Proc-macro isolation

- **ADR ID:** ADR-0007
- **Status:** Proposed
- **Date:** 2026-11-20
- **Decide by:** see the master plan §8 register row ADR-0007
- **Source:** `docs/roadmaps/glyim-master-plan.md` §8 (decision register with proposed defaults)

## Context

M5 proc macros run arbitrary compiled code inside the compiler process. An in-process ABI (like rustc's) makes every compiler upgrade a ABI break, couples macro panics to compiler panics, and makes sandboxing impossible. The plan (CT-10) needs isolation that survives compiler evolution and can be revoked (timeout, resource limits) without corrupting compiler state.

## Decision

**Out-of-process RPC**: proc macros compile to a separate dylib/executable and talk to the compiler over a versioned RPC protocol; WASM isolation is a stretch goal layered on the same protocol, not a different model.

This is the master plan's **proposed default** recorded at M0.4. The ADR is
*proposed*, not accepted: dependent work items must not start before the
decision is accepted with evidence (plan §7.2 rule 5), except where a
maintainer explicitly skips the spike and accepts the default as written.

## Consequences

Macro panics/aborts crash the macro process, not the compiler; the RPC schema becomes the compat contract instead of the C ABI. Falsified if RPC overhead dominates macro-heavy builds beyond the §4.4 budgets — then the protocol is optimized (batching, shared memory) before the model is abandoned.
