<div align="center">
  <img src="docs/logo.png" alt="Glyim Logo" width="300"/>
  <p>
    <strong>A modular, from‑scratch compiler for a Rust‑like systems programming language, written in Rust.</strong><br/>
    Implements a complete compilation pipeline: lexing, parsing, name resolution, HIR, MIR, type inference &amp; trait solving, borrow checking, optimizations, and multiple code generation backends (LLVM and a custom bytecode VM). The project is organised as a Cargo workspace with 39 members — 34 compiler/stdlib crates plus 5 tooling crates — designed for clarity, testability, and incremental development.
  </p>
  <p>
    <img src="https://img.shields.io/badge/Rust-1.95%20%7C%202024-000000?style=flat-square&logo=rust" alt="Rust"/>
    <img src="https://img.shields.io/badge/License-MIT-blue?style=flat-square" alt="License MIT"/>
    <img src="https://img.shields.io/badge/LLVM-23-262D3A?style=flat-square&logo=llvm" alt="LLVM"/>
    <img src="https://img.shields.io/badge/Crates-39-6F4E37?style=flat-square" alt="Crates"/>
    <img src="https://img.shields.io/badge/Backend-LLVM%20%2B%20Bytecode-6A0DAD?style=flat-square" alt="Backend"/>
    <img src="https://img.shields.io/badge/Language%20Server-LSP-4B32C3?style=flat-square" alt="LSP"/>
    <img src="https://img.shields.io/badge/Testing-Snapshot%20%2B%20UI-00BFFF?style=flat-square" alt="Testing"/>
    <img src="https://img.shields.io/badge/Borrow%20Checker-NLL-FF4500?style=flat-square" alt="Borrow Checker"/>
    <img src="https://img.shields.io/badge/Optimizations-Const%20Prop%20%2B%20DCE-333333?style=flat-square" alt="Optimizations"/>
    <img src="https://img.shields.io/badge/Type%20System-Inference%20%2B%20Traits-007ACC?style=flat-square" alt="Type System"/>
    <img src="https://img.shields.io/badge/HIR%2FMIR-Full%20Support-228B22?style=flat-square" alt="HIR/MIR"/>
    <img src="https://img.shields.io/badge/Build-Passing-brightgreen?style=flat-square&logo=githubactions" alt="Build"/>
  </p>
</div>

---

# Glyim

> [!NOTE]
> Glyim is a research-grade compiler under active development. The pipeline is end-to-end functional for a substantial subset of the language, and the test harness verifies every stage from lexing through MIR interpretation. See [Project Status](#project-status) for what's wired up today versus what's still on the roadmap.

---

## Table of Contents

- [Why Glyim](#why-glyim)
- [Features](#features)
- [Architecture](#architecture)
- [Getting Started](#getting-started)
- [Usage](#usage)
- [Language Tour](#language-tour)
- [The `glyip` Build Tool](#the-glyip-build-tool)
- [Development](#development)
- [Testing](#testing)
- [Project Status](#project-status)
- [Acknowledgements](#acknowledgements)

---

## Why Glyim

Most "toy compiler" projects stop at a parser or a tree-walking interpreter. Glyim is built to be a *real* compiler: it has a full type system with inference and trait solving, an NLL borrow checker, monomorphization, a MIR optimizer, and a production LLVM backend via `inkwell`. Every stage is a self-contained crate with a narrow public API, so each phase can be studied and tested in isolation — and replaced without touching the rest.

The design is heavily inspired by rustc's architecture (arena-allocated IRs, interned types, query-style contexts, trait-based dependency injection), but written from scratch with a strong emphasis on readable code and testability.

---

## Features

### Language

- **Rust-like syntax** with `fn`, `let`, `struct`, `enum`, `match`, `if`/`else`, `while`, `loop`, `for`, closures, `impl` blocks, and traits.
- **Generics** with type parameters, generic bounds (`T: Trait`), and where-clauses.
- **Traits** with default methods, associated types, and static dispatch.
- **Pattern matching** with literals, ranges, or-patterns, slices, structs, tuples, enums, and `if` guards.
- **`async`/`await`** with state-machine desugaring (straight-line and multi-await bodies).
- **Closures** with capture analysis (`ByValue` / `ByRef(Mut)` / `ByRef(Not)`).
- **Macros**: declarative `macro_rules!` with fragment specifiers (`:expr`, `:ty`, `:pat`, `:tt`, …), plus built-in `file!`, `line!`, `column!`, `env!`, `option_env!`, `include!`, `include_str!`, `include_bytes!`, `concat!`, `concat_idents!`, `stringify!`.
- **Procedural macros** through a C-compatible token-stream ABI (`glyim-proc-macro`), with a `dlopen`-based cdylib loader.
- **Ranges**, `Option`/`Result`/`Vec`/`String`/`Box`/`PhantomData`/`UnsafeCell` as compiler-known builtins.
- **GSX view syntax**: first-class, typed, host-agnostic view elements (`<button on:click={…}>…</button>`) with a dedicated compiler stage — View IR, flat-`u32` blueprints, ownership-driven reactivity (mutation-specialized patches, no vdom/signals on the fast path), SSR emitter and a headless test host. See `docs/design/GSX-in-Glyim.md` and `docs/design/gsx-implementation.md`.

### Compiler

- **Non-lexical lifetime (NLL) borrow checking** with Polonius-style region inference, cross-block liveness analysis, and two-phase borrow support.
- **Move analysis** with partial-move tracking and drop-flag elaboration.
- **Type inference** with unification variables (general / integer / float), occurs-check, and bidirectional checking.
- **Trait solving** via a fulfillment context with obligation queues, HRTB support (`for<'a>`), and auto-trait computation (`Send`/`Sync`/`Unpin`).
- **Monomorphization** with polymorphization (unused generic params → canonical placeholder) and codegen-unit partitioning.
- **MIR optimizations**: constant propagation, dead code elimination, CFG simplification, unreachable block elimination, slice desugaring, and drop elaboration.
- **Layout computation** with ABI-aware argument passing (`sret`, `byval`, `Ignore`, `Direct`, `Split`).
- **VTable generation** for trait objects.
- **MIR interpreter** used for `const fn` evaluation and test execution.
- **Incremental compilation** via SHA-256 fingerprinting of sources and build configuration.

### Code Generation

- **LLVM backend** (`glyim-codegen-llvm`) using `inkwell`/LLVM 23 — produces native object files with real ABI lowering, debug info, exception handling (Itanium + SEH), and Link-Time Optimization (`fat` / `thin`).
- **Bytecode backend** (`glyim-codegen`) — a compact stack-machine bytecode used for testing and embedded execution, verified by the `glyim-bytecode-vm` crate.

### Tooling

- **`glyim` CLI** — compile with `--emit=obj|exec|mir|llvm-ir|asm|cdylib|syntax|stats`, choose a backend, set opt-level, target triple, LTO strategy, and codegen units; measure compiles with `--time-passes` and `--self-profile[=FILE]` (Chrome trace), and emit a schema-versioned JSON measurement report with `--emit=stats`.
- **`glyip` build tool** — Cargo-like project manager with `new`, `build`, `test`, and `run`; dependency resolution with SemVer 2.0 matching, path deps, git deps (branch/tag/rev), lockfiles, and a registry client.
- **Language Server** (`glyim-lsp`) — diagnostics, goto definition, hover, completion, folding, formatting, rename, workspace symbols, code actions (add missing match arms, generate impl, remove unused imports), and auto-import.
- **Test harness** (`glyim-test`) — compile-pass / compile-fail / UI / run-pass / run-fail modes with inline annotations (`//~ ERROR`, `//~| …`), snapshot testing (CST, def-map, MIR), mocking utilities for every compiler phase, and property-based type generation.

---

## Architecture

The compiler is split into small, single-responsibility crates. Each phase consumes the previous phase's IR and exposes a minimal trait-based interface so tests can substitute mocks.

| Crate | Description |
|-------|-------------|
| `glyim-core` | Foundation types: index vectors, definition IDs, interner, paths, ABI constants. |
| `glyim-span` | Source locations (file, byte index, span), hygiene contexts, multispan diagnostics. |
| `glyim-diag` | Diagnostic types, error codes, `DiagSink`, `miette` integration. |
| `glyim-vfs` | Virtual file system with in-memory file content tracking. |
| `glyim-syntax` | CST definition (Rowan-based), `SyntaxKind` enum, AST node helpers. |
| `glyim-frontend` | Lexer + parser (merged), produces `SyntaxNode`. |
| `glyim-gsx` | GSX view syntax: parser, View IR (VIR), flat-`u32` blueprints, ownership-driven reactivity analysis, SSR emitter, headless test host. |
| `glyim-def-map` | Module graph, item scopes, name resolution. |
| `glyim-meta` | Macro expansion: `macro_rules!` declarative macros and built-in macros. |
| `glyim-proc-macro` | C-compatible proc-macro ABI, registry, and `dlopen` loader. |
| `glyim-hir` | High-level IR (untyped), lowering from CST. |
| `glyim-type` | Type interning (`TyCtx`), type kinds, substitutions, regions, predicates, auto-traits, object safety, layout hints, printing. |
| `glyim-solve` | `InferenceTable`, unification, trait solver, fulfillment context, HRTB. |
| `glyim-typeck` | Type checker: HIR → THIR with inference and trait resolution. |
| `glyim-const-eval` | Constant expression evaluator over HIR. |
| `glyim-mir` | Mid-level IR (CFG), place types, statement/terminator kinds. |
| `glyim-lower` | THIR → MIR lowering + monomorphization + CGU partitioning + polymorphization. |
| `glyim-borrowck` | Borrow checker (NLL, two-phase borrows, move analysis). |
| `glyim-opt` | MIR optimisation passes. |
| `glyim-mir-interp` | Interpreter for MIR (used in tests and const-eval). |
| `glyim-layout` | Type layout computation (size, alignment, ABI, vtables). |
| `glyim-codegen` | Abstract code generation backend trait + bytecode backend. |
| `glyim-codegen-llvm` | LLVM backend (via `inkwell`) with full ABI handling and LTO. |
| `glyim-bytecode-vm` | Switch-dispatch VM executing the bytecode backend's output. |
| `glyim-runtime` | Runtime FFI: alloc, dealloc, drop glue, panic, fs/net/thread/time/process. |
| `glyim-db` | Compilation database: interners, VFS, type context, trait context. |
| `glyim-pipeline` | End-to-end compilation driver (lex → parse → def-map → HIR → typeck → lower → borrowck → opt → codegen). |
| `glyim-cli` | Command-line interface (`clap`) and linker driver. |
| `glyim-lsp` | Language Server Protocol implementation. |
| `glyim-test` | Testing framework: discovery, execution, snapshots, mocks, property testing. |
| `glyim-lang-core` | Core library source (`.g` files): `Option`, `Result`, `iter`, `slice`, `str`, `cell`, `mem`, `ptr`, `ops`, `cmp`, `marker`, `panic`, `hint`, `convert`, `default`, `future`, `parse`. |
| `glyim-lang-alloc` | Alloc library source: `Box`, `Vec`, `String`, `Rc`, `RawVec`, plus the shared `alloc` module. |
| `glyim-lang-std` | Standard library source: `io`, `fs`, `net`, `thread`, `sync`, `env`, `time`, `process`, `task`, plus integer/string formatting (`int_str`). |
| `glyip` | Package manager / build tool. |
| `tools/glyim-metrics` | Repo-health metrics (code lines, tests, doc coverage, inline-allow count) behind the CI metrics ratchet. |
| `tools/glyim-pilot` | Agent orchestrator with gate bundles (clippy, fmt, contracts, coverage, mutation, ledger). |
| `tools/ledger-schema` | `deny_unknown_fields` schemas shared by the readiness ledger and inventory extractors. |
| `tools/glyim-ledger` | Readiness-ledger CLI: `check`, `gaps`, `inventory`, `schema` — registry consistency gates run in CI. |
| `tools/glyim-bench` | M2.2 measurement harness: `perf stat`/callgrind instruction counts, wall time, child RSS, a counting allocator, the CoV stability gate, and the M2.3 deterministic corpus generators (`glyim.corpus/1` manifest). |

### Compilation Pipeline

```
source.g
   │
   ▼
┌──────────────┐   ┌──────────────┐   ┌──────────────┐
│   Lexer      │──▶│   Parser     │──▶│   CST        │  (glyim-frontend, glyim-syntax)
└──────────────┘   └──────────────┘   └──────────────┘
                                             │
                                             ▼
┌──────────────┐   ┌──────────────┐   ┌──────────────┐
│  Macro       │◀──│   Def-Map    │◀──│   HIR        │  (glyim-meta, glyim-def-map, glyim-hir)
│  Expansion   │   │  (name res.) │   │  (untyped)   │
└──────────────┘   └──────────────┘   └──────────────┘
                                             │
                                             ▼
┌──────────────┐   ┌──────────────┐   ┌──────────────┐
│  Trait       │◀──│   Typeck     │──▶│   THIR       │  (glyim-solve, glyim-typeck)
│  Solving     │   │  (inference) │   │  (typed)     │
└──────────────┘   └──────────────┘   └──────────────┘
                                             │
                                             ▼
┌──────────────┐   ┌──────────────┐   ┌──────────────┐
│  Borrowck    │◀──│  Lowering    │──▶│   MIR        │  (glyim-lower, glyim-borrowck, glyim-mir)
│  (NLL)       │   │ (monomorph.) │   │  (CFG)       │
└──────────────┘   └──────────────┘   └──────────────┘
                                             │
                                             ▼
┌──────────────┐   ┌──────────────┐   ┌──────────────┐
│  LLVM IR     │◀──│  Optimizer   │──▶│  Bytecode    │  (glyim-opt, glyim-codegen-llvm, glyim-codegen)
│  → .o / exe  │   │  (MIR passes)│   │  → .gbc      │
└──────────────┘   └──────────────┘   └──────────────┘
                                             │
                                             ▼
                                       ┌──────────────┐
                                       │  Interpreter │  (glyim-mir-interp)
                                       └──────────────┘
```

---

## Getting Started

### Prerequisites

- **Rust** — latest stable, 2024 edition (Rust 1.95+ recommended; the MSRV is pinned in `Cargo.toml` and enforced by the `msrv` CI job).
- **LLVM 23** — required only for the LLVM backend. Set `LLVM_SYS_231_PREFIX` to your LLVM installation prefix before building.
- **`watchexec`** — optional, for the `justfile` recipes.

> [!IMPORTANT]
> The LLVM backend is a hard dependency of the pipeline — there is no
> feature flag to build without it. Install LLVM 23 and set
> `LLVM_SYS_231_PREFIX` (Homebrew: `$(brew --prefix llvm@23)`,
> Debian/Ubuntu: `/usr/lib/llvm-23`).

### Building

```bash
git clone https://github.com/elcoosp/glyim
cd glyim
cargo build --release
```

The compiler driver is then available at `target/release/glyim-cli` (invoke it as `glyim-cli`; rename or symlink to `glyim` if you prefer).

### Quick Start

```bash
# Create a new project
cargo run -p glyip -- new hello
cd hello

# Build it
cargo run -p glyip -- build

# Run it
cargo run -p glyip -- run
```

---

## Usage

### The `glyim` Compiler Driver

```bash
# Compile a source file to an object file using the LLVM backend (default)
glyim input.g -o output.o

# Emit LLVM IR instead of an object file
glyim input.g --emit llvm-ir

# Emit MIR (useful for debugging lowering)
glyim input.g --emit mir

# Emit assembly
glyim input.g --emit asm

# Use the bytecode backend
glyim input.g --backend bytecode --emit obj

# Compile to a runnable executable
glyim input.g --emit exec -o hello

# Compile to a cdylib (used for proc-macro crates)
glyim proc_macro_dep.g --emit cdylib

# Optimise at level 2 with thin LTO (fat LTO requires multi-CGU support,
# which is not yet implemented; --lto fat errors cleanly)
glyim input.g -O2 --lto thin

# Cross-compile to AArch64 Linux
glyim input.g --target aarch64-unknown-linux-gnu

# Emit diagnostics as JSON (for editors / LSP)
glyim input.g --error-format json
```

### Command-Line Flags

| Flag | Description |
|------|-------------|
| `--emit <KIND>` | `obj` (default), `exec`, `mir`, `llvm-ir`, `asm`, `cdylib`, `syntax`, `stats`, `run` |
| `--backend <NAME>` | `llvm` (default), `bytecode`, or `interp` (run mode only: executes through the in-process MIR interpreter) |
| `-O, --opt-level <N>` | Optimisation level 0–3 |
| `--target <TRIPLE>` | Target triple (default: `x86_64-unknown-linux-gnu`) |
| `--linker <PATH>` | Override the system linker |
| `--link-flags <FLAGS>` | Extra flags passed to the linker |
| `--lto <KIND>` | `off` (default), `fat`, `thin` |
| `--codegen-units <N>` | Number of CGUs (default: available parallelism, capped at 16) |
| `--proc-macro-deps <LIST>` | Comma-separated proc-macro dependency source files |
| `--error-format <FMT>` | `human` (default) or `json` |
| `--with-stdlib` | Prepend the assembled standard library (+ prelude) so `println`, `Vec`, `Option`, … resolve without `use` statements |
| `-A/--allow, -W/--warn, -D/--deny <LINT>` | Lint level control (repeatable; `-W help` lists the lints) |
| `--time-passes` | Print the per-phase wall-clock table to stderr after a successful compile |
| `--self-profile[=FILE]` | Write the per-phase profile as a Chrome Trace JSON event list (default `./trace.json`; value is `=`-bound) |

> [!IMPORTANT]
> `--lto=thin` requires the LLVM backend and the `llvm-lto2` tool from the same LLVM distribution `glyim-codegen-llvm` was built against. Requesting `thin` with `--backend bytecode` is an explicit error rather than a silent no-op.

> [!TIP]
> `--emit=run` executes the program in-process through the MIR interpreter (M2.4) — no LLVM toolchain, no system linker, no `glyim-runtime` staticlib. The program's exit code (masked to `0xff`; panics exit 101) becomes the driver's exit code, so it is scriptable exactly like a linked `--emit=exec` binary:
>
> ```bash
> glyim input.g --emit=run --with-stdlib   # runs without a linker
> glyim input.g --emit=run --backend=interp --time-passes
> ```

### The `glyip` Build Tool

`glyip` is Cargo's counterpart for Glyim projects. It reads a `Glyip.toml` manifest, resolves dependencies (path, registry, and git), maintains a `Glyip.lock` lockfile, and drives the compiler.

```bash
glyip new my_project              # Scaffold a new binary project
glyip new my_lib --lib            # Scaffold a new library project

glyip build                       # Build the project (incremental)
glyip build --release --opt-level 2  # Release build with optimisations
glyip build --backend llvm        # Force the LLVM backend

glyip test                        # Run all tests
glyip test --filter parser        # Run only tests matching "parser"
glyip test --compiled             # Compile each test to a native exe and run it

glyip run -- --flag value         # Build and run with program arguments
```

### Manifest (`Glyip.toml`)

```toml
[package]
name = "my_project"
version = "0.1.0"
edition = "2024"

[dependencies]
serde = "1.0"
local_util = { path = "../local_util" }
my_crate = { git = "https://github.com/example/my_crate", tag = "v0.3.0" }

[dev-dependencies]
test_helpers = { path = "../test_helpers" }
```

> [!TIP]
> The resolver performs real SemVer 2.0 matching (caret, tilde, wildcard, and comparison operators), detects dependency cycles, and reports version conflicts with the full set of requesters that disagree.

#### Dependency resolution semantics

`glyip`'s resolver is deliberately simpler than Cargo's. Knowing the
differences up front saves debugging a resolution failure that is not a
bug:

**Greedy breadth-first, no backtracking.** The resolver walks the
dependency graph breadth-first from the root manifest. When it encounters
`foo = "1.0"`, it picks the highest version of `foo` that satisfies that
requirement *at that moment*, records it, and moves on. If a later
transitive dependency needs an incompatible version of `foo`, the resolver
does **not** go back and try a lower version of the first requirement —
it reports a conflict. Cargo's backtracking resolver would try
alternatives; `glyip`'s does not.

**One version per crate name.** Although `Glyip.lock` keys entries by
`name-version` (so the data structure *could* hold `foo-1.0.0` and
`foo-2.0.0` simultaneously, as Cargo's lockfile does), the conflict check
that runs at the end of resolution looks up the first entry matching a
crate name and verifies every requirement seen in the graph against it.
In practice this enforces a single resolved version per name: two
incompatible requirements for the same crate are a hard
`DependencyConflict` error, not a multimapping.

That error names the crate, every distinct requirement gathered, and
every `(requester, requirement)` edge — so a diamond dependency where two
crates disagree is diagnosable without re-running with extra logging.

**Deterministic version selection.** "Highest matching version" is
computed by parsing every candidate and sorting by SemVer precedence, not
by trusting the registry index's JSON listing order. An unparseable or
unsatisfiable requirement surfaces as `DependencyNotFound` rather than
silently falling back to "latest" — a typo in a version string is an
error, not a surprise upgrade.

**Cycle detection.** The visited set is keyed by
`name@path(path)` for path dependencies and `name-version(git)` for
registry and git dependencies. A cycle (crate A path-depends on B, B
path-depends on A) is caught by the visited check before infinite
recursion. Two path dependencies with the same declared `[package] name`
but different directories are correctly treated as distinct crates —
they resolve independently rather than colliding on the name alone.

**What this means in practice.** For the common shapes — a flat
dependency list, or a diamond where everyone agrees on a major version —
`glyip` resolves the same graph Cargo would. The divergence is when two
dependents require mutually-incompatible versions: Cargo would compile
both copies; `glyip` stops with an error naming both requesters. That is
a deliberate scope limit, not an oversight.

---

## Language Tour

```glyim
// Structs, enums, generics, traits.
struct Point<T> {
    x: T,
    y: T,
}

enum Shape {
    Circle(f64),
    Rectangle(f64, f64),
    Point,
}

trait Area {
    fn area(&self) -> f64;
}

impl Area for Shape {
    fn area(&self) -> f64 {
        match self {
            Shape::Circle(r) => 3.14159 * r * r,
            Shape::Rectangle(w, h) => w * h,
            Shape::Point => 0.0,
        }
    }
}

// Pattern matching, ranges, guards.
fn classify(n: i32) -> &str {
    match n {
        0 => "zero",
        1 | 2 => "small",
        x if x < 0 => "negative",
        _ => "large",
    }
}

// Closures and iterators.
fn sum_of_squares(values: &[i32]) -> i32 {
    values.iter().map(|x| *x * *x).sum()
}

// Async / await.
async fn fetch_twice() -> i32 {
    let a = fetch_one().await;
    let b = fetch_one().await;
    a + b
}
```

> [!NOTE]
> The full grammar and standard library are still evolving. The `glyim-lang-core`, `glyim-lang-alloc`, and `glyim-lang-std` crates contain the current standard library source (written in Glyim) and serve as the compiler's own bootstrapping corpus.

---

## Development

### Workspace Layout

All crates live under `crates/`. The workspace root `Cargo.toml` defines shared dependencies and members. The standard library lives under `glyim-lang-core/lib/`, `glyim-lang-alloc/lib/`, and `glyim-lang-std/lib/` as `.g` source files.

### Adding a New Crate

1. Create the directory under `crates/<name>`.
2. Add a `Cargo.toml` with `[package]` and `[dependencies]`.
3. Register the crate in the workspace `members` list.
4. If its public API is used by other crates, add its path to `[workspace.dependencies]`.

### Design Conventions

- **Context traits** — every phase defines a narrow trait (`LowerCtx`, `BorrowckCtx`, `TypeLookup`, `TraitSolver`) that the pipeline implements. This decouples core logic from the database and lets unit tests substitute mocks.
- **Testing mocks** — `glyim-test` provides `MockLowerCtx`, `MockBorrowckCtx`, `MockSolver`, `MockCodegen`, and `TestDbBuilder`.
- **Arena-allocated IRs** — `IndexVec<I, T>` gives every IR node a typed, cheaply-copyable ID. Interning (`TyCtx`) makes type handles stable across phases.
- **Diagnostics** — every phase returns `Vec<GlyimDiagnostic>`, which the driver renders with `miette` (human) or serialises to JSON.

### Running a Subset of Tests

```bash
# Run the whole suite
cargo test

# Run only the glyim-test harness
cargo test -p glyim-test

# Filter tests by path substring
cargo test -p glyim-test -- --filter parser

# Verbose output
GLYIM_TEST_SHOW_OUTPUT=1 cargo test -p glyim-test
```

### Library Examples

The `examples/` directories show how to consume the compiler crates as
libraries rather than through the CLI:

```bash
# Drive the pipeline in-process: write a .g program, lower it to MIR.
cargo run -p glyim-pipeline --example hello_pipeline

# Execute in-process via the interpreter run contract (M2.4 seam).
cargo run -p glyim-pipeline --example run_interp

# Round-trip the readiness ledger's evidence JSONL (M1.5 seam).
cargo run -p glyim-test --example evidence_report

# Measure a workload in-process: counting allocator + CoV gate (M2.2).
cargo run -p glyim-bench --example measure_alloc

# Render the gap registry's witness-debt report (M1.7 seam).
cargo run -p glyim-ledger --example gap_report

# Build + verify a corpus manifest over the 7 generator families (M2.3).
cargo run -p glyim-bench --example corpus_walk
```

---

## Testing

Glyim's test harness (`glyim-test`) is a first-class part of the project. It discovers `.g` files under `tests/` and runs them in one of five modes, selected by an inline header or the containing directory name:

| Mode | Meaning |
|------|---------|
| `compile-pass` | The source must compile without errors. |
| `compile-fail` | The source must produce specific errors, checked against inline annotations. |
| `ui` | The full compiler output (CST, def-map, typeck, diagnostics) must match an `.expected` snapshot. |
| `run-pass` | The compiled program must exit successfully and match its expected stdout/stderr. |
| `run-fail` | The compiled program must fail with a specific exit code and output. |

### Inline Annotations

```glyim
fn main() {
    let x: i32 = "hello";
    //~^ ERROR mismatched types
    //~| expected i32, found str
}
```

- `//~ ERROR <pattern>` — the annotated line must produce an error whose message contains `<pattern>`.
- `//~| ERROR <pattern>` — a continuation of the previous annotation (used for multi-line diagnostic blocks).
- `//~^`, `//~^^`, … — point at earlier lines.
- `//~~` — fuzzy matching (allow the diagnostic to land within ±1 line).
- `//~? ERROR …` — the diagnostic is optional; the test still passes if it is absent.

### Fixture Annotations

Every `.g` fixture carries a machine-readable `//@` header (plan M1.4). The harness rejects fixtures with no `//@` annotations, and run fixtures must pin an explicit outcome:

```glyim
//@ mode: run-pass
//@ expect-exit: 42
//@ feature: LANG-SYN-OPERATORS, TOOL-TEST-RUN-PASS
//@ backends: gam, llvm

fn main() -> i32 { 42 }
```

- `//@ mode:` — explicit test mode (`run-pass`, `compile-fail`, …).
- `//@ expect-exit:` / `//@ expect-stdout:` / `//@ expect-stderr:` — the outcome a run fixture must produce.
- `//@ expect-error:` — a diagnostic substring a `compile-fail` fixture must produce (in addition to inline `//~` annotations).
- `//@ feature:` — readiness-ledger registry IDs the fixture exercises (joined against `ledger/features/**` by `ledger compute`).
- `//@ backends:` — which backends the fixture is verified on (`gam`, `llvm`, `cranelift`, `bytecode`).
- `//@ needs-llvm`, `//@ ignore`, `//@ only-target:`, `//@ compile-flags:`, `//@ aux-file:`, `//@ timeout:`, `//@ min-version:` — harness gates.

The legacy `// key: value` spelling is still parsed, but a fixture must not mix the two forms, and re-tagging is mechanical: `cargo run -p glyim-test --bin glyim-tag-fixtures -- --write`.

### Snapshot Testing

Snapshot tests for the CST, def-map, and MIR are written with `insta`:

```rust
snapshot_cst("my_syntax_case", source);
snapshot_mir("my_mir_case", &ctx, &body);
```

Update snapshots with:

```bash
GLYIM_BLESS=1 cargo test -p glyim-test
# or, for interactive review
cargo insta review
```

### Property-Based Testing

`glyim-test`'s `property` module generates random types (with and without inference variables) and checks invariants such as "unification of a type with itself succeeds" and "unification of two distinct types fails".

---

## Project Status

### Working end-to-end

- Lexing, parsing, CST, error recovery.
- Module graph, name resolution, `use` imports, visibility checks.
- Declarative and built-in macro expansion.
- HIR lowering, including `async fn` / `.await` desugaring.
- Type inference, unification, auto-trait computation, trait solving with HRTB.
- THIR → MIR lowering, monomorphization, polymorphization.
- NLL borrow checking with two-phase borrows and move analysis.
- MIR optimisation passes (const prop, DCE, CFG simplify, unreachable elimination, slice desugar, drop elaboration).
- LLVM codegen for scalar and aggregate types, with real ABI lowering.
- Bytecode codegen and the bytecode VM.
- MIR interpreter used for `const fn` evaluation and `run-*` tests.
- LSP with diagnostics, hover, completion, goto-definition, references, rename, folding, formatting, code actions, and workspace symbols.
- `glyip` build tool: project scaffolding, incremental builds, dependency resolution (path / registry / git), lockfiles, and compiled test execution.

### Tracked gaps

The compiler's development is tracked against an internal de-stubbing plan.
Status is a property of the **readiness ledger**, not of prose: the open gaps
live in `ledger/gaps/known/*.toml` (GAP-0001…GAP-0005), are validated by CI's
`ledger check` job, and carry strict-xfail witness fixtures under
`crates/glyim-test/tests/` where one is reproducible. Render the coverage
report with `cargo run -p glyim-ledger -- gaps`. Notable open items include:

- `.await` inside a loop body is a deliberate, diagnosed hard error
  (`[T0060]`, GAP-0001, witnessed) until the async state-machine v2 lands.
- `panic!` does not unwind: `glyim_panic` writes the message to stderr and
  exits 101 without running destructors (GAP-0003, intentional and
  documented).
- A partial move out of a `Drop`-implementing type suppresses the parent
  destructor entirely — sound (over-retains, never double-frees) but it
  leaks the still-initialized siblings (GAP-0004, witnessed by a run-pass
  fixture that will fail loudly when fine-grained guards land).
- Windows SEH cleanup edges are implemented but structurally unreachable
  until a lowering site populates `cleanup:` (GAP-0002, Windows-only, so
  explicit witness debt).
- Some builtin methods on `Vec`/`String`/`Result`/`Option` lower to
  synthetic `FnDefId`s whose bodies the LLVM backend must intrinsic-lower;
  unimplemented ones produce an explicit compiler error rather than wrong
  code.
- `--lto=thin` requires `llvm-lto2` from the matching LLVM distribution.
- Procedural macros are supported via the C-ABI bridge, but the two-stage
  host compile is still being wired into the default `glyim-cli` flow.

See `KNOWN_GAPS.md` for the full per-entry audit (verification recipes,
closed-entry history) and `docs/archive/session-log-s16-s18.md` for the raw
session log.

> [!WARNING]
> Glyim is not yet self-hosting. The standard library is written in Glyim and consumed by the compiler as a test corpus, but the compiler itself is Rust.

---

## Repository Map & Gates

- `docs/roadmaps/` — the milestone plans; the current one is
  `glyim-master-plan.md`. The workstream state: M0 (rails + metrics) is
  landed, the M1 readiness-ledger slices (M1.1 registry → M1.6
  strict-xfail witnesses) are landed, the M1.7 gap-registry migration is
  landed for the `KNOWN_GAPS.md` §§1–8 sources (`ledger/gaps/known/*.toml`
  plus the `glyim-ledger gaps` witness-debt report), and the M2.1 phase
  instrumentation (`tracing` spans, `--time-passes`, `--self-profile`,
  `--emit=stats`) is landed together with the M2.2 measurement harness
  (`tools/glyim-bench`: instruction counts, wall, child RSS, counting
  allocator, CoV stability gate), and the M2.3 slice 1 corpus substrate is
  landed (`tools/glyim-bench` `corpus` module: the seven deterministic
  generator families, the `glyim.corpus/1` manifest, and the
  identity-verify contract behind "corpora committed with hashes"). The
  next plan items are the rest of M1.7 (the `HANDOFF.md` §2 and playbook
  Appendix A sources) and the rest of M2.3 (the committed corpora
  directory, the real-program set, and the evidence wiring that fills the
  harness's reserved `instructions`/`rss_kb` columns).
- `docs/archive/` — historical session documents (`HANDOFF.md`,
  `FIXES.md`) kept for provenance; the root stays navigation-focused.
- `metrics/baseline.json` — the M0.6 repo-health baseline (file/line/test
  counts, doc coverage, inline-allow count pinned at 0). CI's
  `metrics-ratchet` job fails on regression and allows improvement; bump
  it deliberately with
  `cargo run -p glyim-metrics -- . --write metrics/baseline.json`.
- `.github/workflows/ci.yml` — the enforced gates: fmt + pedantic clippy
  (`-D warnings`, zero inline allows), the test matrix, supply-chain,
  MSRV, and the metrics ratchet.

---

## Acknowledgements

Glyim stands on the shoulders of excellent open-source projects:

- [Rowan](https://github.com/rust-analyzer/rowan) — lossless syntax trees.
- [Inkwell](https://github.com/TheDan64/inkwell) — safe LLVM bindings.
- [Miette](https://github.com/zkat/miette) — beautiful diagnostics.
- [Insta](https://insta.rs/) — snapshot testing.
- [Lasso](https://github.com/Kixiron/lasso) — string interning.
- The Rust compiler team, whose architecture and documentation were a constant source of design inspiration.

---

<p align="center">
  <em>Glyim is a work in progress. Contributions, bug reports, and design discussions are welcome.</em>
</p>
