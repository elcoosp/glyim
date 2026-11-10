# Glyim Technology Stack Specification

**Document Version:** 2.0.0
**Status:** Updated
**Date:** 2026-11-09
**Source of truth:** the root `Cargo.toml` (`[workspace.package]`,
`[workspace.dependencies]`) at workspace version 0.7.0 (commit `61a0ecbc`).

> The previous revision (1.3.0, dated 2025-04-10) described an aspirational
> stack that never matched the tree: it listed `salsa`, `chumsky`, `egg`,
> `regalloc2`, `async-lsp`, `tower`, and crates that do not exist
> (`glyim-egraph`, `glyim-orchestrator`, `glyim-server`,
> `glyim-codegen-cranelift`). This revision lists only what the manifests
> actually declare. If a table here and a `Cargo.toml` disagree, the
> manifest wins (plan F1: status is checked against code, not trusted).

---

## 1. Toolchain & Build Rails

| Tool | Version | Purpose |
| :--- | :--- | :--- |
| Rust (stable channel) | MSRV `rust-version = "1.95"` (workspace `edition = "2024"`) | Language implementation; the `msrv` CI job compiles the workspace on 1.95.0 exactly. |
| LLVM | 23 (`LLVM_SYS_231_PREFIX`) | Hard dependency of the workspace via `inkwell`/`llvm-sys`; there is no feature flag to build without it. |
| `cargo-nextest` | current | Test runner used by CI and the `justfile`. |
| `cargo-deny` | current | Advisory + license gate (`deny.toml`). |
| `just` / `watchexec` | current | Dev recipes (`justfile`) and session watchers. |
| `cargo-fuzz` | nightly driver | `fuzz/` targets (`fuzz_parse`, `fuzz_compile`); excluded from the workspace build. |
| Node.js + pnpm | current | `glyim-docs` (Astro docs site) and the `tools/glyim-pilot` browser extension. |

---

## 2. Workspace Dependencies (root `Cargo.toml`)

Every member crate inherits these pins; versions below are the declared
minimums in `[workspace.dependencies]`.

| Crate | Version | Used by | Purpose |
| :--- | :--- | :--- | :--- |
| `rowan` | 0.17 | `glyim-syntax`, `glyim-frontend` | Lossless CST (green/red trees). |
| `lasso` | 0.7.3 | `glyim-core` | String interning (`ThreadedRodeo`). |
| `smol_str` | 0.3.7 | frontend | Small-string optimization for tokens/identifiers. |
| `num_enum` | 0.7.6 | `glyim-syntax` | `SyntaxKind` u16 conversions. |
| `bitflags` | 2.13 | `glyim-type` | Flag sets (type/attribute flags). |
| `indexmap` | 2.14 | `glyim-def-map`, `glyim-type` | Order-stable maps for item scopes. |
| `smallvec` | 1.16 | `glyim-borrowck`, `glyim-core`, `glyim-type` | Stack-allocated small collections in hot IR paths. |
| `fixedbitset` | 0.5.7 | `glyim-borrowck`, `glyim-lower` | Dense bitsets for dataflow/liveness. |
| `parking_lot` | 0.13 | `glyim-db`, harness | Faster `Mutex`/`RwLock` for interner and caches. |
| `dashmap` | 6.2.1 | `glyim-lsp` | Concurrent map for LSP state. |
| `tracing` / `tracing-subscriber` | 0.1.44 / 0.3.23 | typeck, lsp, test harness, codegen-llvm, glyip | Structured spans; subscriber with `env-filter` + `json`. |
| `miette` | 7.6 | `glyim-diag`, `glyim-span`, `glyim-test` (evidence + fixture-header errors) | Structured diagnostics with labels/help; `glyim-diag` re-exports it. |
| `clap` | 4.6 (derive) | `glyim-cli`, `glyip`, `tools/glyim-pilot` | CLI argument parsing. |
| `serde` / `serde_json` | 1.0 | `glyip`, ledger tools, pilot, harness | Config, lockfiles, evidence/JSON output. |
| `toml` | 1.2.1 | `glyip`, ledger | Manifest + registry parsing. |
| `semver` | 1.0 | `glyip` | SemVer 2.0 requirement parsing/matching. |
| `reqwest` / `flate2` / `tar` | 0.13 / 1 / 0.4 | `glyip` (feature `registry`) | Registry client: download + unpack crate archives. Declared in `glyip`'s own `[dependencies]` (not `[workspace.dependencies]`). |
| `sha2` | 0.11.1 | `glyim-db` (cache fingerprints), `glyip` (lockfiles), `glyim-test` (evidence fixture hashes) | SHA-256 content addressing. |
| `tempfile` | 3.27 | test-harness-heavy crates (13 manifests) | Scratch dirs for compile-and-run fixtures. |
| `tokio` | 1.54 | `glyim-lsp`, `tools/glyim-pilot` | Async runtime for the LSP server and the pilot agent. |
| `async-lsp` | git (`elcoosp/async-lsp`) | `glyim-lsp` (features `omni-trait`, `stdio`, `tokio`) | LSP server runtime layer on tokio. Git dependency — declared in `glyim-lsp`'s own manifest, not `[workspace.dependencies]`. |
| `lsp-types` | 0.97 | `glyim-lsp` | LSP wire types. |
| `libloading` | 0.9 | `glyim-proc-macro` | `dlopen`-based proc-macro cdylib loading. |
| `rayon` | 1.12 | `glyim-test` (parallel fixture execution) | Data parallelism in the harness. |
| `insta` | 1.49 | `glyim-test` | Snapshot testing (CST, def-map, MIR). |
| `anyhow` | 1.0.104 | `tools/glyim-pilot` | Agent-side error propagation (host tooling, not compiler core). |
| `inkwell` | git pin `c9d9734a…` | `glyim-codegen-llvm` | Safe LLVM bindings; drives `llvm-sys` 231.x (LLVM 23). |

Per-crate extras (declared directly by members): `termcolor`, `similar`,
`walkdir`, `regex`, `shell-words`, `rand` in `glyim-test`; `notify` 8.2
(file watching) in `glyim-lsp`; `uuid` v4 in `tools/glyim-pilot`; `syn` in
`tools/ledger-schema` (inventory extraction); `prometheus` (feature) in
`tools/glyim-pilot`.

---

## 3. Test & Supply-Chain Rails

| Rail | Mechanism |
| :--- | :--- |
| Fixture corpora | `glyim-test` harness: compile-pass / compile-fail / ui / run-pass / run-fail, M1.4 `//@` annotation grammar, ~410 tagged fixtures. |
| Differential gate | Every run-pass fixture compiled with the LLVM backend *and* run in the MIR interpreter; exit code + stdout must agree. |
| Metrics ratchet | `tools/glyim-metrics` vs `metrics/baseline.json` (code lines, tests, doc coverage, `inline_allows: 0`). |
| Readiness ledger | `ledger/features/**`, `ledger/gaps/**` checked by `tools/glyim-ledger check` (deny_unknown_fields schemas from `tools/ledger-schema`). |
| Lint rails | `missing_docs = warn` + `clippy::pedantic` baseline workspace-wide; zero inline allows (relaxations live in `[workspace.lints.clippy]` with rationale). |
| Miri | Nightly job over the pure-analysis crates (`glyim-type`, `glyim-borrowck`, `glyim-solve`). |

---

## 4. Exclusions & Alternatives

Deliberately **not** in the stack (as of 0.7.0):

*   **`salsa`** — the query engine is planned (M6, master plan); today
    `glyim-db` is an interner + VFS + `TyCtx` handle and the driver is a
    straight-line pipeline. Do not cite salsa as a current dependency.
*   **`chumsky`/`lalrpop`/`nom`** — the parser is hand-written (recursive
    descent + Pratt expression parsing in `glyim-frontend`) for error
    recovery control; no parser-combinator or generator framework.
*   **`egg` / `regalloc2` / equality-saturation machinery** — `glyim-opt`
    runs five hand-written MIR passes (const-prop, DCE, CFG-simplify,
    unreachable-elim, slice desugar); verified rewrites are the M10
    workstream, and the MIR interpreter is a value-model interpreter, not
    a register-allocated VM.
*   **`tower` / `tower_lsp`** — `glyim-lsp` builds on the `async-lsp`
    runtime instead (see below); the tower service stack is not used.
*   **Cranelift** — a ledger backend *tag* (`//@ backends: cranelift`) and
    a reserved `EvidenceBackend` variant only; there is no
    `glyim-codegen-cranelift` crate. LLVM and the bytecode backend are the
    two real code paths.

---
