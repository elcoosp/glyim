# ADR-0001: Ledger data formats

- **ADR ID:** ADR-0001
- **Status:** Proposed
- **Date:** 2026-11-20
- **Decide by:** see the master plan §8 register row ADR-0001
- **Source:** `docs/roadmaps/glyim-master-plan.md` §8 (decision register with proposed defaults)

## Context

The readiness ledger (plan §3) replaced hand-written status prose with a computed status. To be diffable in CI, reproducible across machines, and greppable by agents, the on-disk formats had to be fixed once, before the registry, extractors, and renderers multiplied around them (M1.1 decides this). Human review needs text formats a reviewer can read in a PR diff; the level engine needs a pure function over stable inputs.

## Decision

Feature and gap registries are **TOML** (curated by hand, reviewed in PRs), evidence records are **JSONL** (machine-appended by the harness, one record per line, hermetic — no clock or PRNG in the payload), and derived outputs (STATUS.md, STATUS.json, README block, KNOWN_GAPS.md) are **JSON or Markdown rendered deterministically**. `compute` is a pure function of (registry, evidence, policy): same inputs, same bytes, on any machine.

This is the master plan's **proposed default** recorded at M0.4. The ADR is
*proposed*, not accepted: dependent work items must not start before the
decision is accepted with evidence (plan §7.2 rule 5), except where a
maintainer explicitly skips the spike and accepts the default as written.

## Consequences

The M1.13 reproducibility check can compare rendered bytes across runs; CI can gate registry edits with a schema check (`deny_unknown_fields`). Falsified if the determinism test fails across machines — then the formats or the engine must be tightened before more faces are built on them.
