<div align="center">
  <img src="docs/logo.png" alt="Glyim Logo" width="200"/>
  <p>
    <strong>A modular, from‑scratch compiler for a Rust‑like systems programming language, written in Rust.</strong><br/>
    Implements a complete compilation pipeline: lexing, parsing, name resolution, HIR, MIR, type inference &amp; trait solving, borrow checking, optimizations, and multiple code generation backends (LLVM and a custom bytecode VM). The project is organised as a Cargo workspace with more than 20 crates, designed for clarity, testability, and incremental development.
  </p>
  <p>
    <img src="https://img.shields.io/badge/Rust-1.94%20%7C%202024-000000?style=flat-square&logo=rust" alt="Rust"/>
    <img src="https://img.shields.io/badge/License-MIT-blue?style=flat-square" alt="License MIT"/>
    <img src="https://img.shields.io/badge/LLVM-22-262D3A?style=flat-square&logo=llvm" alt="LLVM"/>
    <img src="https://img.shields.io/badge/Crates-20%2B-6F4E37?style=flat-square" alt="Crates"/>
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

## Features

- **Lexer & Parser** – Recursive‑descent parser with error recovery, producing a concrete syntax tree (CST).  
- **Name Resolution** – Module graph, item scopes, and path resolution (`self::`, `super::`, `crate::`).  
- **Macro System** – Declarative `macro_rules!` and built‑in macros (`file!`, `line!`, `column!`).  
- **HIR (High‑Level IR)** – Untyped, name‑resolved AST, used as input for type checking.  
- **Type System** – Full type interning, substitutions, regions, predicates, auto‑traits (Send/Sync/Unpin), and object safety checks. Supports ADTs, generics, closures, opaque types, projections.  
- **Type Inference & Trait Solving** – Bidirectional type checking with inference variables, unification, and a simple trait solver (fulfillment‑based).  
- **THIR (Typed HIR)** – Fully typed intermediate representation, still generic.  
- **MIR (Mid‑Level IR)** – Control‑flow graph (CFG) form with statements, terminators, places, and rich rvalue expressions.  
- **Borrow Checking** – Non‑lexical lifetime (NLL) borrow checking with liveness analysis, two‑phase borrow support, and move analysis.  
- **Optimisations** – Constant propagation, dead code elimination, CFG simplification, and unreachable block elimination.  
- **Code Generation**  
  - **Bytecode Backend** – Simple stack‑based bytecode for testing and embedded use.  
  - **LLVM Backend** – Generates native object files using the `inkwell` crate (LLVM 22) with ABI‑aware argument/return lowering (sret, byval, etc.).  
- **Language Server** – LSP implementation supporting `didOpen`, `didChange`, diagnostics, goto definition, hover, completion, folding, formatting, rename, and workspace symbols.  
- **Command‑Line Interface** – `glyim` driver with subcommands for compilation, backend selection, and optimisation levels.  
- **Comprehensive Testing Infrastructure** – Built‑in test runner that supports:  
  - Compile‑pass / compile‑fail / UI / run‑pass / run‑fail modes  
  - Inline annotations (`//~ ERROR`, `//~ WARNING`, fuzzy matching, optional diagnostics)  
  - Snapshot testing for CST, def‑map, and MIR  
  - Mocking utilities for all major compiler phases  
  - Property‑based type generation  
- **Standard & Core Libraries** – Source files for `core`, `alloc`, and `std` written in Glyim syntax, used for testing and bootstrapping.  

---

## Architecture

The compiler is split into many small crates, each with a single responsibility:

| Crate | Description |
|-------|-------------|
| `glyim-core` | Foundation types: index vectors, definition IDs, interner, paths, ABI constants. |
| `glyim-span` | Source locations (file, byte index, span), hygiene contexts, multispan diagnostics. |
| `glyim-diag` | Diagnostic types, error codes, `DiagSink`, miette integration. |
| `glyim-vfs` | Virtual file system with in‑memory file content tracking. |
| `glyim-syntax` | CST definition (Rowan based), `SyntaxKind` enum, AST node helpers. |
| `glyim-frontend` | Lexer + parser (merged), produces `SyntaxNode`. |
| `glyim-def-map` | Module graph, item scopes, name resolution. |
| `glyim-meta` | Macro expansion: `macro_rules!` declarative macros and built‑in macros. |
| `glyim-hir` | High‑level IR (untyped), lowering from CST. |
| `glyim-type` | Type interning (TyCtx), type kinds, substitutions, regions, predicates, auto‑traits, object safety, printing. |
| `glyim-solve` | Type inference table (`InferenceTable`), unification, trait solver, fulfillment context, HRTB support. |
| `glyim-typeck` | Type checker: HIR → THIR with inference and trait resolution. |
| `glyim-mir` | Mid‑level IR (CFG), place types, statement/terminator kinds. |
| `glyim-lower` | THIR → MIR lowering + monomorphization + CGU partitioning + polymorphization. |
| `glyim-borrowck` | Borrow checker (NLL, two‑phase borrows, move analysis). |
| `glyim-opt` | MIR optimisation passes. |
| `glyim-mir-interp` | Interpreter for MIR (used in tests). |
| `glyim-layout` | Type layout computation (size, alignment, ABI, vtables). |
| `glyim-codegen` | Abstract code generation backend trait. |
| `glyim-codegen-llvm` | LLVM backend (via `inkwell`) with full ABI handling. |
| `glyim-runtime` | Runtime stubs (alloc, dealloc, drop glue, panic). |
| `glyim-db` | Compilation database (holds interners, VFS, type context, trait context). |
| `glyim-pipeline` | End‑to‑end compilation driver (lex → parse → def‑map → HIR → typeck → lower → borrowck → opt → codegen). |
| `glyim-cli` | Command‑line interface (clap). |
| `glyim-lsp` | Language Server Protocol implementation. |
| `glyim-test` | Testing framework: test discovery, execution, snapshots, mocks, property testing. |
| `glyim-lang-core` | Core library source (`.g` files) for `core` (Option, Result, iter, slice, str, cell, mem, ptr, ops, cmp, marker, panic, hint, convert, default). |
| `glyim-lang-alloc` | Alloc library source (Box, Vec, String, Rc, RawVec). |
| `glyim-lang-std` | Standard library source (io, fs, net, thread, sync, env, time, process). |
| `glyip` | Package manager / build tool (in development). |
| `glyim-pilot` | Agent‑driven development tool (experimental). |

---

## Getting Started

### Prerequisites

- **Rust** (latest stable, 2024 edition)
- **LLVM 22** (optional – for the LLVM backend)
- **`watchexec`** (optional – for using the `justfile` recipes)

### Building

```bash
git clone <repository-url>
cd glyim
cargo build --release
```

The compiler driver will be available at `target/release/glyim`.

### Running Tests

The test suite uses a custom harness that discovers `.g` files in the `tests/` directory.

```bash
# Run all tests
cargo test

# Run only the test harness (glyim-test)
cargo test -p glyim-test

# Run with verbose output
GLYIM_TEST_SHOW_OUTPUT=1 cargo test -p glyim-test

# Bless (update) snapshot and UI test expectations
GLYIM_BLESS=1 cargo test -p glyim-test
```

---

## Usage

```bash
# Compile a source file using the LLVM backend (default)
glyim input.g -o output.o

# Use the bytecode backend (produces a .bc file)
glyim input.g --backend bytecode

# Optimise (level 1)
glyim input.g -O1

# Specify a target triple
glyim input.g --target aarch64-unknown-linux-gnu

# Get help
glyim --help
```

---

## Development

### Workspace Structure

All crates live under `crates/`. The workspace root `Cargo.toml` defines dependencies and members.

### Adding a New Crate

1. Create a new directory under `crates/`.
2. Add a `Cargo.toml` with appropriate `[package]` and `[dependencies]`.
3. List the crate in the workspace `members` table.
4. If the crate provides a public API used elsewhere, add its path to `workspace.dependencies`.

### Code Organisation

- **Traits for context** – Many phases define a context trait (e.g., `LowerCtx`, `BorrowckCtx`) that the pipeline implements. This keeps core logic decoupled from the actual database.
- **Testing mocks** – The `glyim-test` crate provides mock implementations of these contexts (`MockLowerCtx`, `MockBorrowckCtx`, etc.) for unit testing.
- **Snapshots** – Use `insta` for snapshot testing; run `cargo insta review` to approve changes.

### Running a Subset of Tests

The test harness accepts a filter:

```bash
cargo test -p glyim-test -- --filter parser
```

This runs only tests whose file path contains `parser`.

---

## License

This project is licensed under the **MIT License** – see the [LICENSE](LICENSE) file for details.

---

## Acknowledgements

- [Rowan](https://github.com/rust-analyzer/rowan) – for lossless syntax trees.
- [Inkwell](https://github.com/TheDan64/inkwell) – for LLVM bindings.
- [Miette](https://github.com/zkat/miette) – for fancy diagnostics.
- [Insta](https://insta.rs/) – for snapshot testing.
- The Rust compiler team for design inspiration.

---

<p align="center">
  <em>Glyim is a work in progress. Contributions are welcome!</em>
</p>
