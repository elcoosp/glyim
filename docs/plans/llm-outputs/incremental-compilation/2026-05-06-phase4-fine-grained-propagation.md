# Glyim Incremental Compiler — Phase 4 Implementation Plan

## Fine-Grained Incremental Recompilation & Change Propagation

**Codebase:** elcoosp-glyim v0.5.0  
**Rust Workspace | 20 Crates | LLVM 22.1 / Inkwell 0.9**  
**Date:** 2026-05-07

---

## 1. Executive Summary

Phase 4 is the integration phase that transforms Glyim from a compiler with isolated incremental infrastructure components into a fully operational incremental compilation platform. Phases 0 through 3 built the foundational machinery — the query engine (`glyim-query`), the Merkle IR tree (`glyim-merkle`), JIT micro-modules, and the e-graph optimizer — but these components currently exist as separate, partially connected subsystems. The linear pipeline in `glyim-compiler/src/pipeline.rs` still performs a full recompilation on every invocation, the `--incremental` CLI flag is a no-op placeholder, and the `compile_source_to_hir_incremental()` function, while structurally present, does not actually skip any work when source files are unchanged.

Phase 4 closes this gap by refactoring the compilation pipeline into a query-driven architecture where each compilation stage is a memoized query keyed by the semantic hash of its inputs. When a source file changes, only the queries whose fingerprints differ are re-executed; all other queries return their cached results from the `IncrementalState`. This is the red/green invalidation model already implemented in `glyim-query/src/invalidation.rs`, but it must be wired into the actual compilation stages. The Merkle store will cache per-function compilation artifacts (typed HIR, e-graph-optimized HIR, LLVM IR, object code), enabling fine-grained reuse when a single function changes within a multi-function module. The e-graph optimizer will participate in incremental invalidation through its `InvariantCertificate` mechanism, so functions whose optimization invariants are unchanged skip the equality saturation pass entirely.

The phase also introduces a file-watcher-based `glyim watch` command that continuously monitors source files, triggers incremental recompilation on changes, and reports recompilation diagnostics (what was recompiled, why, and how long it took). This provides the "live semantics" foundation that later phases will build upon for hot code swap and REPL-style interaction.

**Estimated effort:** 25–34 working days.

**Key deliverables:**
- Query-driven pipeline replacing the linear `compile_source_to_hir()` flow
- Function-granularity red/green change detection and artifact caching via Merkle store
- Working `--incremental` CLI flag on `build`, `check`, and `run` commands
- Incremental test execution via `glyim-testr`
- `glyim watch` command with file-watcher integration
- Incremental compilation diagnostics and reporting

---

## 2. Current Codebase State Assessment

### 2.1 The Linear Pipeline (As-Is)

The current compilation pipeline is defined in `glyim-compiler/src/pipeline.rs`. The `compile_source_to_hir()` function executes a fixed sequence of stages:

1. **Macro expansion** — `crate::macro_expand::expand_macros()` transforms source via Wasm procedural macros
2. **Parsing** — `glyim_parse::parse()` produces an AST and interner
3. **Declaration scanning** — `glyim_parse::declarations::parse_declarations()` builds a `DeclTable`
4. **HIR lowering** — `glyim_hir::lower_with_declarations()` produces the HIR
5. **Type checking** — `TypeChecker::check()` produces `expr_types` and `call_type_args`
6. **Method desugaring** — `glyim_hir::desugar_method_calls()` rewrites method calls
7. **Monomorphization** — `merge_mono_types()` produces concrete `mono_hir` and `merged_types`

The resulting `CompiledHir` struct is passed directly to `Codegen::generate()` in `glyim-codegen-llvm/src/codegen/mod.rs`, which compiles the entire module in a single pass. There is no mechanism to skip stages when inputs are unchanged, no per-function caching, and no way to recompile only the functions that were affected by a source change.

The `compile_source_to_hir_incremental()` function already exists but is essentially a thin wrapper around `compile_source_to_hir()`. It creates an `IncrementalState`, computes a source fingerprint, and records it in the query context, but it does not actually use the query context to skip any work. The comment in the code says "On subsequent calls it loads previous state, detects changes, invalidates affected queries, and re-runs only the Red stages," but the implementation just calls `compile_source_to_hir()` unconditionally.

### 2.2 Existing Incremental Infrastructure

| Component | Crate | Status | Gap |
|-----------|-------|--------|-----|
| Query Engine | `glyim-query` | Implemented: `QueryContext`, `Fingerprint`, `DependencyGraph`, `InvalidationReport` | Not wired into pipeline stages |
| Incremental State | `glyim-query/src/incremental.rs` | Implemented: `IncrementalState` with source hash tracking, persistence | Only tracks source-level hashes, not per-function or per-query |
| Invalidation | `glyim-query/src/invalidation.rs` | Implemented: red/green computation via transitive dependents | Dependency graph is not populated with actual compilation dependencies |
| Merkle Store | `glyim-merkle` | Implemented: `MerkleStore` with CAS-backed `put`/`get`/`contains` | Not used for caching compilation artifacts |
| Content Store | `glyim-macro-vfs` | Implemented: `LocalContentStore`, `RemoteContentStore`, Bazel REv2 | Only used for macro caching, not for compilation artifacts |
| Semantic Hash | `glyim-hir/src/semantic_hash.rs` | Implemented: SHA-256 over normalized HIR | Not used as query fingerprint key |
| Granularity Monitor | `glyim-query/src/granularity.rs` | Implemented: `GranularityMonitor`, `EditHistory` | Not connected to pipeline |
| CAS Object Cache | `glyim-compiler/src/pipeline.rs` | Partial: `build_with_cache()` caches whole-module object files | Coarse-grained (entire module), no per-function caching |
| CLI `--incremental` | `glyim-cli/src/commands/cmd_build.rs` | Placeholder: prints a note, does nothing | Not functional |

### 2.3 Critical Gaps That Phase 4 Addresses

| Gap | Impact | Affected Crate | Phase 4 Solution |
|-----|--------|---------------|-------------------|
| Pipeline is linear, not query-driven | Every build recompiles everything from scratch | `glyim-compiler` | Refactor pipeline into memoized queries |
| No per-function change detection | Changing one function recompiles the entire module | `glyim-compiler`, `glyim-query` | Function-granularity fingerprints via semantic hash |
| Dependency graph is empty | Invalidation cannot propagate from changed sources to dependent queries | `glyim-query` | Populate dependency graph during pipeline execution |
| Merkle store not used for artifacts | Typed HIR, e-graph output, object code are never cached | `glyim-merkle` | Cache per-function artifacts in MerkleStore |
| `--incremental` is a no-op | Users cannot opt into incremental compilation | `glyim-cli` | Wire the flag to the query-driven pipeline |
| No file watcher | No `glyim watch` command for continuous compilation | (missing) | Add `cmd_watch.rs` with `notify` crate |
| No incremental test running | All tests re-run even when unrelated to changes | `glyim-testr` | Wire `testr/incremental.rs` to the real dependency graph |
| No recompilation diagnostics | Users cannot see what was recompiled and why | (missing) | Add `IncrementalReport` with timing and invalidation trace |

---

## 3. Architecture Design

### 3.1 Query-Driven Pipeline

The fundamental architectural change is replacing the sequential function calls in `compile_source_to_hir()` with a set of memoized queries. Each query is a function that:
1. Computes a fingerprint from its inputs
2. Checks the `QueryContext` for a cached result with that fingerprint
3. If cached and green, returns the cached result
4. If not cached or red, executes the computation, stores the result, and records its dependencies

The query functions replace the current pipeline stages:

```
Current (linear):            Query-driven:
─────────────────            ──────────────
expand_macros()       →      query_macro_expand(source_hash)
parse()               →      query_parse(expanded_source_hash)
parse_declarations()  →      query_declarations(ast_hash)
lower_with_decls()    →      query_lower(ast_hash, decl_table_hash)
typeck::check()       →      query_typeck(hir_hash)
desugar_methods()     →      query_desugar(hir_hash, type_info_hash)
monomorphize()        →      query_mono(hir_hash, type_info_hash)
egraph::optimize()    →      query_optimize(mono_hir_hash, effect_hash)
codegen::generate()   →      query_codegen(optimized_hir_hash)
```

Each query function follows a uniform pattern:

```rust
fn query_<stage>(
    ctx: &mut QueryContext,
    inputs: &StageInputs,
    deps: Vec<Dependency>,
) -> Result<Arc<StageOutput>, PipelineError> {
    let fp = Fingerprint::combine(
        Fingerprint::of_str(STAGE_NAME),
        inputs.fingerprint(),
    );
    if let Some(cached) = ctx.get(&fp) {
        return Ok(cached.downcast::<StageOutput>());
    }
    let result = compute_stage(inputs)?;
    ctx.insert(fp, Arc::new(result.clone()), fp, deps);
    Ok(Arc::new(result))
}
```

### 3.2 Fingerprint Granularity

The current `IncrementalState` tracks fingerprints at the source-file level: one `Fingerprint` per source file, computed from the raw source bytes. This is too coarse for fine-grained incremental compilation because a change to a comment or an unused function invalidates the entire file.

Phase 4 introduces a two-level fingerprint hierarchy:

1. **Module-level fingerprint** — computed from the raw source bytes (unchanged from current behavior). This is the "dirty check" that determines whether any query in the module needs re-execution.
2. **Item-level fingerprint** — computed from the `SemanticHash` of each `HirItem`. After parsing and lowering, each function, struct, enum, and impl block gets its own semantic hash. The item-level fingerprint is used as the query key for per-function type checking, optimization, and code generation.

The `SemanticHash` infrastructure already exists in `glyim-hir/src/semantic_hash.rs` and computes SHA-256 hashes over the normalized form of each `HirItem`. The `semantic_hash_item()` function is already called in `pipeline.rs` (in `semantic_hash_of_source()`), but its output is not used for incremental invalidation — it is only used for CAS object caching. Phase 4 wires the per-item semantic hashes into the `QueryContext` as the primary cache keys for all downstream queries.

### 3.3 Dependency Graph Population

The `DependencyGraph` in `glyim-query/src/dep_graph.rs` is a `petgraph::DiGraph` that maps from fingerprint to fingerprint, enabling transitive dependency computation. Currently, this graph is empty because no pipeline stage records its dependencies. Phase 4 populates the graph as follows:

| Query | Dependencies |
|-------|-------------|
| `query_parse` | Source file fingerprint |
| `query_lower` | AST fingerprint, declaration table fingerprint |
| `query_typeck` | HIR fingerprint (per-item), all called-function HIR fingerprints |
| `query_desugar` | HIR fingerprint, type info fingerprint |
| `query_mono` | HIR fingerprint, type info fingerprint, all generic instantiation fingerprints |
| `query_optimize` | Mono HIR fingerprint (per-function), effect fingerprint, invariant certificate |
| `query_codegen` | Optimized HIR fingerprint (per-function) |

When a source file changes, the module-level fingerprint changes, which invalidates `query_parse`. The parse query's output fingerprint changes, which invalidates `query_lower`, and so on transitively. However, at the item level, only items whose semantic hashes actually changed are re-processed. Items whose semantic hashes are identical (because the change was in a different function) remain green, and their downstream queries return cached results.

### 3.4 Per-Function Artifact Caching via Merkle Store

The `MerkleStore` (in `glyim-merkle/src/store.rs`) is a content-addressed cache backed by the CAS infrastructure (`LocalContentStore` from `glyim-macro-vfs`). Currently, it is only used for Merkle tree node storage. Phase 4 extends it to cache per-function compilation artifacts.

Each function's compilation artifacts are stored as a `MerkleNode` whose children are the hashes of its inputs:

```
MerkleNode for function "add":
  header: { kind: "fn_artifact", name: "add", hash: <semantic_hash> }
  children:
    - MerkleNode { kind: "typed_hir", data: <serialized HirFn with type info> }
    - MerkleNode { kind: "effects", data: <serialized EffectSet> }
    - MerkleNode { kind: "optimized_hir", data: <serialized optimized HirFn> }
    - MerkleNode { kind: "llvm_ir", data: <LLVM IR string> }
    - MerkleNode { kind: "object_code", data: <compiled .o bytes> }
```

When a function's semantic hash matches a cached MerkleNode, all five artifacts are available without recompilation. This is the same principle as the existing `build_with_cache()` function, but at function granularity instead of module granularity.

### 3.5 Data Flow Through the Incremental Pipeline

The incremental pipeline follows this flow:

1. **Dirty check:** Compute the source file fingerprint. If it matches the cached fingerprint, skip all queries and return the cached result. If not, proceed to step 2.
2. **Parse and lower:** Re-parse and lower the source. This is unavoidable because the source changed.
3. **Item diff:** Compute the semantic hash of each `HirItem`. Compare with cached item hashes. Identify which items are red (changed) and which are green (unchanged).
4. **Per-item type checking:** For red items, re-run type checking. For green items, load cached `expr_types` and `call_type_args` from the Merkle store.
5. **Per-item desugaring and monomorphization:** Same red/green logic.
6. **Per-item e-graph optimization:** For red items, run equality saturation (or skip if the `InvariantCertificate` matches). For green items, load the cached optimized HIR from the Merkle store.
7. **Per-item code generation:** For red items, re-run LLVM code generation. For green items, load the cached object code from the Merkle store.
8. **Link:** Combine all object files (red and green) and link into the final binary.

This flow ensures that a change to a single function only triggers recompilation of that function (plus any functions that depend on it transitively through the dependency graph).

---

## 4. Query-Driven Pipeline Specification

### 4.1 New File: `glyim-compiler/src/queries.rs`

This file defines all query functions and the `QueryPipeline` struct that orchestrates them.

```rust
// crates/glyim-compiler/src/queries.rs

use glyim_query::{QueryContext, Fingerprint, Dependency, IncrementalState};
use glyim_merkle::MerkleStore;
use glyim_interner::Interner;
use std::sync::Arc;

/// Top-level orchestrator for the query-driven incremental pipeline.
pub struct QueryPipeline {
    ctx: QueryContext,
    merkle: MerkleStore,
    interner: Interner,
    config: PipelineConfig,
    report: IncrementalReport,
}

/// Diagnostic report for incremental compilation.
#[derive(Debug, Clone, Default)]
pub struct IncrementalReport {
    pub total_items: usize,
    pub red_items: Vec<String>,        // items that were recompiled
    pub green_items: Vec<String>,      // items that were reused
    pub cache_hits: usize,
    pub cache_misses: usize,
    pub stage_timings: Vec<(String, std::time::Duration)>,
    pub total_elapsed: std::time::Duration,
}
```

### 4.2 Query Functions

Each query function follows the same pattern. Here are the specifications for all queries:

#### 4.2.1 `query_macro_expand`

- **Input:** Source string + source path
- **Fingerprint:** `Fingerprint::of(source.as_bytes())`
- **Output:** `Arc<String>` (expanded source)
- **Dependencies:** Source file path + source fingerprint
- **Cache key:** Source fingerprint only (macro expansion is deterministic given the source)
- **Merkle artifact:** None (the expanded source is the input to the next query)

#### 4.2.2 `query_parse`

- **Input:** Expanded source string
- **Fingerprint:** `Fingerprint::of(expanded_source.as_bytes())`
- **Output:** `Arc<ParseOutput>` (AST + interner)
- **Dependencies:** Expanded source fingerprint
- **Cache key:** Expanded source fingerprint
- **Merkle artifact:** `MerkleNode { kind: "ast", data: serialized AST }`

#### 4.2.3 `query_declarations`

- **Input:** Expanded source string
- **Fingerprint:** `Fingerprint::of(expanded_source.as_bytes())` (same source, different view)
- **Output:** `Arc<DeclTable>`
- **Dependencies:** Expanded source fingerprint
- **Cache key:** Expanded source fingerprint
- **Merkle artifact:** `MerkleNode { kind: "decl_table", data: serialized DeclTable }`

#### 4.2.4 `query_lower`

- **Input:** Parse output + declaration table
- **Fingerprint:** `Fingerprint::combine(ast_fp, decl_table_fp)`
- **Output:** `Arc<Hir>`
- **Dependencies:** Parse fingerprint, declaration table fingerprint
- **Cache key:** Combined fingerprint
- **Merkle artifact:** `MerkleNode { kind: "hir", data: serialized Hir }`

#### 4.2.5 `query_typeck`

- **Input:** HIR + interner
- **Fingerprint:** Per-item: `semantic_hash_item(item, &interner)` converted to `Fingerprint`
- **Output:** `Arc<TypeInfo>` (expr_types + call_type_args + typed HirFn map)
- **Dependencies:** Per-item HIR fingerprints, plus fingerprints of all called functions
- **Cache key:** Per-item semantic hash
- **Merkle artifact:** `MerkleNode { kind: "typed_hir", data: serialized TypeInfo for one function }`

This is the first query that operates at item granularity. The type checker currently processes all items in a single `TypeChecker::check()` call, but it must be refactored to support per-function checking. This is possible because the type checker already maintains per-function state (the `expr_types` vector is indexed by `ExprId`, and each function's `ExprId`s are independent). The refactoring involves:

1. Extracting the per-function type checking loop from `TypeChecker::check()` into a `TypeChecker::check_fn(&mut self, f: &HirFn)` method
2. Computing the `expr_types` slice for each function independently
3. Storing the per-function `expr_types` in a `HashMap<Symbol, Vec<HirType>>`
4. During incremental recompilation, only calling `check_fn()` for red functions and loading cached `expr_types` for green functions

#### 4.2.6 `query_desugar`

- **Input:** HIR + type info
- **Fingerprint:** `Fingerprint::combine(hir_fp, type_info_fp)`
- **Output:** `Arc<Hir>` (with desugared method calls)
- **Dependencies:** HIR fingerprint, type info fingerprint
- **Cache key:** Combined fingerprint
- **Merkle artifact:** `MerkleNode { kind: "desugared_hir", data: serialized Hir }`

#### 4.2.7 `query_mono`

- **Input:** Desugared HIR + type info
- **Fingerprint:** Per-item: semantic hash of the desugared item
- **Output:** `Arc<MonoResult>` (mono_hir + merged_types + type_overrides)
- **Dependencies:** Per-item HIR fingerprints, all generic instantiation fingerprints
- **Cache key:** Per-item semantic hash
- **Merkle artifact:** `MerkleNode { kind: "mono_hir", data: serialized monomorphized HirFn }`

#### 4.2.8 `query_optimize`

- **Input:** Mono HIR + effect analysis + type info
- **Fingerprint:** Per-item: semantic hash + effect hash + invariant certificate version
- **Output:** `Arc<Hir>` (e-graph-optimized)
- **Dependencies:** Mono HIR fingerprint, effect fingerprint, rule set version
- **Cache key:** Per-item semantic hash (checked against `InvariantCertificate`)
- **Merkle artifact:** `MerkleNode { kind: "optimized_hir", data: serialized optimized HirFn + InvariantCertificate }`

This query integrates directly with the `InvariantCertificate` mechanism from Phase 3. The query first checks whether the certificate matches the cached version; if so, it returns the cached optimized HIR without running equality saturation.

#### 4.2.9 `query_codegen`

- **Input:** Optimized HIR + type info + target triple + optimization level
- **Fingerprint:** Per-item: semantic hash + config fingerprint
- **Output:** `Arc<CodegenArtifact>` (LLVM IR string + object code bytes)
- **Dependencies:** Optimized HIR fingerprint, config fingerprint
- **Cache key:** Per-item semantic hash + config fingerprint
- **Merkle artifact:** `MerkleNode { kind: "object_code", data: compiled .o bytes }`

This is the most impactful query for incremental performance because LLVM code generation is the most expensive pipeline stage. Caching per-function object code means that changing one function only requires recompiling that one function's object code, then re-linking the entire module.

### 4.3 Pipeline Refactoring Strategy

The refactoring must be incremental (ironically) to avoid breaking the existing compilation pipeline. The strategy is:

1. **Add the `queries.rs` module** alongside the existing `pipeline.rs`. The new module defines the `QueryPipeline` struct and all query functions, but does not modify `pipeline.rs`.
2. **Add a feature flag** `query-pipeline` that gates the new pipeline. When the feature is off, the existing linear pipeline is used. When the feature is on, the `QueryPipeline` is used.
3. **Wire the feature flag into the CLI** via the `--incremental` flag. When `--incremental` is passed, the `query-pipeline` feature is activated and the `QueryPipeline` is used.
4. **Gradually migrate** each pipeline stage from the linear flow to the query-driven flow, testing each stage individually.
5. **Once all stages are migrated**, remove the feature flag and make the query-driven pipeline the default.

This approach ensures that the compiler continues to work correctly at every step, and that the incremental pipeline can be tested independently.

---

## 5. Per-Function Code Generation

### 5.1 The Challenge

The current `Codegen::generate()` method in `glyim-codegen-llvm/src/codegen/mod.rs` compiles the entire `Hir` module in a single pass. It makes three passes over the module: first to register types and extern declarations, second to forward-declare all functions, and third to emit function bodies. This whole-module approach prevents per-function incremental compilation because even if only one function changed, the entire module must be recompiled.

### 5.2 Solution: Function-Level Codegen

Phase 4 refactors the `Codegen` struct to support per-function code generation. The key changes are:

1. **Split `generate()` into three phases:**
   - `prepare_module(&mut self, hir: &Hir)` — registers types, extern declarations, and forward-declares functions (Pass 1 + Pass 2 from the current code)
   - `codegen_fn(&mut self, f: &HirFn) -> Result<(), String>` — emits the body of a single function (Pass 3, per-function)
   - `finalize_module(&mut self) -> Result<(), String>` — verifies the module and emits debug info

2. **Cache the `Codegen` struct** between incremental compilations. The struct's `mono_cache`, `struct_types`, `enum_types`, and other internal state are reusable across compilations of the same module, as long as the type declarations have not changed.

3. **For incremental compilation:**
   - On first compilation: call `prepare_module()` + `codegen_fn()` for all functions + `finalize_module()`
   - On subsequent compilations: call `prepare_module()` (if type declarations changed), then `codegen_fn()` only for red functions, then `finalize_module()`

### 5.3 Object Code Extraction per Function

The LLVM code generator produces a single object file containing all functions. For per-function caching, we need to extract individual function object code. This is achieved through LLVM's section attributes:

1. Each function is annotated with `__attribute__((section(".glyim.fn.<name>")))` via inkwell's `set_section()` API.
2. After writing the object file, the object code for each function section is extracted using a lightweight ELF/Mach-O parser (the `object` crate).
3. Each function's object code is stored in the Merkle store as a `MerkleNode { kind: "object_code", data: section_bytes }`.

On incremental recompilation, the green functions' object code sections are loaded from the Merkle store, and the red functions' object code sections are produced by the code generator. All sections are combined into a single relocatable object file using the `object` crate's write API, then linked as usual.

### 5.4 New Module: `glyim-codegen-llvm/src/incremental.rs`

```rust
// crates/glyim-codegen-llvm/src/incremental.rs

use crate::Codegen;
use glyim_hir::Hir;
use glyim_merkle::MerkleStore;
use std::sync::Arc;

/// Manages incremental code generation state.
pub struct IncrementalCodegen {
    /// The LLVM module, reused across compilations.
    codegen: Option<Codegen<'static>>,
    /// Per-function object code cache.
    merkle: Arc<MerkleStore>,
    /// Functions that were recompiled in this session.
    recompiled: Vec<String>,
}

impl IncrementalCodegen {
    pub fn new(merkle: Arc<MerkleStore>) -> Self { ... }

    /// Compile a full module (first build or full rebuild).
    pub fn compile_full(&mut self, hir: &Hir, ...) -> Result<Vec<FnObjectCode>, String> { ... }

    /// Compile only the changed functions (incremental build).
    pub fn compile_incremental(
        &mut self,
        hir: &Hir,
        red_fns: &[String],
        green_fn_hashes: &[(String, ContentHash)],
        ...
    ) -> Result<Vec<FnObjectCode>, String> { ... }

    /// Extract per-function object code sections from a compiled module.
    fn extract_fn_sections(obj_bytes: &[u8]) -> Result<HashMap<String, Vec<u8>>, String> { ... }

    /// Combine per-function object code sections into a single relocatable object.
    fn combine_fn_sections(
        sections: &HashMap<String, Vec<u8>>,
    ) -> Result<Vec<u8>, String> { ... }
}
```

---

## 6. Dependency Graph Construction

### 6.1 Static Dependencies

Static dependencies are determined by analyzing the HIR. A function `f` statically depends on:
- The source file it is defined in
- Every function it calls (by name)
- Every struct/enum type it references
- Every method it dispatches on

These dependencies are computed during the `query_typeck` phase by walking the `HirExpr` tree of each function and collecting all `HirExpr::Call`, `HirExpr::MethodCall`, and type references. The `dependency_names` module in `glyim-hir/src/dependency_names.rs` already implements this analysis — it extracts the set of symbols that a function depends on.

### 6.2 Dynamic Dependencies

Dynamic dependencies arise from macro expansion. A macro may generate code that calls functions not present in the original source, creating dependencies that are not visible until macro expansion is complete. These dependencies are captured by recording the expanded source fingerprint as a dependency of the parse and lower queries.

### 6.3 Dependency Graph Update Protocol

The dependency graph is updated during each query execution:

1. When `query_typeck` processes a function, it records a dependency edge from the function's fingerprint to the fingerprints of all functions it calls.
2. When `query_mono` processes a generic function, it records a dependency edge from the monomorphized function's fingerprint to the generic function's fingerprint.
3. When `query_optimize` processes a function, it records a dependency edge from the optimized function's fingerprint to the mono function's fingerprint and the invariant certificate fingerprint.

After all queries complete, the dependency graph contains the complete set of edges. When a source file changes, the `invalidate()` function in `glyim-query/src/invalidation.rs` computes the transitive closure of affected fingerprints, producing the red/green partition.

### 6.4 Cross-Module Dependencies

Phase 4 handles single-module incremental compilation. Cross-module dependencies (where a function in module A calls a function in module B) are deferred to Phase 5 (Cross-Module Incremental Linking). In Phase 4, cross-module calls are treated as opaque: they are assumed to be stable (their fingerprints do not change unless the called module is recompiled). This is a conservative approximation that may cause unnecessary recompilation in rare cases, but it is correct.

---

## 7. File Watcher & `glyim watch` Command

### 7.1 New Crate: `glyim-watch`

```
crates/glyim-watch/
├── Cargo.toml
└── src/
    ├── lib.rs          — public API
    ├── watcher.rs      — file system watcher using `notify` crate
    ├── session.rs      — incremental compilation session
    └── diagnostics.rs  — recompilation reporting
```

### 7.2 Cargo.toml

```toml
[package]
name = "glyim-watch"
version.workspace = true
edition.workspace = true
license.workspace = true
rust-version.workspace = true
description = "File watcher and continuous compilation for Glyim"

[dependencies]
notify = "7"
glyim-compiler = { path = "../glyim-compiler" }
glyim-query = { path = "../glyim-query" }
glyim-merkle = { path = "../glyim-merkle" }
tracing = "0.1"
colored = "3"
```

### 7.3 Watcher Architecture

The `glyim watch` command runs a persistent process that:

1. **Initial compilation:** Compiles the project using the incremental pipeline, establishing the initial `IncrementalState` and Merkle store.
2. **File watching:** Uses the `notify` crate to monitor the project's source directory for file changes. The watcher filters for `.g` file extensions and ignores temporary files, build artifacts, and the CAS directory.
3. **Debouncing:** Waits for a 100ms quiet period after the last file change event before triggering recompilation. This prevents spurious recompilations when a file is saved multiple times in rapid succession (e.g., by IDE auto-save).
4. **Incremental recompilation:** When a change is detected, computes the source fingerprint diff, identifies changed items, and recompiles only the red items.
5. **Diagnostics output:** Prints a summary of what was recompiled, how many items were red vs. green, and the total time.

### 7.4 CLI Command

```rust
// crates/glyim-cli/src/commands/cmd_watch.rs

use glyim_compiler::pipeline::PipelineConfig;
use glyim_watch::WatchSession;
use std::path::PathBuf;

pub fn cmd_watch(input: PathBuf, target: Option<String>) -> i32 {
    let config = PipelineConfig {
        jit_mode: false,
        ..Default::default()
    };
    let mut session = WatchSession::new(input, config);
    match session.run() {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("watch error: {e}");
            1
        }
    }
}
```

### 7.5 Diagnostics Format

The watch command outputs recompilation diagnostics in a human-readable format:

```
[watch] Detected change: src/main.g (2 items changed)
[watch] Recompiling: add, factorial
[watch] Reused from cache: main, fibonacci, Vec.push, Vec.new
[watch] Incremental build: 3.2ms (2 red, 4 green, 0.8ms typeck, 1.1ms egraph, 0.9ms codegen)
[watch] Build succeeded. Watching for changes...
```

---

## 8. Incremental Test Execution

### 8.1 Current State

The test runner's incremental module (`glyim-testr/src/incremental.rs`) is a placeholder that always returns all tests. The `DependencyGraph` struct has no fields and its `affected_tests()` method ignores the `changed_files` parameter.

### 8.2 Refactoring

Phase 4 replaces the placeholder with a real dependency-aware test selector:

```rust
// crates/glyim-testr/src/incremental.rs (rewritten)

use crate::types::TestDef;
use glyim_query::{DependencyGraph, Fingerprint};
use std::collections::{HashMap, HashSet};

pub struct TestDependencyGraph {
    /// Maps each test name to the set of HIR item fingerprints it depends on.
    test_deps: HashMap<String, HashSet<Fingerprint>>,
    /// Maps each HIR item fingerprint to the source file it came from.
    item_sources: HashMap<Fingerprint, String>,
}

impl TestDependencyGraph {
    pub fn new() -> Self { ... }

    /// Record that a test depends on specific HIR items.
    pub fn add_test_dependency(&mut self, test_name: &str, item_fp: Fingerprint) { ... }

    /// Given a set of changed files, compute which tests are affected.
    pub fn affected_tests(
        &self,
        changed_files: &HashSet<&str>,
        all_tests: &[TestDef],
    ) -> Vec<TestDef> {
        // Compute affected item fingerprints from changed files
        let affected_items: HashSet<Fingerprint> = self.item_sources.iter()
            .filter(|(_, src)| changed_files.contains(src.as_str()))
            .map(|(fp, _)| *fp)
            .collect();

        // Return tests whose dependencies intersect with affected items
        all_tests.iter()
            .filter(|test| {
                self.test_deps.get(&test.name)
                    .map(|deps| deps.iter().any(|d| affected_items.contains(d)))
                    .unwrap_or(true) // conservative: run test if deps unknown
            })
            .cloned()
            .collect()
    }
}
```

### 8.3 Integration with Query Pipeline

The test dependency graph is populated during the `query_typeck` phase. When the type checker processes a test function (identified by the `#[test]` attribute or the `test_` naming convention), it records the fingerprints of all HIR items that the test function depends on. This information is stored in the `IncrementalState` and reused across builds.

When the `glyim test --incremental` command is invoked, the test runner:
1. Loads the `IncrementalState` from the previous build
2. Computes which source files changed
3. Uses `TestDependencyGraph::affected_tests()` to determine which tests to run
4. Runs only the affected tests

---

## 9. CLI Integration

### 9.1 Modified Commands

| Command | New Flag | Behavior |
|---------|----------|----------|
| `glyim build` | `--incremental` | Uses `QueryPipeline` instead of linear pipeline |
| `glyim check` | `--incremental` | Uses `QueryPipeline` for type checking only |
| `glyim run` | `--incremental` | Uses `QueryPipeline` + JIT with incremental object code |
| `glyim test` | `--incremental` | Runs only affected tests |
| `glyim watch` | (new command) | Continuous incremental compilation |
| `glyim build` | `--incremental-status` | Prints cache hit/miss statistics without building |

### 9.2 `--incremental` Flag Wiring

The `--incremental` flag currently exists in `cmd_build.rs` as a placeholder. Phase 4 wires it to the `QueryPipeline`:

```rust
// crates/glyim-cli/src/commands/cmd_build.rs (modified)

pub fn cmd_build(
    input: PathBuf,
    output: Option<PathBuf>,
    target: Option<String>,
    release: bool,
    bare: bool,
    incremental: bool,  // NOW FUNCTIONAL
) -> i32 {
    let mode = if release { BuildMode::Release } else { BuildMode::Debug };
    let result = if incremental {
        pipeline::build_incremental(&input, output.as_deref(), mode, target.as_deref())
    } else if bare || input.is_file() {
        pipeline::build_with_mode(&input, output.as_deref(), mode, target.as_deref(), None)
    } else {
        pipeline::build_package(&input, output.as_deref(), mode, target.as_deref())
    };
    match result {
        Ok(path) => { eprintln!("Built: {}", path.display()); 0 }
        Err(e) => { eprintln!("error: {e}"); 1 }
    }
}
```

### 9.3 New Entry Point: `build_incremental`

```rust
// In glyim-compiler/src/pipeline.rs

pub fn build_incremental(
    input: &Path,
    output: Option<&Path>,
    mode: BuildMode,
    target: Option<&str>,
) -> Result<(PathBuf, IncrementalReport), PipelineError> {
    let (source, _) = load_source_with_prelude(input)?;
    let cache_dir = input.parent()
        .unwrap_or(Path::new("."))
        .join(".glyim/incremental");

    let mut query_pipeline = QueryPipeline::new(
        &cache_dir,
        PipelineConfig { mode, target: target.map(String::from), ..Default::default() },
    );

    let compiled = query_pipeline.compile(&source, input)?;
    let report = query_pipeline.report().clone();

    // ... generate object code, link ...
    Ok((output_path, report))
}
```

---

## 10. Incremental State Persistence

### 10.1 Current Persistence Mechanism

The `IncrementalState::save()` method in `glyim-query/src/incremental.rs` persists two things:
1. **Source hashes** — serialized via `postcard` into `source-hashes.bin`
2. **Query context** — serialized via `PersistenceLayer::save()` into the same directory

This is sufficient for source-level granularity but not for item-level granularity. Phase 4 extends the persistence to include:

3. **Item semantic hashes** — `HashMap<String, Fingerprint>` mapping item names to their semantic hashes
4. **Per-item query results** — the cached `Arc<QueryResult>` for each item, serialized into separate files
5. **Merkle node references** — the `ContentHash` of each item's Merkle node in the artifact cache
6. **Test dependency graph** — serialized `TestDependencyGraph`
7. **Dependency graph edges** — serialized `DependencyGraph`

### 10.2 Persistence Directory Layout

```
.glyim/incremental/
├── source-hashes.bin          — source file fingerprints
├── item-hashes.bin            — per-item semantic hash fingerprints
├── dep-graph.bin              — serialized dependency graph
├── test-deps.bin              — serialized test dependency graph
├── query-results/
│   ├── <fingerprint_1>.bin    — cached query result for fingerprint 1
│   ├── <fingerprint_2>.bin    — cached query result for fingerprint 2
│   └── ...
└── merkle-refs/
    ├── <item_name>.bin        — Merkle node ContentHash for item
    └── ...
```

### 10.3 Garbage Collection

Over time, the `query-results/` and `merkle-refs/` directories accumulate entries from previous builds. Phase 4 implements a simple garbage collection strategy:

- On each build, compute the set of fingerprints that are still reachable from the current source hashes (by traversing the dependency graph).
- Delete any files in `query-results/` whose fingerprints are not reachable.
- Delete any Merkle nodes whose `ContentHash` is not referenced by any reachable fingerprint.

This GC runs at the end of each incremental build and is fast (linear in the number of cached artifacts).

---

## 11. Incremental Report & Diagnostics

### 11.1 IncrementalReport Structure

```rust
#[derive(Debug, Clone, Default)]
pub struct IncrementalReport {
    /// Total number of HIR items in the module.
    pub total_items: usize,
    /// Items that were recompiled (red).
    pub red_items: Vec<ItemReport>,
    /// Items that were reused from cache (green).
    pub green_items: Vec<String>,
    /// Number of query cache hits.
    pub cache_hits: usize,
    /// Number of query cache misses.
    pub cache_misses: usize,
    /// Per-stage timing information.
    pub stage_timings: Vec<(String, std::time::Duration)>,
    /// Total elapsed time for the incremental build.
    pub total_elapsed: std::time::Duration,
    /// Whether this was a full rebuild or an incremental build.
    pub was_full_rebuild: bool,
    /// Invalidation report from the query engine.
    pub invalidation: Option<glyim_query::InvalidationReport>,
}

#[derive(Debug, Clone)]
pub struct ItemReport {
    /// The item name (function name, struct name, etc.).
    pub name: String,
    /// Why the item was recompiled.
    pub reason: RedReason,
    /// Time spent recompiling this item.
    pub elapsed: std::time::Duration,
    /// Stages that were executed for this item.
    pub stages_executed: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RedReason {
    /// The item's source code changed.
    SourceChanged,
    /// A dependency of this item changed.
    DependencyChanged(String),
    /// The item was not in the cache (first build).
    NotCached,
    /// The invariant certificate changed (e-graph optimization).
    InvalidationCertificateChanged,
    /// The rule set version changed.
    RuleSetUpdated,
}
```

### 11.2 Human-Readable Output

The report is printed to stderr in a concise format:

```
Incremental build: 6 items (2 red, 4 green)
  RED add          — source changed           0.8ms (typeck, egraph, codegen)
  RED factorial    — dependency changed: add  1.2ms (typeck, mono, egraph, codegen)
  GREEN main       — cache hit
  GREEN fibonacci  — cache hit
  GREEN Vec.push   — cache hit
  GREEN Vec.new    — cache hit
Total: 3.4ms (vs. 28.1ms full build, 8.2x speedup)
```

### 11.3 Machine-Readable Output

For IDE integration, the report can also be emitted as JSON via the `--incremental-json` flag:

```json
{
  "total_items": 6,
  "red_count": 2,
  "green_count": 4,
  "total_elapsed_ms": 3.4,
  "full_build_ms": 28.1,
  "speedup": 8.2,
  "items": [
    {"name": "add", "status": "red", "reason": "source_changed", "elapsed_ms": 0.8},
    {"name": "factorial", "status": "red", "reason": "dependency_changed:add", "elapsed_ms": 1.2},
    {"name": "main", "status": "green", "reason": "cache_hit", "elapsed_ms": 0},
    ...
  ]
}
```

---

## 12. Error Handling & Consistency Guarantees

### 12.1 Incremental Consistency

The incremental pipeline must guarantee that the output of an incremental build is identical to the output of a full rebuild. This is ensured by:

1. **Fingerprint soundness:** The `Fingerprint` of a query input must change if and only if the input's semantics change. The `SemanticHash` provides this guarantee for HIR items.
2. **Dependency completeness:** The dependency graph must include all edges. A missing edge would cause the pipeline to reuse a stale cached result when a dependency changed. The conservative approach of over-approximating dependencies (e.g., treating all cross-module calls as dependencies) ensures completeness at the cost of occasional unnecessary recompilation.
3. **Merkle integrity:** The `MerkleNode` hash is computed from the serialized artifact data. If the data is corrupted, the hash will not match, and the artifact will be treated as a cache miss (triggering recompilation).

### 12.2 Error Recovery

If the incremental state becomes inconsistent (e.g., due to a process crash during a build), the pipeline must recover gracefully:

1. **Corrupted query results:** If deserialization of a cached query result fails, treat it as a cache miss and recompute.
2. **Corrupted Merkle nodes:** If the Merkle store returns corrupted data (hash mismatch), treat it as a cache miss.
3. **Missing dependency graph:** If the dependency graph cannot be loaded, fall back to a full rebuild and rebuild the graph from scratch.
4. **Version mismatch:** If the `IncrementalState` was created by a different version of the compiler, discard it and start fresh. The `CARGO_PKG_VERSION` constant is embedded in source fingerprints (via `compute_source_hash()`) to detect version changes.

### 12.3 Fallback to Full Build

The `QueryPipeline` must provide a fallback mechanism when incremental compilation is not possible or when it would be slower than a full build. The fallback triggers when:

- The `IncrementalState` does not exist (first build)
- More than 80% of items are red (incremental overhead exceeds benefit)
- The total incremental build time exceeds 90% of the estimated full build time
- The user passes `--no-incremental` or `--full-rebuild`

---

## 13. Testing Strategy

### 13.1 Unit Tests

| Test | Location | Description |
|------|----------|-------------|
| `query_fingerprint_soundness` | `glyim-compiler/src/queries.rs` | Verify that identical inputs produce identical fingerprints |
| `query_cache_hit` | `glyim-compiler/src/queries.rs` | Verify that unchanged inputs return cached results |
| `query_cache_invalidation` | `glyim-compiler/src/queries.rs` | Verify that changed inputs trigger recomputation |
| `item_level_diff` | `glyim-compiler/src/queries.rs` | Verify that changing one item only marks it red |
| `dep_graph_transitive` | `glyim-query/src/dep_graph.rs` | Verify transitive dependency computation |
| `merkle_artifact_roundtrip` | `glyim-merkle/src/store.rs` | Verify that artifacts survive serialization/deserialization |
| `fn_object_code_extraction` | `glyim-codegen-llvm/src/incremental.rs` | Verify per-function object code extraction |
| `fn_object_code_combine` | `glyim-codegen-llvm/src/incremental.rs` | Verify combining object code sections |
| `test_dep_graph_affected` | `glyim-testr/src/incremental.rs` | Verify affected test computation |
| `incremental_report_format` | `glyim-compiler/src/queries.rs` | Verify report formatting |

### 13.2 Integration Tests

| Test | Location | Description |
|------|----------|-------------|
| `incremental_build_no_change` | `glyim-cli-tests-full` | Build, then rebuild with no changes — second build should be near-instant |
| `incremental_build_comment_change` | `glyim-cli-tests-full` | Build, change a comment, rebuild — should skip all compilation |
| `incremental_build_fn_change` | `glyim-cli-tests-full` | Build, change one function, rebuild — only that function should recompile |
| `incremental_build_dep_change` | `glyim-cli-tests-full` | Build, change a called function, rebuild — caller should also recompile |
| `incremental_build_struct_change` | `glyim-cli-tests-full` | Build, change a struct field, rebuild — all functions using the struct should recompile |
| `incremental_test_subset` | `glyim-cli-tests-full` | Build, change one function, run tests — only tests depending on that function should run |
| `incremental_watch_debounce` | `glyim-cli-tests-full` | Verify watch command debounces rapid file changes |
| `incremental_corruption_recovery` | `glyim-cli-tests-full` | Corrupt the cache, rebuild — should fall back to full build |
| `incremental_version_mismatch` | `glyim-cli-tests-full` | Change compiler version, rebuild — should discard old cache |
| `incremental_object_reuse` | `glyim-cli-tests-full` | Build, change one function, rebuild — verify green functions' object code is reused |

### 13.3 Property Tests

| Property | Description |
|----------|-------------|
| `incremental_equals_full` | For any source change, incremental build output equals full rebuild output |
| `fingerprint_determinism` | The same input always produces the same fingerprint |
| `cache_coherence` | A cache hit always returns the correct result |
| `idempotent_invalidation` | Running invalidation twice produces the same red/green partition |

---

## 14. Implementation Timeline

### Phase 4A: Query-Driven Pipeline Foundation (7–9 days)

| Day | Task | Files |
|-----|------|-------|
| 1–2 | Create `queries.rs` with `QueryPipeline` struct and query function pattern | `glyim-compiler/src/queries.rs` |
| 3–4 | Implement `query_parse`, `query_declarations`, `query_lower` with caching | `glyim-compiler/src/queries.rs` |
| 5–6 | Implement `query_typeck` with per-function granularity | `glyim-compiler/src/queries.rs`, `glyim-typeck/src/typeck/mod.rs` |
| 7–8 | Implement `query_desugar` and `query_mono` | `glyim-compiler/src/queries.rs` |
| 9 | Integration test: query-driven pipeline produces same output as linear pipeline | `glyim-compiler/tests/` |

### Phase 4B: Per-Function Code Generation & Caching (6–8 days)

| Day | Task | Files |
|-----|------|-------|
| 1–2 | Refactor `Codegen::generate()` into `prepare_module()` + `codegen_fn()` + `finalize_module()` | `glyim-codegen-llvm/src/codegen/mod.rs` |
| 3–4 | Implement `IncrementalCodegen` with per-function object code extraction | `glyim-codegen-llvm/src/incremental.rs` |
| 5–6 | Implement `query_codegen` with Merkle store caching | `glyim-compiler/src/queries.rs` |
| 7 | Implement `query_optimize` with InvariantCertificate integration | `glyim-compiler/src/queries.rs` |
| 8 | Integration test: per-function codegen produces same output as whole-module codegen | `glyim-codegen-llvm/tests/` |

### Phase 4C: Dependency Graph & Invalidation (5–7 days)

| Day | Task | Files |
|-----|------|-------|
| 1–2 | Populate dependency graph during query execution | `glyim-compiler/src/queries.rs`, `glyim-query/src/dep_graph.rs` |
| 3–4 | Implement item-level diff using semantic hashes | `glyim-compiler/src/queries.rs`, `glyim-hir/src/semantic_hash.rs` |
| 5–6 | Implement incremental state persistence with item-level granularity | `glyim-query/src/incremental.rs`, `glyim-query/src/persistence.rs` |
| 7 | Integration test: changing one function only recompiles that function + dependents | `glyim-cli-tests-full/` |

### Phase 4D: CLI & Watch Command (4–5 days)

| Day | Task | Files |
|-----|------|-------|
| 1 | Wire `--incremental` flag to `QueryPipeline` in `cmd_build`, `cmd_check`, `cmd_run` | `glyim-cli/src/commands/` |
| 2–3 | Implement `glyim-watch` crate with `notify`-based file watcher | `crates/glyim-watch/` |
| 4 | Implement `cmd_watch.rs` and `IncrementalReport` diagnostics | `glyim-cli/src/commands/cmd_watch.rs`, `glyim-compiler/src/queries.rs` |
| 5 | Integration test: watch command detects changes and triggers incremental rebuild | `glyim-cli-tests-full/` |

### Phase 4E: Incremental Testing & Polish (3–5 days)

| Day | Task | Files |
|-----|------|-------|
| 1–2 | Rewrite `glyim-testr/src/incremental.rs` with real dependency-aware test selection | `glyim-testr/src/incremental.rs` |
| 3 | Implement garbage collection for stale cache entries | `glyim-query/src/incremental.rs` |
| 4 | Add `--incremental-status` and `--incremental-json` CLI flags | `glyim-cli/src/commands/` |
| 5 | Final integration tests and documentation | All crates |

### Total: 25–34 working days

---

## 15. Crate Dependency Changes

### 15.1 New Crate

| Crate | Tier | Dependencies | Description |
|-------|------|-------------|-------------|
| `glyim-watch` | 5 | `notify`, `glyim-compiler`, `glyim-query`, `glyim-merkle`, `colored` | File watcher and continuous compilation |

### 15.2 Modified Crates

| Crate | Changes |
|-------|---------|
| `glyim-compiler` | New `queries.rs` module; new `build_incremental()`, `check_incremental()`, `run_incremental()` entry points; new `IncrementalReport` type |
| `glyim-codegen-llvm` | Refactored `Codegen::generate()` into `prepare_module()` + `codegen_fn()` + `finalize_module()`; new `incremental.rs` module |
| `glyim-query` | Extended `IncrementalState` with item-level hashes, Merkle references, and test dependency graph; extended persistence |
| `glyim-testr` | Rewritten `incremental.rs` with real `TestDependencyGraph` |
| `glyim-cli` | Wired `--incremental` flag; new `cmd_watch.rs`; new `--incremental-status` and `--incremental-json` flags |

### 15.3 Workspace Cargo.toml Update

```toml
[workspace]
members = [
    # ... existing members ...
    "crates/glyim-watch",
]
```

### 15.4 Tier Assignment

```
Tier 1: glyim-interner, glyim-diag, glyim-syntax
Tier 2: glyim-lex, glyim-parse
Tier 3: glyim-hir, glyim-typeck, glyim-macro-core, glyim-macro-vfs, glyim-egraph
Tier 4: glyim-codegen-llvm
Tier 5: glyim-cli, glyim-cas-server, glyim-watch
```

`glyim-watch` is tier 5 because it depends on `glyim-compiler` (tier 5). No tier violations are introduced.

---

## 16. Risk Register

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| Query-driven pipeline produces different output than linear pipeline | Medium | Critical | Property test: `incremental_equals_full` for all integration tests |
| Per-function codegen produces different object code than whole-module codegen | Medium | High | Byte-level comparison test: compile same HIR both ways and diff output |
| Dependency graph misses edges, causing stale cache hits | Medium | Critical | Conservative over-approximation; property test: `incremental_equals_full` |
| Merkle store serialization/deserialization introduces data loss | Low | High | Round-trip tests for every artifact type; hash verification on load |
| File watcher misses events on certain platforms | Medium | Medium | Use `notify` crate's recommended debouncing pattern; fall back to polling if needed |
| `--incremental` flag breaks existing build workflows | Low | High | Feature-gated behind `query-pipeline` feature; opt-in via `--incremental` flag |
| LLVM module reuse across compilations causes state leakage | Medium | Critical | Create fresh `Codegen` struct for each build; verify module with `module.verify()` |
| Cache directory grows unbounded | Low | Low | Garbage collection on each build; LRU eviction for Merkle store |
| Performance regression for small projects (incremental overhead > full build) | Medium | Medium | Fallback to full build when >80% items are red; benchmark against linear pipeline |
| Type checker refactoring to per-function mode introduces bugs | Medium | High | Gradual migration; dual-mode testing (run both per-function and whole-module, compare results) |

---

## 17. Performance Targets

| Metric | Target | Measurement |
|--------|--------|-------------|
| Incremental build with no changes | < 5ms | Time from `glyim build --incremental` to completion when source is unchanged |
| Incremental build with 1 function changed (10-function module) | < 15ms | Time from file save to build completion |
| Incremental build with 1 function changed (100-function module) | < 25ms | Time from file save to build completion |
| Cache hit rate for typical edit-compile cycle | > 80% | Fraction of items that are green per build |
| Watch command latency (file change to diagnostic) | < 200ms | End-to-end time from file save to recompilation report |
| Memory overhead of incremental state | < 50MB | Size of `.glyim/incremental/` directory for a 1000-function project |
| Full build regression | < 10% | Full build time with `--incremental` must be within 10% of linear pipeline |

---

## 18. Migration Strategy

### 18.1 Feature Flag Approach

The query-driven pipeline is initially gated behind a `query-pipeline` feature flag in `glyim-compiler/Cargo.toml`:

```toml
[features]
default = []
query-pipeline = ["glyim-query", "glyim-merkle"]
```

The `--incremental` CLI flag activates this feature at runtime. When the feature is off, the existing linear pipeline is used without any changes. This allows the Phase 4 implementation to land incrementally without breaking any existing functionality.

### 18.2 Gradual Rollout

1. **Phase 4A–4C:** Feature flag is off by default. Development and testing happen behind the flag.
2. **Phase 4D:** Feature flag is activated by `--incremental` flag. Users can opt in.
3. **Phase 4E:** After integration tests pass and performance targets are met, the feature flag is removed and the query-driven pipeline becomes the default. The linear pipeline is retained as a fallback (`--no-incremental`).

### 18.3 Backward Compatibility

The `.glyim/incremental/` directory is created lazily on first incremental build. If the directory does not exist, the pipeline starts with an empty `IncrementalState`. If the directory exists but contains stale data (from a different compiler version), it is discarded and recreated. This ensures that users upgrading from Phase 3 to Phase 4 do not experience any issues.

---

## 19. Success Criteria

Phase 4 is complete when all of the following are true:

1. `glyim build --incremental` produces identical output to `glyim build` for all test cases
2. Changing a single function in a 100-function module triggers recompilation of only that function and its dependents
3. An incremental build with no changes completes in under 5ms
4. `glyim watch` detects file changes and triggers incremental recompilation within 200ms
5. `glyim test --incremental` runs only tests affected by the change
6. The `IncrementalReport` accurately reports red/green items, timing, and invalidation reasons
7. All property tests (`incremental_equals_full`, `fingerprint_determinism`, `cache_coherence`) pass
8. Full build time regression is under 10% compared to the linear pipeline
9. The `--incremental` flag is no longer a placeholder and is documented in `--help`
10. The `.glyim/incremental/` directory is properly garbage-collected after each build
