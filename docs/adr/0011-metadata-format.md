# ADR-0011: Metadata format (`.gmeta`)

- **ADR ID:** ADR-0011
- **Status:** Proposed
- **Date:** 2026-11-20
- **Decide by:** see the master plan §8 register row ADR-0011
- **Source:** `docs/roadmaps/glyim-master-plan.md` §8 (decision register with proposed defaults)

## Context

M8 separate compilation needs an on-disk metadata format for crates: defs, signatures, generics, trait/impl indexes, const values, inlinable MIR, stable hashes. The format is read on every downstream compile, so decode speed and zero-copy access matter; but it must also validate corrupt input without crashing (the decoder is fuzz-targeted, M8.2). Off-the-shelf serde formats buy convenience but pay in decode cost and validation story.

## Decision

**Custom little-endian offset tables with `zerocopy`**: fixed-layout tables, mmap-able, checksummed per section, versioned. A `rkyv`-like archive is chosen only if it matches the speed *and* gives the same validation story.

This is the master plan's **proposed default** recorded at M0.4. The ADR is
*proposed*, not accepted: dependent work items must not start before the
decision is accepted with evidence (plan §7.2 rule 5), except where a
maintainer explicitly skips the spike and accepts the default as written.

## Consequences

The decoder fuzz target (M8.2) can guarantee no crash on corrupt input; cross-crate compile reads stay O(tables) not O(parse). Falsified if decode/validation benchmarks show a mature library beats the custom tables on both speed and validation — adopt it then.
