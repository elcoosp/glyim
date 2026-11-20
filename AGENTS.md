# AGENTS.md — the agent operating protocol

This repository is developed by autonomous agents as the primary workforce
(humans review). The protocol below is the master plan's §7 operating protocol
(`docs/roadmaps/glyim-master-plan.md`) copied out as the single operative
document every agent must follow on every work item. When this file and the
plan drift, the plan wins and this file must be regenerated from it.

The ledger owns status. `ledger next` picks work, `ledger explain` says what a
task needs, `ledger diff` proves a change moved no readiness level backwards.
Status is computed, never typed (constitution C1).

## 1. The work loop

```
ledger next --lane <L>            # 1. pick the top item (deterministic)
ledger explain <ID>               # 2. what evidence is missing / what blocks it
git switch -c wi/<id>-<slug>      # 3. one work item per branch/PR
<write the witness / failing test FIRST>   # 4. TDD: strict-xfail fixture, proptest, or perf benchmark
<implement>                       # 5. smallest change that satisfies DoD
just verify                       # 6. fmt + clippy(deny) + tests + ledger check + perf-compare
ledger diff origin/main HEAD      # 7. paste into the PR: level changes, gap changes, perf deltas
```

## 2. Rules

1. **Never weaken a gate.** Thresholds only tighten. A temporary exception
   requires a `ledger/waivers.toml` entry with an issue link and an expiry
   ≤ 14 days; expired waivers fail CI.
2. **Witness first.** Bugs are added to the gap registry with a reproducing
   fixture *before* the fix. A fix that makes the witness pass must also delete
   the xfail marker and close the gap (CI enforces).
3. **Class-level defence.** Every bug-fix PR includes a bug-class record and
   the defence (type/constructor/validator/lint/oracle/generator/metamorphic).
   If the defence is large, open a prevention-debt item and link it; do not
   silently skip.
4. **Break freely, migrate fully.** Breaking changes are allowed. In the *same
   PR*: codemod script under `scripts/codemods/`, all call sites updated,
   `docs/migrations/NNNN-title.md`, ledger diff.
5. **ADR before architecture.** Any item marked ADR in the plan's §6/§8 starts
   with a time-boxed spike (≤ 1 week) and ends with an accepted ADR containing
   numbers. Do not start the dependent work items before acceptance (default
   decisions in §8 are used only if the spike is explicitly skipped by a
   maintainer).
6. **Stop-and-report triggers:** a gate fails and the cause is unclear after
   one bisect; a perf regression cannot be explained by the diff;
   TV/differential/fuzzing finds a miscompile (→ witness + class-b gap first,
   it outranks feature work); a change would exceed 800 changed lines outside
   codemod output (split it); an ADR assumption is falsified.
7. **No hand-written status prose.** Do not create `HANDOFF`/`PROGRESS`/`STATUS`
   markdown. Run `ledger handoff` — it emits the state from the ledger.
8. **Lane discipline.** Edit crates owned by your lane (`ledger/owners.toml`);
   cross-lane changes go through a small interface PR first.
9. **Compute budgets.** PR tier ≤ 20 min; nightly runs the heavy oracles
   (differential, TV, metamorphic, mutation sample); weekly runs soak/fuzz farms
   and full mutation testing. Never put a nightly-only check in the "required"
   set without a PR-tier smoke version.
10. **Flaky tests are bugs.** Quarantine with an expiry and a ledger entry;
    never retry-until-green.

## 3. Review checklist (reviewer = human or reviewer-agent)

1. Could a **type** have prevented this?
2. A **constructor**?
3. A **validator** at a phase edge?
4. An **exhaustive match**?
5. Only then: is there a test, a generator rule, a metamorphic relation?
6. Perf delta shown?
7. Deterministic under chaos mode?
8. Ledger diff reviewed?
9. Docs/`--explain` updated?
10. Is any new `unsafe`/`unwrap`/`panic` justified in the allow-list?

## 4. Universal definition of done (every work item)

- `cargo fmt --check`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo nextest run --workspace`
- `ledger check`
- `ledger diff` shows no regressions
- `perf-compare` within budget
- no new entries in panic/unsafe/wildcard/HashMap ratchets
- docs and migration note updated
- ADR linked if a decision was made

`just verify` runs the mechanical subset of this list locally; CI runs the
same gates (`.github/workflows/ci.yml`). A work item is done when every line
above is true — not when the code works.
