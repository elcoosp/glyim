# ADR-0002: Query engine

- **ADR ID:** ADR-0002
- **Status:** Proposed
- **Date:** 2026-11-20
- **Decide by:** see the master plan §8 register row ADR-0002
- **Source:** `docs/roadmaps/glyim-master-plan.md` §8 (decision register with proposed defaults)

## Context

M6 moves the compiler onto incremental queries: parsing, def-collection, typeck, MIR, mono. Two credible architectures exist — adopting an existing engine (Salsa, used by rust-analyzer and rustc's successor prototypes) or building a purpose-built one. Hand-rolling an incremental engine repeats a known-hard problem (cycles, cancellation, backdating, persistence) inside a compiler that already has enough of them, but a framework can also impose overhead the hand-rolled version avoids. The decision needs numbers, not taste, before M6 work starts (plan R2).

## Decision

Adopt **Salsa** for the query engine, with persistence added at the CGU/object-cache layer rather than persisting Salsa's dep-graph. A custom engine is chosen only if the M6.0 spike shows >15% straight-line overhead, or missing parallel/cancellation support that Salsa cannot grow.

This is the master plan's **proposed default** recorded at M0.4. The ADR is
*proposed*, not accepted: dependent work items must not start before the
decision is accepted with evidence (plan §7.2 rule 5), except where a
maintainer explicitly skips the spike and accepts the default as written.

## Consequences

M6.1–M6.10 are built on query APIs from day one. Falsified if the M6.0 spike numbers (overhead, memory, cycle handling, parallelism, cancellation, durability) favor a custom engine — the ADR is then superseded before dependent WIs start.
