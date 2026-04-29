# Glyim Roadmap

This document outlines the development trajectory of the Glyim programming language from the completed v0.2.0 release through the planned v0.10.0 release. Each version builds upon a strict, tiered architecture, adding fundamental language features, tooling, ecosystem support, and enterprise-grade production capabilities.

## Current Status (April 2026)

- **v0.2.0 (Feels Like a Real Language)** is released and stable.
- **v0.3.0 (The Type System)** is under active development on the `dev` branch.

All past and future designs are documented in formal Architecture & Design Specifications (`GLYM-ARCH-00x`). This roadmap summarises their scope and delivery targets.

---

## Roadmap Overview

| Version | Theme | Key Deliverables | Status |
|---------|-------|-----------------|--------|
| **v0.1.0** | Architectural Runway | End‑to‑end pipeline, 11 crates, strict DAG, CAS traits, `MacroContext` trait, hygiene framework | ✅ Released |
| **v0.2.0** | Feels Like a Real Language | `let`/`let mut`, `if`/`else`, strings, `println`/`assert`, Rowan CST, ariadne errors, JIT, `glyim init`, UI test framework | ✅ Released |
| **v0.3.0** | The Type System | Structs, enums, monomorphic type checker, `bool`/`Unit`, pattern matching, `Option<T>`/`Result<T,E>`, raw pointers & `@rust` FFI, `f64`, one working macro, `ExprId` plumbing, CI & file size enforcement | 🚧 In Progress |
| **v0.4.0** | Developer Experience | LSP server, formatter (`glyim-fmt`), macro expansion preview, `glyim-lint`, semantic tokens, inlay hints | 📅 Planned |
| **v0.5.0** | Ecosystem & Production | Package manager (`glyim-pkg`), standard library (`Vec`, `String`, `HashMap`, I/O), distributed CAS, remote build cache, DWARF debug info, test runner, cross‑compilation, `glyim doc`, `--release` mode | 📅 Planned |
| **v0.6.0** | Macros That Don’t Suck | Quote/splice syntax (`quote!{…}`), macro stepper, rich multi‑span errors, built‑in derive macros (`Debug`, `Clone`, `Serialize`, …), macro composability protocol, extended `MacroContext` reflection API, incremental macro expansion, attribute arguments | 📅 Planned |
| **v0.7.0** | The IDE That Makes You Smile | Salsa‑style incremental database (`glyim-ide-db`), go‑to‑definition through macros, auto‑completion with types, find‑all‑references, rename, code lenses (run/debug tests), DAP integration with macro‑aware debugging, code actions (add import, derive), inlay hints, hover docs | 📅 Planned |
| **v0.8.0** | Safe, Concurrent, Everywhere | Ownership & borrowing (gradual adoption, `--strict`), destructors (`Drop` trait, RAII), `?` operator, `From` trait, effect‑typed async/await (`!IO`, `!Async`), structured concurrency (`TaskGroup`), OS threads, channels, `Mutex`/`RWLock`, WASM target (`wasm32-wasi`), trait objects (`dyn Trait`), ownership‑aware macro errors | 📅 Planned |
| **v0.9.0** | The Onboarding Release | “The Glyim Book”, “Glyim by Example”, zero‑install browser playground (WASM‑based), full stdlib API documentation, migration guides (Rust, C++, Go), edition system (Glyim 2026) | 📅 Planned |
| **v0.10.0** | The Production Release | `glyim-ffi`: safe C/C++ header parsing & wrapper generation, C++ exception interception; first‑party `glyim::tracing` (OpenTelemetry), async‑aware profiling (`glyim profile` with eBPF), native SBOM generation (SPDX), SLSA provenance signing, dependency auditing (`glyim audit`) | 📅 Planned |

---

## Detailed Milestones

### ✅ v0.1.0 – Architectural Runway *(Released)*
- 11 crates organised in a strict 5‑tier DAG.
- Lossless syntax trees via Rowan.
- Trait‑based macro context (`MacroContext`) and content‑addressed store (`ContentStore`).
- End‑to‑end compiling `main = () => 42` to native via `inkwell`.

### ✅ v0.2.0 – Feels Like a Real Language *(Released)*
- **Syntax:** `let`/`let mut`, `if`/`else` expressions, string literals (`&str` fat pointer), `println`/`assert` built‑ins.
- **Tooling:** Rowan CST replaces raw enum parser, ariadne‑powered diagnostics, error recovery, JIT execution (`glyim run` without `cc`), `glyim init`.
- **Testing:** UI test framework with snapshot `.g/.stderr` pairs, fuzz harnesses.

### 🚧 v0.3.0 – The Type System *(In Development)*
- **Type Checking:** Monomorphic type checker (`HirType`, `TypeChecker`), local inference for `let`, explicit annotations on functions.
- **Data Types:** `struct` with named fields, `enum` as tagged unions, `match` with exhaustiveness check, `bool` as distinct type, `Unit` (`()`), `f64` float type, `as` cast.
- **Built‑in Generics:** `Option<T>` and `Result<T,E>` as built‑in monomorphized types, `?` operator for `Result` propagation.
- **FFI & Raw Pointers:** `@rust("libc") extern` blocks, `*const T`/`*mut T` types (no arithmetic).
- **Macros:** One working interpreted macro (`@identity`).
- **Refactoring:** Split god files, CI with GitHub Actions, file size enforcement (≤500 LOC).

### v0.4.0 – Developer Experience *(Planned)*
- **IDE:** Basic LSP skeleton (`glyim-lsp`) providing semantic tokens, hover, go‑to‑definition, and macro expansion preview.
- **Tooling:** Formatter (`glyim-fmt`), linting skeleton (`glyim-lint`), initial REPL improvements.

### v0.5.0 – Ecosystem & Production *(Planned)*
- **Package Manager:** `glyim.toml` manifest, content‑hash lockfile (`glyim.lock`), minimal‑version‑selection resolver, registry integration (`glyim add/remove/fetch/publish`), workspace support.
- **Standard Library:** `Vec<T>`, `String`, `HashMap<K,V>`, `Iterator<T>`, `Range<T>`, I/O (files, stdio), heap allocation, `#[no_std]` mode.
- **Caching & Distribution:** Remote content‑addressable store via Bazel REAPI protocol, remote build caching (`glyim cache`).
- **Debugging:** Full DWARF debug info via Inkwell’s `DIBuilder`, macro‑aware DWARF (points to macro call site).
- **Testing:** Built‑in test runner (`#[test]`, `glyim test`).
- **Cross‑Compilation:** Support for `aarch64`‑linux, `aarch64`‑darwin, `x86_64`‑darwin with lazy‑download sysroots.
- **Documentation:** `glyim doc` generates HTML from doc comments.
- **Optimisation:** `--release` mode with ThinLTO.

### v0.6.0 – Macros That Don’t Suck *(Planned)*
- **Ergonomics:** `quote! { ... }` blocks with `${expr}` splicing, `join`, `str` helpers.
- **Built‑in Derives:** `Debug`, `Clone`, `Eq`, `Hash`, `Serialize`, `Deserialize`, `Default` as standard library macros (not compiler intrinsics).
- **Composability:** Macros declare `requires { ... }`; compiler detects conflicting method generations.
- **Rich Errors:** Multi‑span errors that trace from macro invocation back through nested expansions.
- **API Extension:** `MacroContext` gains reflection for enum variants, function signatures, modules, caller location, and sandboxed file access.
- **Incremental Expansion:** Per‑invocation caching with dependency tracking; only changed invocations are re‑expanded.
- **Attribute Arguments:** Structured parsing `@serde(rename_all = "camelCase")` with typed values.
- **Macro Debugger:** `glyim macro-step` CLI and LSP code lens showing step‑by‑step expansion diffs.

### v0.7.0 – The IDE That Makes You Smile *(Planned)*
- **Incremental Database:** `glyim-ide-db` (Salsa‑style) caches parse → HIR → typeck → indices; invalidates only what changed.
- **Navigation:** Go‑to‑definition, find‑all‑references, and rename **through macro expansions** using `ExpansionMap`.
- **Completion:** Struct fields, trait methods, import insertion with type signatures.
- **Testing Integration:** Code lenses for `#[test]` (run/debug), inline test results.
- **Debugging:** DAP adapter (`glyim-dap`) translates macro‑generated stack frames to macro call sites.
- **Quick Fixes:** Add missing import, add `@derive`, fill struct fields.
- **Hints & Hover:** Inlay hints (type annotations, parameter names) and rich hover docs with macro expansion preview.

### v0.8.0 – Safe, Concurrent, Everywhere *(Planned)*
- **Memory Safety:** Gradual ownership/borrowing (runtime checks → compile‑time with `--strict`), destructors (`Drop` trait, RAII).
- **Error Handling:** `?` operator with automatic `From` conversion.
- **Concurrency:** Effect‑typed async/await (`!IO`, `!Async`), `TaskGroup` structured concurrency, OS threads, channels, `Mutex`, `RWLock`, scoped threads.
- **Targets:** WASM/WASI (`wasm32‑wasi`), `#[no_std]` embedded support.
- **Dynamic Dispatch:** `dyn Trait` fat pointers and vtables.
- **Ownership‑Aware Macros:** Borrow‑check errors inside generated code point back to the macro invocation.

### v0.9.0 – The Onboarding Release *(Planned)*
- **Documentation:** “The Glyim Book” (20+ chapters), “Glyim by Example” (50+ runnable concepts), full stdlib API docs with examples.
- **Playground:** Zero‑install browser environment (WASM compiler + Monaco editor, fuel‑bounded execution, URL sharing).
- **Editions:** `glyim.edition = "2026"` in `glyim.toml`, `glyim fix --edition 2026` for migration.
- **Migration Guides:** Side‑by‑side comparisons for Rust, C++, Go developers.

### v0.10.0 – The Production Release *(Planned)*
- **FFI:** `glyim‑ffi` procedural macro for safe C/C++ header parsing, automatic wrapper generation, C++ exception trapping.
- **Observability:** First‑party `glyim::tracing` (OpenTelemetry spans, implicit async context), `glyim profile` with eBPF‑based async‑aware flamegraphs.
- **Supply‑chain Security:** Native SBOM output (SPDX 2.3) via `glyim build --sbom`, SLSA provenance signing (Sigstore), `glyim audit` for vulnerability scanning.

---

## Timeline (High‑Level Estimate)

| Phase | Versions | Estimated Duration |
|-------|----------|-------------------|
| Foundation & Core | v0.1.0 – v0.3.0 | Q1‑Q3 2026 |
| Developer Experience | v0.4.0 | Q4 2026 |
| Ecosystem & Production | v0.5.0 | Q1 2027 |
| Macros 2.0 | v0.6.0 | Q2 2027 |
| IDE & Debugging | v0.7.0 | Q3 2027 |
| Safety & Concurrency | v0.8.0 | Q4 2027 |
| Onboarding | v0.9.0 | Q1 2028 |
| Enterprise Readiness | v0.10.0 | Q2 2028 |

*Dates are aspirational and subject to change based on contributor velocity and design refinements.*

---

## How to Contribute

All design decisions are recorded in Architecture Decision Records (`docs/adr/`) and the versioned specifications (`docs/specs/v0.X.0.md`). The current development focus is **v0.3.0**. If you want to help:

- Check the `dev` branch for in‑progress work.
- Read the v0.3.0 spec and the corresponding execution plan.
- Pick a TDD‑sized task from the phase list and open a PR.

For any questions about the roadmap or to propose a new feature, open a Discussion in the repository.
