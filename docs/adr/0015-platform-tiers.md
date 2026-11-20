# ADR-0015: Platform tiers

- **ADR ID:** ADR-0015
- **Status:** Proposed
- **Date:** 2026-11-20
- **Decide by:** see the master plan §8 register row ADR-0015
- **Source:** `docs/roadmaps/glyim-master-plan.md` §8 (decision register with proposed defaults)

## Context

The ledger's RG-19 and the CI matrix need a fixed answer to 'which targets are first-class?'. Every tier promise is a permanent CI and release cost (M8.8 adds per-target linker drivers), so the tier list must match actual CI capacity, not aspiration (risk R8).

## Decision

**Tier-1** (required green, release-blocking): linux-x86_64, linux-aarch64, macos-aarch64. **Tier-2** (best-effort, no release block): windows-x86_64. New targets enter through this ADR, never by silently growing the CI matrix.

This is the master plan's **proposed default** recorded at M0.4. The ADR is
*proposed*, not accepted: dependent work items must not start before the
decision is accepted with evidence (plan §7.2 rule 5), except where a
maintainer explicitly skips the spike and accepts the default as written.

## Consequences

CI jobs and release artifacts map 1:1 to declared tiers. Falsified if CI capacity cannot carry the tier matrix — the tier drops a level with an ADR revision, rather than the gate quietly rotting.
