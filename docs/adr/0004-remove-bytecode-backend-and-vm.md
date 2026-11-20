# ADR-0004: Remove bytecode backend + VM

- **ADR ID:** ADR-0004
- **Status:** Proposed
- **Date:** 2026-11-20
- **Decide by:** see the master plan §8 register row ADR-0004
- **Source:** `docs/roadmaps/glyim-master-plan.md` §8 (decision register with proposed defaults)

## Context

The workspace carries three executors: the MIR interpreter, the bytecode VM, and LLVM codegen. The bytecode VM duplicates semantics the interpreter already implements, costs ≈9k LOC of maintenance surface, and splits every parity question (which constructs behave identically?) into a third column. The plan's oracle strategy (CO-1: the interpreter is the reference semantics; LLVM/Cranelift are the production codegens) leaves no unique role for a bytecode executor — unless the ledger shows a supported use case only it covers.

## Decision

**Remove** the bytecode backend and VM (M5.8, ADR-gated). One fewer executor: the interpreter is the oracle, LLVM (and later Cranelift) are the codegens. The ledger's construct-support matrix loses the bytecode column instead of carrying three-way parity forever.

This is the master plan's **proposed default** recorded at M0.4. The ADR is
*proposed*, not accepted: dependent work items must not start before the
decision is accepted with evidence (plan §7.2 rule 5), except where a
maintainer explicitly skips the spike and accepts the default as written.

## Consequences

≈9k LOC and one whole executor disappear; tests migrate to the interpreter or LLVM backends. Falsified if the ledger shows a unique supported use case with a maintainer who needs bytecode execution (e.g., a sandbox without an interpreter) — then the removal is re-scoped instead of executed.
