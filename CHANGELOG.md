# Changelog

All notable changes to Glyim are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added
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
- **GSX SSR emitter dropped every element's closing `>`** (`</h1` instead of
  `</h1>`) — the crate's own G7 snapshot tests were left red on `main` (the
  ubuntu `Test` job has been failing since); the fix makes all 88 `glyim-gsx`
  tests pass.
- **`glyim-test`'s `json-output` feature did not compile when enabled**: the
  optional `serde` dependency lacked its `derive` feature, so the feature's
  `#[derive(Serialize)]` blocks failed (`cargo test --all-features` could not
  build the crate). Found by the M1.3 gate run.

### Changed
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
