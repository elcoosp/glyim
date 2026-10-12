```markdown
# Glyim Technology Stack Specification

**Document Version:** 1.3.0  
**Status:** Updated  
**Date:** 2025-04-10  
**Owner:** Language Infrastructure Team

---

## 1. Core Infrastructure (Layer 0)

This layer provides foundational utilities, data structures, and the central nervous system (Query Engine) for the compiler. We prioritize crates that are actively maintained, support modern Rust editions, and offer zero-allocation optimizations.

| Crate | Version | Purpose | Rationale |
| :--- | :--- | :--- | :--- |
| **`salsa`** | `0.26.0` | Query Engine Framework | The industry standard for incremental recomputation in compilers. Latest version improves incremental re-use and cycle detection. |
| **`lasso`** | `0.7.3` | String Interning | Fast, copy-on-write string interning with `Lasso<SmartString>`. Replaces `string-interner` to significantly reduce memory footprint and allocations in `glyim-interner`. |
| **`miette`** | `7.6.0` | Error Reporting | The modern successor to `anyhow`/`eyre`. Provides beautiful, structured diagnostic types with `Diagnostic` and `Severity`. Essential for `glyim-diag` to provide IDE-quality error messages. |
| **`anyhow`** | `1.0.0` | Error Propagation | Retained for legacy compatibility in early bootstrapping, though `miette` is now primary. |
| **`tracing`** | `0.1.40` | Instrumentation & Logging | Structured, `tokio`-aware instrumentation. Used by `salsa` to visualize query execution and profile compilation bottlenecks. |
| **`dashmap`** | `7.0.0-rc2` | High-Performance Maps | Adopted Release Candidate `7.0.0-rc2`. Offers significant performance improvements and updated hash algorithms compared to `6.1.0`. |

---

## 2. Frontend & Syntax (Layer 1)

Crates responsible for lexing, parsing, and maintaining a lossless syntax tree necessary for IDE features and macro hygiene.

| Crate | Version | Purpose | Rationale |
| :--- | :--- | :--- | :--- |
| **`rowan`** | `0.16.1` | Lossless Syntax Tree | Retains whitespace, comments, and trivia perfectly. Critical for Tier 1 (Syntax) compilation and accurate error reporting. |
| **`chumsky`** | `0.9.3` | Parser Combinators | Used to build `glyim-parse`. **Efficiency:** Using `chumsky` combinators to build a custom Pratt parser is more efficient than generated parsers (like `lalrpop`) because it eliminates intermediate grammar processing and reduces runtime indirection. It also offers superior error recovery for indentation-sensitive grammars. |
| **`smol_str`** | `0.3.6` | Small String Optimization | Optimizes string storage for identifiers to reduce memory footprint in `rowan` trees. |

---

## 3. Analysis & Metaprogramming (Layer 2)

The "Brain" of the compiler, handling type inference, trait resolution, and the execution of compile-time code.

| Crate | Version | Purpose | Rationale |
| :--- | :--- | :--- | :--- |
| **`rayon`** | `1.12.0` | Parallelism Framework | Used to parallelize heavy lifting phases (type checking, monomorphization) in `glyim-db` and `glyim-egraph`. |
| **`regalloc2`** | `0.15.1` | Register Allocator | High-performance register allocator for `glyim-mir-interp` (MIR Interpreter) to execute `comptime` functions efficiently. |

---

## 4. Optimization & Graph Theory (Layer 3)

Crates responsible for optimizing code via Equality Saturation and managing graph structures for the module system.

| Crate | Version | Purpose | Rationale |
| :--- | :--- | :--- | :--- |
| **`egg`** | `0.11.0` | E-Graphs Library | The definitive library for equality saturation. Used by `glyim-egraph` to perform aggressive algebraic optimization on MIR. |
| **`petgraph`** | `0.6.5` | Generic Graph Algorithms | Provides algorithms (isomorphism, dot export, dominators) used by `glyim-def-map` for module resolution and `glyim-mir` for CFG analysis. |
| **`fixedbitset`** | `0.5.7` | Efficient Bitsets | High-performance bitsets used by `glyim-borrowck` and dataflow analysis for efficient liveness lookups. |

---

## 5. Tooling & Interfaces (Layer 4)

Supporting infrastructure for CLI, LSP, and build orchestration.

| Crate | Version | Purpose | Rationale |
| :--- | :--- | :--- | :--- |
| **`async-lsp`** | `0.25.0` | LSP Server Framework | Replaces `tower_lsp`. Provides async/await support with `tokio` and integrates seamlessly with our custom server architecture. Better suited for high-performance, stateful LSP handling than the standard `tower_lsp` middleware. |
| **`tower`** | `0.5.3` | Service Abstraction | Modular framework for `glyim-lsp` and `glyim-server` (CAS Daemon) to handle concurrency, timeouts, and middleware. |
| **`clap`** | `4.5.31` | CLI Parser | Modern, derive-based argument parsing for `glyim-cli`. |
| **`insta`** | `1.47.2` | Snapshot Testing | Used for `glyim-test` to ensure compiler errors/outputs match golden files. |

---

## 6. Backend & Runtime (Layer 5)

Crates for code generation and runtime support.

| Crate | Version | Purpose | Rationale |
| :--- | :--- | :--- | :--- |
| **`inkwell`** | `0.10.0` (git master, LLVM 23) | LLVM Bindings | Rust wrappers around LLVM C++ API. Used by `glyim-codegen-llvm` for IR generation and optimization passes. |
| **`llvm-sys`** | `231.0.1` | Raw LLVM Bindings | The underlying FFI layer. We explicitly vendor this or pin a compatible version to ensure ABI stability for `inkwell`. |
| **`tokio`** | `1.52.3` | Async Runtime | Async runtime for `glyim-orchestrator` (distributed builds) and optional compiler server. |
| **`serde`** | `1.0.228` | Serialization | Used for configuration (`glyim-config`), artifact metadata, and network protocols in `glyim-server`. |
| **`tracing-subscriber`** | `0.3.23` | Log Formatting | Human-readable log output layer for `tracing`, integrating with `miette` for colored terminal output. |

---

## 7. Dependencies by Workspace Crate

A matrix view of how external dependencies map to internal crates (non-exhaustive).

| Internal Crate | Key External Dependencies |
| :--- | :--- |
| `glyim-db` | `salsa`, `parking_lot`, `tracing` |
| `glyim-lex` / `glyim-parse` | `chumsky`, `rowan`, `smol_str` |
| `glyim-mir-interp` | `regalloc2` |
| `glyim-egraph` | `egg`, `rayon` |
| `glyim-def-map` | `petgraph`, `fixedbitset` |
| `glyim-lsp` | `async-lsp`, `tower`, `rowan` |
| `glyim-diag` | `miette`, `owo-colors`, `textwrap` |
| `glyim-cli` | `clap` |
| `glyim-codegen-llvm` | `inkwell` (linked against `llvm-sys` 231.x) |
| `glyim-server` | `tokio`, `tower`, `serde` |

---

## 8. Exclusions & Alternatives

**Excluded:**
*   **`lalrpop`**: We are building a custom Pratt parser using `chumsky`. Generated parsers like `lalrpop` add indirection and bloat compared to optimized combinators.
*   **`nom`**: `nom` is powerful but significantly increases binary size and compile times compared to `chumsky`.
*   **`syn`**: We are building a compiler, so we cannot depend on `syn` (which is Rust-specific). We use `rowan` for our syntax tree.
*   **`log`**: Replaced by `tracing` for better instrumentation support and `span` handling.
*   **`tower_lsp`**: While powerful, we prefer `async-lsp` for tighter integration with `tokio` and our custom server architecture.

**Alternatives Considered:**
*   **`logos`**: Considered for parsing, but `chumsky` was chosen for better error recovery support in an indentation-sensitive grammar.
*   **`cranelift`**: Considered as a primary backend for tiered compilation due to faster compile times. Currently adopted as an **experimental** backend (`glyim-codegen-cranelift`) not in v1.0 scope, but `inkwell` is the primary supported path.
*   **`llvm-sys` versions**: While 231 is the latest major version, we pin this tightly via `inkwell` to ensure our passes work reliably against the API surface.

---
```
