# ADR-0012: Hashing policy

- **ADR ID:** ADR-0012
- **Status:** Proposed
- **Date:** 2026-11-20
- **Decide by:** see the master plan §8 register row ADR-0012
- **Source:** `docs/roadmaps/glyim-master-plan.md` §8 (decision register with proposed defaults)

## Context

Two failure classes to prevent at once: (1) nondeterminism — std `HashMap` iteration order leaks into output unless maps are ordered or hash seeds are fixed, and the plan's determinism rules (CT-9) forbid that; (2) HashDoS — hostile identifiers colliding under a fixed/weak hash. Different maps need different answers: hot in-memory maps want speed; output-affecting maps want stable order; content fingerprints want a real hash, not a speed hash.

## Decision

**FxHash/foldhash for in-memory maps** (fast, seedless), **`FxIndexMap` where iteration order feeds output** (deterministic), **blake3 for content fingerprints** (stable hashes, incremental), and explicit resource limits on any hash exposed to hostile input.

This is the master plan's **proposed default** recorded at M0.4. The ADR is
*proposed*, not accepted: dependent work items must not start before the
decision is accepted with evidence (plan §7.2 rule 5), except where a
maintainer explicitly skips the spike and accepts the default as written.

## Consequences

M3.9's determinism lint can name the disallowed types; fingerprint stability across versions becomes a testable property. Falsified if HashDoS is reproduced on hostile identifiers under the chosen in-memory hashes — then seedless maps get keyed behind limits or move to a keyed hash at those sites.
