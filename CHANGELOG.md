# Changelog

All notable changes to Glyim are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added
- **Status renderers (M1.9 first slice)**: `tools/glyim-ledger/src/status.rs`
  renders the M1.8 engine's outcome as the two plan-M1.9 artifacts that
  hang directly off it: the `glyim.status/1` JSON contract (per-level
  histogram + per-feature summary entries; the compute contract keeps the
  `missing`-reason prose) and the marker-delimited Markdown block
  (`<!-- ledger:status:start/end -->`) the "generated content" embedding
  step needs. Both are pure functions of the engine outcome — no clock,
  entries already sorted — pinned by golden tests, a summary-vs-entries
  cross-check, and byte-identical repeat renders. Wired as
  `glyim-ledger status [ROOT] [--evidence PATH] [--format md|json]`,
  with the `status_report` example and the dependency-free `status`
  bench; the "STATUS is up to date" CI gate and the remaining M1.9
  renderers (`KNOWN_GAPS.md` generation, Astro page) are later slices.
- **Level-computation engine (M1.8 slice)**: `tools/glyim-ledger/src/compute.rs`
  implements the plan's `ledger compute` as a pure function of
  *(registry, evidence, policy)*. Per feature it walks the L0–L4 evidence
  ladder (level N requires every rule of levels L1..N; a level whose rules
  are all backend-gated away stops the ladder instead of advancing
  vacuously), applies every open gap's `caps` ceiling (`capped_by` names
  the cappers), and reports the `[hardening]` axes (`fuzz_cpu_hours`,
  `mutation_kill_rate_pct`) as `unmeasured` for L5 targets — no evidence
  kind records them yet, so nothing silently claims hardening. The
  policy schema (`glyim.ledger-policy/1`, `ledger/policy.toml`) lives in
  `tools/ledger-schema/src/policy.rs` beside the other strict registry
  schemas; evidence enters through the `EvidenceSource` seam
  (`ProjectEvidenceSource` over `ledger/evidence/*.jsonl`,
  `JsonlEvidenceSource` for explicit paths, `InMemoryEvidenceSource` for
  tests), reading the harness's `glyim.evidence/1` wire format strictly
  (`deny_unknown_fields`, schema-tag check, hash/mode/backend invariants)
  without a dependency on the test-harness crate. Determinism and
  monotonicity are pinned by golden tests and a deterministic-PRNG
  property test (evidence growth never lowers a computed level); the
  `glyim.ledger-compute/1` JSON contract (`--format json`) and the text
  report render the same outcome. Wired as `glyim-ledger compute [ROOT]
  [--evidence PATH] [--format text|json]`, with the
  `compute_levels` example, the dependency-free `compute` bench, the
  `TOOL-LEDGER-COMPUTE` registry row, and README/CLI docs.
- **Corpus substrate (M2.3 slice 1)**: `tools/glyim-bench/src/corpus.rs`
  adds the seven deterministic synthetic generator families the master
  plan's M2.3 calls for (wide, deep, generic, trait, match, macro,
  closure) as pure functions of `(family, seed, size)` — no clock, no
  ambient PRNG — each mirroring a shape proven by an existing run-pass
  fixture, with the reference exit code derived by construction under
  documented `i32` overflow bounds. The `glyim.corpus/1` manifest
  (`deny_unknown_fields`, byte-deterministic rendering) records family,
  seed, size, exit, compile flags, and the FNV-1a-64 identity of the
  source; `CorpusManifest::verify` regenerates every row and fails with a
  typed `CorpusIdentityMismatch` — the machine-checked "corpora committed
  with hashes" guarantee. New seams carry `tracing` spans
  (`corpus_generate`, `corpus_manifest_build`) and miette help text
  (`CorpusPlan`, `CorpusManifestParse`, `CorpusIdentityMismatch`). Tests
  pin determinism, exit-code contracts, JSON round-trip/strictness, drift
  detection, and FNV vectors; bench `glyim-bench --bench corpus_gen`
  pins build/verify/marginal cost; new consumer example
  `tools/glyim-bench/examples/corpus_walk.rs`; ledger row
  `TOOL-BENCH-CORPUS-GEN`.
- **Micro-debt cleanup**: `glyim-test`'s `TestMode: FromStr` now carries a
  typed `TestModeError` (miette `help()` with the canonical mode list;
  `Display` text byte-identical to the former string error, so substring
  assertions keep passing) instead of `Err = String` — the same debt class
  as the ledger/metrics CLI errors — and `tools/glyim-pilot`'s HTTP
  control-plane bind degrades to a logged error instead of
  `unwrap()`-panicking the detached task (constitution C4).
- **Docs drift fixes**: the README repository map now lists the
  `glyim-ledger gaps` subcommand (landed with the M1.7 witness-debt
  report), records the M2.3 slice-1 state, and lists the `corpus_walk`
  example; the degenerate `/// Struct.` / `/// Variant.` placeholder
  rustdocs in `crates/glyim-test/src/harness/config.rs` are replaced with
  real field docs (the workspace-wide placeholder sweep — ~2.6k sites —
  is tracked as a docs follow-up).
- **Gap registry migration (M1.7 slice)**: the open `KNOWN_GAPS.md` §§1–8
  entries are now registry rows under `ledger/gaps/known/` — the seed's
  GAP-0001/GAP-0002 join **GAP-0003** (§4, `panic!` does not unwind —
  intentional, documented), **GAP-0004** (§6, a partial move out of a
  `Drop`-implementing type suppresses the parent destructor — witnessed) and
  **GAP-0005** (§6, conservative `needs_drop` for `dyn Trait`/projections).
  New `glyim-ledger gaps [ROOT]` subcommand renders the witness-debt report
  (`GapOutcome` / `run_gaps` / `format_gap_report`), with a consumer example
  (`tools/glyim-ledger/examples/gap_report.rs`), a dependency-free bench
  (`benches/gap_report.rs`), the `TOOL-LEDGER-GAPS` registry row, and
  `tracing` spans on the new seam. `KNOWN_GAPS.md` stays the prose audit
  (verification recipes + closed entries) and points at the registry; the
  ~6.1k-line raw session log that had accreted after its Verification
  section moved to `docs/archive/session-log-s16-s18.md`.
- **Measurement harness (M2.2 slice)**: new `tools/glyim-bench` crate turns
  a command (or an in-process workload) into numbers with a stability
  guarantee — the M2.2 definition of done is a *variance* budget, not a
  speed number: instruction-count CoV < 0.1 % over 10 runs.
  - `glyim-bench run [--iterations N] [--source perf|callgrind|wall]
    [--json OUT] -- CMD...` executes the command N times and prints the
    report; `--source perf` wraps the child in `perf stat -x, -e
    instructions,cycles`, `--source callgrind` in `valgrind --tool=callgrind`,
    and the default auto-detects `perf`, falling back to wall-only sampling
    (logged at `info`, never silently). Child RSS comes from
    `getrusage(RUSAGE_CHILDREN)` (Linux; the running maximum over reaped
    children). `glyim-bench gate [--max-cov-pct P] REPORT` re-checks a
    written report against the budget and exits non-zero on failure.
  - New `glyim_bench` library seams: `runner::run_iterated` (one
    `BenchSample` per iteration), `source` parsers for `perf stat -x,` CSV
    and the callgrind `summary:` line, `stats` (min/mean/median/max +
    population CoV, `DEFAULT_MAX_COV_PCT = 0.1`), `alloc::CountingAllocator`
    (opt-in `counting_allocator!` macro; gross allocation events/bytes, the
    columns the M1.5 evidence schema reserved for M2), and `report`
    (`glyim.bench/1` JSON with `deny_unknown_fields`, no clock/PRNG data,
    byte-deterministic rendering — same discipline as the M1.5 evidence
    records).
  - Errors on the new seams are typed `BenchError` diagnostics with
    actionable `help()` text (a missing `perf` suggests `--source wall`; a
    gate failure suggests raising `--iterations`); the runner emits
    `bench_run` spans and per-sample `debug!` events.
  - Tests: CoV vectors, gate boundaries, both counter parsers (with
    `<not counted>` / `<not supported>` handling), report round-trip +
    unknown-field rejection + determinism, and (unix) runner end-to-end over
    `sh -c`. New bench `glyim-bench --bench runner_overhead` pins the
    harness's own cost; new consumer example
    `tools/glyim-bench/examples/measure_alloc.rs` walks the in-process
    counting-allocator path.
  - Ledger: the `tooling.bench` area joins the taxonomy and
    `ledger/features/tooling/bench.toml` declares the harness surface
    (`TOOL-BENCH-RUN`, `TOOL-BENCH-COUNTING-ALLOC`, `TOOL-BENCH-STABILITY-GATE`).
- **Micro-debt cleanup**: `tools/glyim-ledger`'s `parse_args`/`write_schema`
  return a typed `LedgerCliError` and `tools/glyim-metrics`' CLI a typed
  `MetricsCliError` (both `miette::Diagnostic` with per-variant `help()`
  lines) instead of bare `Result<_, String>`; tests now assert on
  constructors rather than message substrings, and CLI output still carries
  the usage text plus a `help:` line.
- **Docs drift fixes**: the README repository map no longer lists the
  already-shipped M2.1 slice as a "next plan item" (it now records the
  landed M2.1/M2.2 state and points at M1.7/M2.3 as next), the "Tracked
  gaps" async bullet no longer claims loop-await state machines are
  implemented (`.await` in a loop body is the diagnosed `[T0060]` hard
  error per `KNOWN_GAPS.md` §1), the member count is 39 (34 + 5 tooling)
  with a `tools/glyim-bench` row, `docs/TECHSTACK.md` re-anchors at
  workspace 0.7.1 (`ba2a526e`) and records the `miette`/`clap`/`tracing`
  consumer additions and the `libc` per-crate extra.
- **Per-phase instrumentation (M2.1 slice)**: every pipeline phase
  (load → parse → expand → defmap → hir → typeck → lower → borrowck →
  opt → mono → codegen) now opens a `tracing` span and accumulates
  wall-clock microseconds into a `glyim_pipeline::PhaseProfile`, exposed
  on `CompileArtifacts` / every emit path.
  - `glyim` CLI: `--time-passes` prints the fixed-width per-phase table
    on stderr; `--self-profile[=FILE]` writes a Chrome Trace event list
    (Perfetto / `chrome://tracing`-openable; value is `=`-bound so the
    positional INPUT can never be swallowed, bare form defaults to
    `./trace.json`); `--emit=stats` runs the analysis-only front half and
    writes a schema-versioned (`version: 1`) JSON report with the phase
    table plus mono/warning counters. A compile-cache hit records no
    phases and says so instead of printing an empty table.
  - New `glyim_pipeline::Pipeline::compile_stats` public API backs
    `--emit=stats`; `emit_mir`/`emit_syntax`/`emit_llvm_ir`/`emit_asm`
    now return the profile so every emit mode can honour
    `--time-passes`/`--self-profile`.
  - Determinism contract (unit-tested): the phase set/order is pinned,
    all three renderers are byte-deterministic for a given profile, and
    an empty profile renders a valid zeroed report (no NaN).
  - Errors on the new seams carry miette help text (e.g. an unwritable
    `--self-profile` path suggests the remedy) and the write path emits
    `tracing::debug!` events.
  - New consumer example `crates/glyim-pipeline/examples/phase_profile.rs`
    drives `compile_stats` + all three renderers as a library consumer;
    `hello_pipeline` now also prints the phase table. New bench
    `glyim-pipeline --bench phase_prof` measures the renderer cost
    (sub-µs per render — the instrumentation gate is effectively free).
- **Micro-debt cleanup**: `glyim-test`'s fixture-header parser
  (`harness::config::parse_test_config`) returns a typed
  `TestConfigError` (miette `Diagnostic` with an actionable `help` line
  naming the `//@` grammar and the tag codemod) instead of a bare
  `String`; the stale rustdocs in `glyim-hir/src/lower/lower_async.rs`
  that claimed the multi-await v1 desugar was an unwired scaffold (and
  referenced a nonexistent `allow(dead_code)`) now describe the live
  dispatch; the malformed `KNOWN_GAPS` reference in the
  `glyim-mir-interp` crate docs is fixed.
- **Docs drift fixes**: README's `glyip` quickstart no longer documents a
  nonexistent `-O2` flag (`glyip build` takes `--opt-level`), the CLI
  flag table gains `--with-stdlib`, `-A/-W/-D`, `--emit=syntax`, and the
  new M2.1 measurement flags, the stdlib module lists match the actual
  `lib/` directories (`parse`, `alloc`, `int_str`), the "Tracked gaps"
  section no longer claims cross-frame unwinding is missing (it is
  implemented and hardened — see `KNOWN_GAPS.md`), and the repository
  map describes the real milestone state (M0 landed, M1.1–M1.6 landed)
  instead of "M0 slice". `docs/TECHSTACK.md` now records `async-lsp` in
  the §2 table (previously claimed absent while `glyim-lsp` depends on
  it), fixes the `clap` consumer list (`glyim-cli`/`glyip`/
  `glyim-pilot` — not `glyim-ledger`), corrects the
  `reqwest`/`flate2`/`tar` versions, updates `miette`'s consumers, and
  re-anchors its source-of-truth header at workspace 0.7.0.
  CONTRIBUTING no longer claims `RUSTC_WRAPPER=kache` is set in the
  default dev environment (it is maintainer-machine-specific), and the
  stale `glyim-test` manifest note about the (now closed) async
  `fn_abi_of` codegen gap is reconciled with CI.
- **Evidence JSONL emitter (M1.5 slice)**: `glyim-test` now emits one
  readiness-ledger evidence record per executed fixture when
  `GLYIM_TEST_EVIDENCE` names an output path — the machine-checked
  input `ledger compute` (M1.8) will consume, per plan §3.4/§6 M1.5.
  - `harness::evidence`: typed `EvidenceRecord` (`glyim.evidence/1`) with
    outcome/backend enums from the ledger's backend set, SHA-256 fixture
    hashes over the exact source bytes, mode, revision, declared
    `//@ feature:`/`//@ backends:` tags, and stable short reason classes
    (free-form compiler output deliberately dropped so message refactors
    cannot move ledger statistics). `instructions`/`rss_kb` are reserved
    (always `None` until M2's counting-allocator instrumentation).
  - Hermetic by construction: no clock or PRNG data —
    `TestResult::duration` never reaches a record, and two runs against
    the same fixtures serialise byte-identically (tested). Records
    deserialise strictly (`deny_unknown_fields`, same discipline as
    `ledger-schema`), and `write_evidence` validates the whole slice
    before writing so a schema bug cannot poison the evidence store.
  - `tracing` spans (`evidence_write`, record/byte count events) on the
    emission path, matching the harness's existing span style; failures
    render through `miette` with actionable `help` text (the remedy names
    `GLYIM_TEST_EVIDENCE` or the schema rule that was violated).
  - Bench: `glyim-test --bench evidence` measures a full 375-fixture
    evidence pass (record construction ≈0.6 ms, serialisation ≈0.2 ms,
    validated write ≈0.6 ms) — the M1.5 gate is effectively free.
  - New consumer examples: `crates/glyim-test/examples/evidence_report.rs`
    round-trips the JSONL contract (producer + validator + reason-class
    aggregate) and `crates/glyim-pipeline/examples/hello_pipeline.rs`
    drives the compiler as a library (write a `.g` program, lower it to
    MIR in-process).
- **Micro-debt cleanup**: the harness JSON reporter no longer swallows
  failures — `GLYIM_TEST_JSON` write/serialise errors now print a
  miette-rendered diagnostic instead of vanishing into `let _ =`; the
  dead workspace dependencies `textwrap`, `owo-colors`, and `petgraph`
  (declared in `[workspace.dependencies]`, referenced by no member,
  absent from `Cargo.lock`) are removed; the stale committed
  `Cargo.lock` (members at 0.5.1 vs workspace 0.6.0) is regenerated.
- **Docs drift fixes**: README's architecture table gains the missing
  `glyim-gsx` row and a tooling row for `tools/*` (the workspace has 38
  members: 34 crates + 4 tools); CONTRIBUTING says Rust ≥ 1.95 (was
  1.85), replaces the inline-`#[allow]` guidance with the M0
  zero-allow policy (relaxations only in `[workspace.lints.clippy]`),
  documents the M1.4 `//@` fixture grammar instead of the legacy
  `// key:` headers, and updates the corpus count (~260 → ~410);
  `docs/TECHSTACK.md` is rewritten (v2.0.0) to list only dependencies
  the manifests actually declare (the previous revision listed
  `salsa`/`chumsky`/`egg`/`regalloc2`/`async-lsp` and nonexistent
  crates); the master plan's F1 evidence pointer now reflects that the
  production-readiness playbook has been retired from `docs/roadmaps/`.
  The M0 metrics ratchet baseline is refreshed (+1,041 code lines, +10
  test functions, +16 documented items, 0 inline allows).
- **Fixture annotation grammar + codemod (M1.4)**: every `.g` fixture
  carries a machine-readable `//@` header, and the harness rejects
  fixtures that lack one — the "100% of fixtures tagged; fixtures lacking
  expectations rejected" DoD is a tested fact, not a promise.
  - `glyim-test`: new `annotations::directives` grammar — `//@ mode:`,
    `//@ feature:` (comma-separated registry IDs, shape-checked),
    `//@ backends:` (validated against the ledger's `gam`/`llvm`/
    `cranelift`/`bytecode` set), the `expect-*` outcome family
    (`expect-exit`/`expect-stdout`/`expect-stderr`/`expect-error`/
    `expect-warning`/`forbid-warning`), and the harness gates
    (`needs-llvm`, `ignore`, `only-target`, `compile-flags`, `aux-file`,
    `timeout`, `min-version`, `revisions`, `[rev] compile-flags`).
    Unknown keys, malformed IDs, duplicate singular keys, and mixing a
    legacy `// key:` header with its canonical `//@` spelling are hard
    parse errors, so a typo can never become a silent no-op tag.
  - `glyim-test`: the collector rejects fixtures with no `//@` header
    (`MissingAnnotations`) and fixtures whose mode lacks the required
    expectation — run-pass/run-fail must pin an exit code or
    stdout/stderr pattern (the strategies' defaults `0`/`1` are now
    spelled out in every fixture), compile-fail must carry an inline
    `//~` annotation or `//@ expect-error:` (`MissingExpectation`).
  - `glyim-test`: new `glyim-tag-fixtures` codemod bin converts every
    fixture header to the canonical form and derives the ledger tags
    (410 fixtures: 375 in the `glyim-test` corpora plus the 35 in the
    per-crate corpora of `glyim-lower`/`glyim-typeck`/`glyim-pipeline`/
    `glyim-lang-{alloc,core,std}` that drive the same `TestCollector`):
    `feature` from a curated keyword table (92 distinct IDs, all
    cross-checked against the M1.2 registry via `--verify-ids`) plus the
    fixture-mode feature, `backends` from the CI reality (`gam, llvm`
    for the run corpora and `needs-llvm` fixtures; `gam, bytecode` for
    the `--backend=bytecode` fixture; `gam` elsewhere). The rewrite is
    idempotent; prose comments pass through untouched.
  - The tagging surfaced two real seams: `runtime/drop_order.g` (a
    bytecode-backend fixture not driven by any corpus — the async-proof
    gating in `tests/runtime.rs` covers `runtime/m5` only) and the
    `typeck/` + `codegen/` fixture dirs (no corpus root points at them).
    Both are now tagged and recorded for the ledger's work queue (M1.9
    `ledger next`), not silently fixed here.
  - Bench: `glyim-test --bench annotations` measures one full discovery
    pass (parse: 375 rich headers in ~0.4 ms; codemod rewrite: 375
    legacy fixtures in ~1.4 ms) — the M1.4 gate is effectively free.
  - New tests: 29 unit tests across the grammar contract and the codemod
    rewrite rules, plus the workspace-wide `annotations_corpus` DoD gate
    (covering the per-crate corpora as well). The M0 metrics ratchet
    baseline is refreshed (+129 test functions, 0 inline allows).
  - Docs drift fixed while landing: README badge/MSRV said Rust 1.94
    (workspace `rust-version` is 1.95), the crate-count badge said 20+
    (the workspace has 38 members), and the stale committed `Cargo.lock`
    (members at 0.5.0 vs workspace 0.5.1) is regenerated.
- **Inventory extractors, slice 1 (M1.3)**: `ledger inventory` reads the
  source, so the registry can no longer drift from the code (plan §3.5).
  - `ledger-schema`: two `syn`-based extractors — MIR IR variants
    (`mir.<Enum>.<Variant>` for the five backend-parity enums
    `StatementKind`/`Rvalue`/`Operand`/`ProjectionElem`/`TerminatorKind`
    in `crates/glyim-mir`) and runtime FFI symbols
    (`runtime.ffi.<symbol>` for every `#[no_mangle]`/`#[unsafe(no_mangle)]`
    `pub extern "C"` fn in `crates/glyim-runtime`, 99 of them). Feature
    entries gain an optional `inventory = [...]` claim list; a new
    `ledger/inventory-ignore.toml` registers deliberate non-features
    (exact keys only — no globs, so a new variant can never be silently
    swallowed). `inventory_diagnostics` fails `ledger check` on unmapped
    items (`LEDGER-INVENTORY-UNMAPPED`), stale ignores
    (`LEDGER-INVENTORY-STALE-IGNORE`), stale claims
    (`LEDGER-INVENTORY-STALE-CLAIM`), duplicate claims/ignores
    (`LEDGER-INVENTORY-DUP-CLAIM`, `LEDGER-INVENTORY-DUP-IGNORE`),
    claimed-and-ignored conflicts (`LEDGER-INVENTORY-CONFLICT`), and
    unparseable sources (`LEDGER-INVENTORY-PARSE`) — the M1.3 DoD
    "adding a MIR variant without registration fails `ledger check`
    (tested)" is a unit + integration tested fact.
  - `glyim-ledger`: new `ledger inventory [ROOT]` subcommand (items with
    claim/ignore status, exit 1 on gate errors); `ledger check` now runs
    the inventory gate and reports the extracted item count.
  - Registry seed: the 15 implemented FFI families claim their 99
    symbols; the 29 existing MIR variants are ignored-with-reason (the
    per-variant rows are the §3.6 backend-parity matrix, M1.8). The
    extractor immediately surfaced registry drift: `RUNTIME-FFI-FS`
    claimed 16 symbols in its title but implements 14 (title corrected);
    `RUNTIME-FFI-SYNC` (mutex/rwlock) has no backing symbols at all and
    stays unclaimed until they land.
  - Bench: `extract_inventory` harness measures parse+extract of a
    synthetic 150-variant/100-symbol corpus (~1.4 ms, parse-dominated).
  - 31 new tests across the two ledger crates, written test-first.
- **Feature registry seed (M1.2)**: the ledger's claim surface grows from
  6 smoke-test entries to 216 curated features across 32 registry files,
  covering the documented inventory end to end: the README feature list
  and language tour, `docs/specs/v0.1.0.md`, all eight `docs/design/*`
  documents, the 33 stdlib modules of `glyim-lang-core`/`-alloc`/`-std`,
  the 16 runtime FFI symbol families, and the tooling surface (CLI,
  `glyip`, LSP, test harness, and the ledger/metrics tooling itself).
  - `ledger-schema`: the feature taxonomy the master plan delegates to an
    "Appendix C" that was never checked in (docs drift found during the
    M1.2 deep dive) now lands in-repo as `TAXONOMY` — 34 areas over six
    namespaces (`lang.*`, `compiler.*`, `codegen.*`, `tooling.*`,
    `std.*`, `runtime.*`), each with an enforced ID prefix — plus
    `min_target_level` encoding release gate RG-09 (`core` ⇒ L5,
    `std` ⇒ L4) at declaration time. `ledger check` now fails on unknown
    areas (`LEDGER-AREA-UNKNOWN`), prefix mismatches
    (`LEDGER-ID-AREA-MISMATCH`), tier/target violations
    (`LEDGER-TIER-TARGET`), and dead spec references
    (`LEDGER-SPEC-MISSING`): every entry's `spec` must resolve to a real
    repo-relative file, so a moved or renamed doc breaks CI instead of
    silently rotting the registry.
  - `ledger-schema`: new `load_project_registry` production entry point —
    `glyim-ledger check <ROOT>` walks `ROOT/ledger` and anchors
    repo-relative references (`spec` paths, gap witnesses) at the project
    root, matching the plan's path examples; the fixture-facing
    `load_registry` keeps its behavior for tests.
  - `glyim-ledger`: the check gate runs the taxonomy/spec validation and
    reports a `bytecode` backend variant for the
    `glyim-codegen`/`glyim-bytecode-vm` executor (goes away with
    ADR-004's removal, if accepted).
  - Bench: the `ledger-schema` load+validate harness is scaled to the
    registry's real size (300 features / 50 gaps synthetic, was
    200/40), switched to the production `load_project_registry` path and
    fixed to a taxonomy-valid fixture, so CI headroom tracking measures
    the M1.2 shape: ~2.6 ms per load+validate of 350 entries.
  - Seed data fixes found by the new gates: `LANG-ASYNC-AWAIT` now
    targets L5 as RG-09 requires for `core` tier entries.
  - 65 tests total in the two ledger crates (14 new taxonomy/spec tests
    in `ledger-schema`, 2 new check-integration tests in
    `glyim-ledger`), written test-first.
- **Readiness Ledger v1 foundation (M1.1)**: new `tools/ledger-schema`
  and `tools/glyim-ledger` crates implementing the registry schema half
  of the master plan's Workstream A (§3).
  - `ledger-schema`: strict TOML entry types (`[[feature]]`, `[[gap]]`)
    with `deny_unknown_fields` — typos are load errors, never silently
    ignored keys; the maturity-level/gap-class/backend/lane vocabulary
    from the plan (§3.2, §3.3, §3.6, §3.8); `load_registry` walking
    `ledger/features/**` and `ledger/gaps/**` deterministically (sorted
    walks, sorted diagnostics); `Registry::validate` — the `ledger check`
    engine (unique IDs, `affects`/`requires` reference resolution,
    `requires` cycle detection, and the strict-xfail witness contract
    with `//@ known-gap:` headers and ratcheted witness debt); and a
    dependency-free JSON-Schema (draft-07) export for both entry types.
  - `glyim-ledger`: `ledger check [ROOT]` registry gate (exit 1 on any
    error, witness debt reported as warnings) and `ledger schema [OUT]`
    JSON-Schema export. Hand-rolled argument parsing, matching the
    `glyim-metrics` no-clap convention. 49 tests total, written
    test-first, plus a load+validate throughput bench.
  - Seed registry `ledger/`: 6 real feature entries (control flow,
    pattern matching, generics, async, GSX views, unwind cleanup) and 2
    real gap entries migrated from `KNOWN_GAPS.md` (GAP-0001 await-in-loop
    class a, GAP-0002 SEH cleanup unreachable class b), both declared
    `unwitnessed` until the M1.6/M1.7 witness fixtures land.
  - CI: new `ledger-check` job runs the gate on every PR (pure Rust, no
    LLVM dependency).
- **GSX view syntax** (docs/design/GSX-in-Glyim.md): first-class, typed,
  host-agnostic view syntax with a dedicated compiler stage.
  - Frontend: `Gsx*` CST nodes and `parse_gsx_element` (elements, fragments,
    self-closing, directive attrs `on:`/`class:`/`style:`/`bind:`, spread,
    shorthand, hyphenated names, `for … by` keyed iteration, text holes split
    at parse time; zero lexer changes).
  - HIR: `Expr::Gsx` preserves element structure (no call-tree desugar).
  - Typeck: `check_gsx` validates tags/props/events/children against dialect
    schemas and checks hole expressions.
  - New crate `glyim-gsx`: View IR (skeleton + slots + regions), flat-`u32`
    blueprints, read/write-set analysis with prefix interference,
    mutation-specialized patch functions (§7.5), SSR emitter, Host trait +
    headless op-recording host, keyed-list LIS reconcile, `Signal<T>` (G0201)
    diagnostics with fix-its. 88 crate tests including the §3.1 Counter
    example end-to-end.
- Fuzzing infrastructure: `fuzz_parse` (lexer/parser/CST round-trip) and
  `fuzz_compile` (full-pipeline ICE detector) cargo-fuzz targets with
  corpus seeds and a token dictionary.
- CI: strict M5 runtime proof (real LLVM codegen), differential suite
  (LLVM native vs MIR interpreter), lint job (`fmt` + `clippy -D
  warnings`), cargo-deny supply-chain job, runner caching, timeouts,
  concurrency cancellation.
- MIT LICENSE file, declared MSRV (`rust-version = "1.85"`).
- **M0.6 metrics ratchet**: new `tools/glyim-metrics` crate (TDD, 18 unit
  tests + dependency-free scan bench) measures lexical repo health —
  `.rs` files, code lines, public items, doc coverage, test functions,
  and the inline-`allow` count the M0 zero-allow policy pins at zero.
  The binary writes `metrics/baseline.json` (906 files / 226.5k lines /
  4811 tests / 0 inline allows at M0) and CI's new `metrics-ratchet` job
  fails on any regression while allowing improvements (gates never
  weaken, C9).
- **Workspace-wide pedantic lint rails** (M0 §2 C4, §7.4):
  `[workspace.lints.clippy] pedantic` is wired into all 36 member crates
  via `[lints] workspace = true`; every relaxation is a single
  rationale-annotated entry in the root manifest, and the enforced gate
  (`cargo clippy --workspace --all-targets --all-features -- -D warnings
  -W clippy::pedantic` with manifest-derived re-asserts) passes with
  **zero inline `#[allow]`/`#[expect]` attributes** in the tree.
- `rust-toolchain.toml` pins local development to the `stable` toolchain
  (MSRV 1.95.0 stays a separate CI job).

### Fixed
- **`glyim-opt`'s drop-flag statement inspection re-read the pushed
  statements through `new_stmts.last().unwrap()`** — provably non-empty,
  but the re-inspection also saw the optional plain-flag statement pushed
  one line earlier, so the per-element check's input depended on push
  order. The per-element flag statement is now computed from the original
  statement before it is moved (branch is total; emitted order fixed:
  original stmt, plain flag, per-element flag). Behaviour-preserving; the
  `COMPILER-OPT-DROP-ELAB` fixtures pin it.
- **GSX SSR emitter dropped every element's closing `>`** (`</h1` instead of
  `</h1>`) — the crate's own G7 snapshot tests were left red on `main` (the
  ubuntu `Test` job has been failing since); the fix makes all 88 `glyim-gsx`
  tests pass.
- **`glyim-test`'s `json-output` feature did not compile when enabled**: the
  optional `serde` dependency lacked its `derive` feature, so the feature's
  `#[derive(Serialize)]` blocks failed (`cargo test --all-features` could not
  build the crate). Found by the M1.3 gate run.

### Changed
- **The strict-xfail witness contract covers run modes (M1.7)**:
  `//@ known-gap: GAP-NNNN` is now accepted on `run-pass`/`run-fail`
  fixtures (a pinned exit code is a valid gap-open behaviour witness) and a
  failing run witness sharpens into `FailureReason::KnownGapClosed` instead
  of a generic exit/output mismatch. `compile-pass`/`ui` witnesses stay
  rejected at discovery — the discovery error is renamed
  `WitnessRequiresCompileFail` → `WitnessUnsupportedMode` with updated
  guidance. First consumer:
  `tests/run-pass/behavior/partial_move_drop_guard_known_gap.g` witnesses
  GAP-0004 on the exit-code contract.
- **`regalloc2` dependency removed** from the workspace (unused).
- Session documents `HANDOFF.md` and `FIXES.md` moved to `docs/archive/`
  to keep the repo root navigation-focused.
- Dead code deleted across the workspace: unused fields/methods/helpers
  (`DoneDefaults` fields, `record_commit`/`set_provider_cooldown`,
  `SourceMap::path`, `AnalysisDriver::cache_dir`, test-only helpers that
  no test reached, …) — replaced with `#[cfg(test)]` where the consumers
  are the test suites themselves.
- ~160 `missing_docs` gaps closed; `push_str(&format!(..))` idioms
  converted to `write!`/`writeln!`; format args inlined per
  `uninlined_format_args`.
- CI installs LLVM 23 consistently (`./llvm.sh 23` — previously mixed
  v22 script with v23 dev packages).
- Release profile enables `overflow-checks` (compiler-internal arithmetic
  overflow is an ICE, never a silent wrap).

### Fixed
- `parse_type` recursion depth unbounded → deeply nested types crashed the
  compiler with an uncatchable stack overflow (SIGSEGV). Now a parse
  diagnostic (`MAX_EXPR_DEPTH` guard, same policy as expressions).
- MIR interpreter subslice projection: bounds guard checked `to` while
  subtracting `from` → `usize` underflow (panic in debug, bogus slice
  length in release). Both operands are now validated.
- MIR interpreter `byte_payload`: program-controlled `repeat(n)` could
  OOM the host; payload capped at 16 MiB.
- const-eval array/tuple index: `u128 → usize` truncation silently aliased
  indices ≥ 2^64 to in-bounds elements. Oversized indices now error.
- Interpreter internal index accesses (`basic_blocks`, argument locals)
  return `InterpError` with a "please report" message instead of panicking.
- `glyim-cli` ICE report now includes the panic message, suppresses the
  default panic printer during `catch_unwind`, and points to the correct
  issue tracker.
- `glyip test` isolates interpreter panics per fixture instead of aborting
  the whole test session.
- LLVM codegen: recoverable type/ABI failures return diagnostics instead
  of panicking (`fn type lowering failed`, `Codegen error`, variant index).
- README: correct repo URL, binary name, and LLVM-install guidance.
- Legacy session instrumentation and stale doc-comment artifacts
  (unbalanced backticks in doc comments, detached docs, duplicated
  crate-level docs) cleaned up as part of the M0 doc pass.

## [0.1.0] — initial public state

Research-grade end-to-end compiler: lexing, parsing, name resolution,
macros, HIR, type inference + trait solving, NLL borrow checking, MIR
optimizations, LLVM 23 backend, bytecode backend + VM, MIR interpreter,
LSP, `glyip` build tool, and a five-mode test harness.
