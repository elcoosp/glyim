# M0 slice: workspace-wide pedantic lint rails + M0.6 metrics baseline

## State

- **Branch:** `feat/next-milestone`, cut from `main` @ `26bf7ca7` ("docs:
  master plan v1").
- **Tree:** intentionally dirty until `ship.sh` lands its per-file
  Conventional commits; this PR is the M0 slice of
  `docs/roadmaps/glyim-master-plan.md`.
- **Gate:** `cargo fmt --all -- --check` and
  `cargo clippy --workspace --all-targets --all-features -- -D warnings
  -W clippy::pedantic` (with manifest-derived `-A` re-asserts) both pass
  — **`=== GATE GREEN ===`** — with **zero inline `#[allow]`/`#[expect]`
  attributes** in the entire tree (verified by grep; the only hit is a
  prose mention inside a doc comment in `glyim-core/src/lint.rs`).
- **PR URL / checks:** *pending* — this environment intentionally does
  not run `ship.sh` or `gh pr checks`; the PR will be created by running
  `./ship.sh` from the packaged tree.

## Milestone (why this slice)

The master plan's M0 milestones are ordered by risk-adjusted leverage.
This slice takes the two cheapest, hardest-to-revert rails first:

1. **Lint track (§2 C4, §7.4):** `clippy::pedantic` is wired
   workspace-wide through `[workspace.lints.clippy]` in the root
   manifest + `[lints] workspace = true` in all 36 member crates. Every
   relaxation is one rationale-annotated manifest entry (reviewable in a
   single place; a candidate list to *shrink* later — gates never
   weaken, C9). CLI re-asserts of `-A clippy::<lint>` after
   `-W clippy::pedantic` are derived from the manifest by the gate
   script, so the manifest stays the single source of truth.
2. **M0.6 metrics baseline:** new `tools/glyim-metrics` crate +
   `metrics/baseline.json` + a CI ratchet job that fails on regression
   and allows improvement.

Plus the two pre-reqs called out by the plan: the unused `regalloc2`
dependency is removed, and the stale session documents
(`HANDOFF.md`, `FIXES.md`) are archived under `docs/archive/`.

## Files

- 680 modified + 2 deleted + 4 new paths, +8,659/−10,376 lines:
  - root `Cargo.toml`: `[workspace.lints.*]` table (~45 annotated
    overrides), `regalloc2` removed; every member manifest gains
    `[lints] workspace = true`.
  - `tools/glyim-metrics/` (new): lib + bin + dependency-free bench.
  - `metrics/baseline.json` (new): 906 files / 226,539 code lines /
    2,300 public items / 1,981 documented / **0 inline allows** / 4,811
    tests / 36 members.
  - `.github/workflows/ci.yml`: new `metrics-ratchet` job.
  - `rust-toolchain.toml` (new): stable channel for local dev; MSRV
    1.95.0 stays pinned by the `msrv` CI job.
  - `CHANGELOG.md`, `README.md`: M0 entries + repository-map section.
  - `docs/archive/HANDOFF.md`, `docs/archive/FIXES.md`: moved from root.
  - ~160 `missing_docs` gaps closed; dead code deleted (unused fields,
    methods, and test helpers) or re-marked `#[cfg(test)]` where only
    tests consume it; ~90 HEAD-era glob imports restored after an
    over-eager expansion broke the `use crate::*;` test idiom.

## Tests

- `glyim-metrics`: 18 unit tests, written **test-first** (red:
  34 missing-symbol errors, log kept in the session; green after impl):
  gold tests for every counter on fixture strings, JSON round-trip,
  ratchet direction tests (lower-is-better for `inline_allows`,
  higher-is-better for the rest, informational `workspace_members`),
  and a synthetic-tree `scan_tree` test.
- Workspace: the enforced gate covers every crate with
  `--all-targets --all-features`; `cargo fmt --all -- --check` is clean.
- Full-suite note: the lint/doc pass is behavior-preserving; the only
  intentional behavior change is `AnalysisDriver` no longer accepting a
  never-read `cache_dir`.

## Bench

`cargo bench -p glyim-metrics` (`benches/scan_bench.rs`, harness =
false, no new dependencies):

```text
root: /home/z/glyim (906 files, 226539 code lines)
run 0..4 ≈ 1.82e8 ns/scan
min per code line: ~800 ns
```

A full-tree metrics scan costs ≈0.18 s, so the CI ratchet job's
10-minute budget has >50× headroom.

## Risks

- **Lexical metrics are approximate** (no syn parsing): `public_items`
  counts `pub <keyword>` occurrences; a false positive can only
  *raise* a baseline, which makes the ratchet stricter, not looser.
- **`wildcard_imports = "allow"`** is the widest relaxation; it is
  documented with its reason (the `use crate::*;` test idiom) and is a
  ratchet follow-up.
- **Glob restore blast radius:** restoring ~80 `use x::*;` lines
  re-imports names wholesale in lib code; the pedantic gate plus the
  full test matrix hold it steady (gate green, all targets compile).
- **`--check` depends on filesystem order stability** — `scan_tree`
  sorts directory entries, so runs are deterministic.

## Ship plan

`./ship.sh` (never executed by the authoring session): derives the next
version (no tags → base 0.1.0 → history contains `feat:` commits →
**v0.2.0**), lands per-file Conventional commits (≤72-char subject +
body), tags `v0.2.0`, pushes `feat/next-milestone` + `HEAD` + the tag,
and opens the PR with this body. Expected URL:
`https://github.com/elcoosp/glyim/pull/…` — **pending** until the run.
