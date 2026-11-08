# Changelog

All notable changes to Glyim are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added
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
