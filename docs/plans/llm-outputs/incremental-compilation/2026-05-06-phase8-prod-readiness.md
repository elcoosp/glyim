# Glyim Incremental Compiler — Phase 8 Implementation Plan

## Performance Hardening, Production Readiness & Release Stabilization

**Codebase:** elcoosp-glyim v0.5.0  
**Rust Workspace | 20+ Crates | LLVM 22.1 / Inkwell 0.9**  
**Date:** 2026-05-07

---

## 1. Executive Summary

Phase 8 is the capstone phase that transforms the Glyim incremental compiler from a feature-complete research platform into a production-ready, performance-hardened, and release-stable compiler toolchain. Phases 0 through 7 assembled a formidable set of capabilities — query-driven incremental pipeline with per-function artifact caching, Merkle IR trees with content-addressed storage, JIT micro-modules with OrcV2 hot-reload, e-graph middle-end with algebraic optimization, cross-module incremental linking with CAS-backed dependency sharing, test-aware compilation with integrated mutation testing, and LSP-based IDE integration with real-time semantic analysis — but these features were built incrementally across eight development phases, each with its own error paths, concurrency models, memory allocation patterns, and API surfaces. No systematic effort has been made to profile end-to-end compilation latency, optimize memory usage under large workspaces, verify concurrency safety across the LSP server and JIT execution engine, fuzz the parser and type checker for crash resilience, or benchmark the incremental pipeline against regression thresholds.

Phase 8 closes these gaps through four interconnected workstreams. **Performance hardening** profiles every pipeline stage from source load to binary output, optimizes the hot paths identified by profiling (semantic hash computation, Merkle store lookups, e-graph equality saturation, LLVM code generation), reduces memory allocation pressure in the query engine and analysis database, and introduces a benchmarking infrastructure that tracks compilation latency, memory usage, and incremental efficiency across commits. **Production hardening** fuzzes the parser, type checker, and HIR lowering pipeline with structure-aware fuzzers to eliminate crash bugs; verifies concurrency safety across the LSP server's `AnalysisDatabase` RwLock access patterns, the JIT's thread-safe dispatch tables, and the mutation test runner's parallel execution; and adds structured telemetry that reports compilation performance, cache hit rates, and error rates to the incremental state directory for post-hoc analysis.

**API stabilization** audits every public API across all 20+ crates, deprecates the legacy linear pipeline (replaced by the query-driven pipeline in Phase 4), unifies error types across the pipeline (the `PipelineError` enum still contains `ParseError[]` with `(usize, usize)` spans alongside `TypeError[]` with the same raw tuples), stabilizes the `glyim-diag::Span` with `FileId` introduced in Phase 7, and publishes a compatibility guarantee document that specifies which APIs are stable, which are experimental, and which are deprecated. **Release preparation** consolidates the feature flags introduced across all phases into a coherent configuration system, generates comprehensive rustdoc for all public APIs, creates a migration guide from v0.5.0 (batch compiler) to v1.0.0 (incremental platform), and produces the release artifacts (binary distributions, VS Code extension package, Neovim plugin, and Homebrew formula).

**Estimated effort:** 30–42 working days.

**Key deliverables:**
- End-to-end profiling infrastructure with per-stage latency, memory, and cache-hit tracking
- Optimized hot paths: semantic hashing (3x speedup target), Merkle store (2x lookup speedup), e-graph saturation (2x speedup for large modules)
- Memory usage reduction: 40% peak RSS reduction target for large workspace compilation
- Structure-aware fuzz targets for parser, type checker, and HIR lowering with continuous integration
- Concurrency safety audit and verification for LSP server, JIT engine, and mutation test runner
- `glyim-bench` crate with deterministic benchmark suite and regression detection
- Public API audit and stabilization across all crates with compatibility guarantees
- Legacy pipeline deprecation and migration path
- Unified error type system with `FileId`-aware `Span`s
- Comprehensive rustdoc generation for all public APIs
- v1.0.0 release artifacts (binary, VS Code extension, Neovim plugin, Homebrew formula)
- Migration guide from v0.5.0 batch compiler to v1.0.0 incremental platform

---

## 2. Current Codebase State Assessment

### 2.1 Performance Profile (As-Is)

No systematic profiling has been performed on the Glyim compiler. The only performance-related infrastructure is the `IncrementalReport` struct from Phase 4, which records per-stage timing (type check, e-graph, codegen) and cache hit/miss counts, but this is a coarse measurement that does not capture memory allocation patterns, query engine overhead, or the cost of Merkle store serialization and deserialization. The `GranularityMonitor` in `glyim-query/src/granularity.rs` tracks edit granularity for adaptive invalidation but does not measure latency.

| Pipeline Stage | Known Bottleneck | Measurement | Optimization Status |
|----------------|-----------------|-------------|---------------------|
| Macro expansion | Wasm instantiation per macro call | Not measured | None |
| Parsing | `parse()` is whole-file, single-pass | Not measured | None |
| Declaration scanning | `parse_declarations()` is header-only | Not measured | Adequate (fast path) |
| HIR lowering | Full AST traversal per build | Not measured | None |
| Type checking | `TypeChecker::check()` is O(n) per function | Not measured | None |
| Method desugaring | Full HIR traversal | Not measured | None |
| Monomorphization | `merge_mono_types()` clone-heavy | Not measured | None |
| E-graph optimization | Equality saturation can be exponential | Phase 3 report: can exceed 5s for large functions | Invariant certificates provide partial caching |
| LLVM code generation | Most expensive stage (dominates wall-clock time) | Phase 4 report: 60-80% of total compile time | Per-function caching (Phase 4) provides biggest win |
| Linking | `cc` invocation with single object file | Not measured | None |
| Merkle store put/get | SHA-256 computation + CAS blob storage | Not measured | None |
| Semantic hashing | SHA-256 over normalized HIR | Not measured | Incremental (per-item) but not optimized |
| Incremental state persistence | `postcard` serialization of query context | Not measured | None |
| LSP analysis | Full incremental pipeline per edit | Phase 7 target: sub-50ms | Not yet measured |

### 2.2 Memory Usage (As-Is)

No memory profiling has been performed. The following known memory allocation patterns are potential problems for large workspaces:

| Component | Allocation Pattern | Concern |
|-----------|-------------------|---------|
| `Interner` | Grows unbounded; `HashMap<String, Symbol>` + `Vec<String>` | Never shrinks; all symbols from all compilations persist |
| `IncrementalState` | `HashMap<Fingerprint, Arc<QueryResult>>` | All cached query results held in memory until GC |
| `MerkleStore` | CAS blob storage with sharded filesystem | In-memory index grows with artifact count |
| `AnalysisDatabase` (LSP) | `RwLock<HashMap<FileId, Hir>>` | Full HIR for every open file held in memory |
| `SymbolIndex` (LSP) | `HashMap<String, Vec<SymbolInfo>>` + `HashMap<(FileId, usize), SymbolInfo>` | Dual index doubles memory for symbol data |
| `ReferenceGraph` (LSP) | `HashMap<String, Vec<Reference>>` | Unbounded; every reference in every open file |
| `MutationEngine` | `Vec<Mutation>` for all possible mutations | Can be very large (hundreds of mutants per function) |
| `Coverage counters` | `Vec<u64>` indexed by instrumentation point ID | Grows with code size; not freed until dump |
| E-graph `EGraph` | `egg::EGraph` with union-find | Can grow exponentially during saturation |
| `CompiledHir` | Contains `Hir`, `Interner`, `expr_types`, `call_type_args` | Cloned frequently in incremental pipeline |

### 2.3 Concurrency Model (As-Is)

The codebase has multiple concurrency models that have never been audited for safety:

| Component | Concurrency Model | Safety Concern |
|-----------|-------------------|----------------|
| LSP server | `tokio` async runtime; `RwLock` on `AnalysisDatabase` | Writer starvation possible if many concurrent reads; `unwrap()` on lock acquisition |
| Analysis driver | Single background thread; `mpsc::unbounded_channel` | Unbounded channel can grow without limit if analysis is slower than edits |
| JIT execution | `OrcV2` thread-safe JIT; `DispatchTable` with `RwLock` | `unsafe extern "C" fn()` calls have no lifetime tracking |
| Mutation test runner | `Executor` spawns subprocess per test; `TestRunner` uses `tokio::JoinSet` | Shared `MutationScoreReport` written by multiple tasks |
| `MACRO_EXPANSION_TABLE` | `LazyLock<Mutex<Vec<MacroExpansion>>>` | Global mutable state; mutex contention under concurrent compilation |
| File watcher (`glyim-watch`) | `notify` crate callbacks on separate thread | Debouncing logic is not synchronized with pipeline execution |
| CAS server | `axum` async handlers on `tokio` | Shared `LocalContentStore` accessed without explicit synchronization |
| Incremental state | `QueryContext` is `&mut self` (single-threaded) | Not `Send`; cannot be shared across threads in workspace builds |

### 2.4 Error Handling and Crash Resilience (As-Is)

| Component | Error Handling | Crash Risk |
|-----------|---------------|------------|
| Parser | Returns `Vec<ParseError>` with byte-offset spans | `unwrap()` on token kind assumptions in some match arms |
| Type checker | Returns `Result<Vec<TypeError>, TypeError>` | Bails on first error in some paths; panics on impossible types |
| HIR lowering | `unwrap()` on `DeclTable` lookups | Can panic on malformed AST from macro expansion |
| Codegen | `Result<(), String>` with stringly-typed errors | LLVM API calls can abort the process on invalid IR |
| JIT | `unsafe` function pointer calls | Segfault on type mismatch or dangling symbol |
| Merkle store | `Result<Option<Vec<u8>>, String>` | Filesystem I/O errors not handled in all paths |
| LSP server | `tower_lsp::jsonrpc::Result` | Internal panics would crash the server process |
| Orchestrator | `Result<_, OrchestratorError>` | No retry logic for CAS unavailability |

### 2.5 API Stability (As-Is)

| Concern | Current State |
|---------|--------------|
| Feature flags | `query-pipeline` (Phase 4), `coverage` (Phase 6), `mutation` (Phase 6), `lsp` (Phase 7) — scattered across crates, no unified configuration |
| Error types | `PipelineError` aggregates `ParseError[]` (with `(usize, usize)` spans), `TypeError[]` (same), `Codegen(String)`, `Link(String)` — inconsistent with Phase 7 `Span` with `FileId` |
| `Span` migration | Phase 7 added `FileId` to `Span`, but `ParseError` and `TypeError` still use `(usize, usize)` |
| Legacy pipeline | `compile_source_to_hir()` still exists alongside `QueryPipeline` — no deprecation warning |
| Public API surface | No `#[doc(hidden)]` or `#[unstable]` annotations; all public items are implicitly stable |
| SemVer compliance | Workspace version is `0.5.0`; no compatibility guarantees documented |
| Deprecation policy | No `#[deprecated]` annotations on any API |

### 2.6 Critical Gaps That Phase 8 Addresses

| Gap | Impact | Affected Crate | Phase 8 Solution |
|-----|--------|---------------|-------------------|
| No profiling infrastructure | Cannot identify performance bottlenecks | All | New `glyim-bench` crate with `criterion`-based benchmarks |
| Semantic hashing is unoptimized | SHA-256 over full normalized HIR per item; O(n) in item size | `glyim-hir` | Incremental hashing with change detection |
| Memory not profiled | Large workspaces may exceed available RAM | All | Memory profiling with `jemalloc` + `heaptrack`; allocation reduction |
| No fuzz testing | Parser and type checker may crash on adversarial input | `glyim-parse`, `glyim-typeck` | `cargo-fuzz` targets with structure-aware fuzzers |
| Concurrency not audited | Data races, deadlocks, and writer starvation possible | `glyim-lsp`, `glyim-testr`, `glyim-codegen-llvm` | `loom`-based concurrency tests + audit |
| Feature flags scattered | No unified way to enable/disable phase features | `glyim-compiler`, `glyim-cli` | Consolidated `glyim-config` crate |
| Error types inconsistent | `Span` with `FileId` vs. `(usize, usize)` raw tuples | `glyim-parse`, `glyim-typeck`, `glyim-compiler` | Unified error types with `FileId`-aware `Span` |
| Legacy pipeline not deprecated | Users may use wrong pipeline | `glyim-compiler` | `#[deprecated]` + migration guide |
| No benchmark regression detection | Performance regressions are invisible | All | CI benchmark comparison with threshold alerts |
| No API stability guarantees | Breaking changes can happen silently | All | Stability tiers + compatibility document |
| No release artifacts | Users must build from source | `glyim-cli` | Binary releases, extension packages, Homebrew formula |

---

## 3. Architecture Design

### 3.1 Profiling Infrastructure

The profiling infrastructure is built into the `glyim-bench` crate, which provides deterministic, reproducible benchmarks that measure compilation latency, memory usage, and incremental efficiency. The crate uses `criterion` for statistical benchmarking and `jemalloc` for heap profiling.

```
┌─────────────────────────────────────────────────────┐
│                glyim-bench                            │
│  ┌─────────────────────────────────────────────────┐ │
│  │         Benchmark Runner                         │ │
│  │  criterion::Criterion + jemalloc + heaptrack     │ │
│  └───────────────────┬─────────────────────────────┘ │
│  ┌───────────────────▼─────────────────────────────┐ │
│  │         Benchmark Suite                          │ │
│  │  Full Build | Incremental Edit | JIT Compile     │ │
│  │  E-Graph Optimize | LSP Analysis | Mutation Test │ │
│  └───────────────────┬─────────────────────────────┘ │
│  ┌───────────────────▼─────────────────────────────┐ │
│  │         Fixture Repository                       │ │
│  │  Small (10 fn) | Medium (100 fn) | Large (1000) │ │
│  │  Workspace (5 pkg) | Stress (10K fn)             │ │
│  └───────────────────┬─────────────────────────────┘ │
│  ┌───────────────────▼─────────────────────────────┐ │
│  │         Regression Detector                      │ │
│  │  Compare against baseline; alert on >10% change  │ │
│  └─────────────────────────────────────────────────┘ │
└─────────────────────────────────────────────────────┘
```

### 3.2 Benchmark Fixture Design

The benchmark suite uses deterministic fixtures that exercise specific pipeline stages:

| Fixture | Description | Size | Purpose |
|---------|-------------|------|---------|
| `small_single` | Single file, 10 functions, no generics | ~200 LOC | Baseline latency measurement |
| `medium_single` | Single file, 100 functions with generics | ~2K LOC | Scaling behavior |
| `large_single` | Single file, 1000 functions with deep call chains | ~20K LOC | Memory pressure, incremental efficiency |
| `workspace_5pkg` | 5 packages with cross-module dependencies | ~10K LOC total | Orchestrator performance, CAS throughput |
| `stress_10k` | Single file, 10,000 trivial functions | ~50K LOC | Parser/lexer throughput, symbol table scaling |
| `egraph_heavy` | Functions with complex arithmetic patterns | ~500 LOC | E-graph saturation time and memory |
| `mutation_suite` | 50 functions with comprehensive test coverage | ~5K LOC | Mutation testing throughput |
| `lsp_edit` | Simulated LSP edit sequence (100 edits) | ~2K LOC | LSP analysis latency, database update cost |
| `jit_hot_reload` | 20 JIT compilations with incremental changes | ~1K LOC | JIT tier-up timing, dispatch table update cost |

Each fixture is generated programmatically (not hand-written) to ensure reproducibility and to allow parameterized scaling. The fixture generator produces valid Glyim source code that exercises specific language features: function definitions, struct definitions, enum definitions, generic functions, method calls, match expressions, and cross-module imports.

### 3.3 Per-Stage Profiling

The `ProfileCollector` is a lightweight struct that records timing, allocation counts, and cache hit/miss rates for each pipeline stage. It is injected into the `QueryPipeline` and the `PackageGraphOrchestrator` to provide fine-grained profiling without modifying the pipeline logic.

```rust
// crates/glyim-bench/src/profile.rs

use std::collections::HashMap;
use std::time::{Duration, Instant};

/// A collected profile for a single compilation.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CompilationProfile {
    /// Unique identifier for this profiling session.
    pub id: u64,
    /// Timestamp of the compilation start.
    pub started_at: chrono::DateTime<chrono::Utc>,
    /// Total wall-clock time.
    pub total_duration: Duration,
    /// Per-stage timing and metrics.
    pub stages: HashMap<StageName, StageProfile>,
    /// Memory metrics.
    pub memory: MemoryProfile,
    /// Incremental metrics.
    pub incremental: IncrementalProfile,
}

#[derive(Debug, Clone, Hash, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum StageName {
    MacroExpand,
    Parse,
    Declarations,
    Lower,
    TypeCheck,
    Desugar,
    Monomorphize,
    EGraphOptimize,
    Codegen,
    Link,
    MerkleStore,
    SemanticHash,
    IncrementalStatePersist,
    LspAnalysis,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct StageProfile {
    /// Wall-clock time for this stage.
    pub duration: Duration,
    /// Number of items processed.
    pub items_processed: usize,
    /// Number of cache hits (for query-driven stages).
    pub cache_hits: usize,
    /// Number of cache misses.
    pub cache_misses: usize,
    /// Approximate bytes allocated during this stage.
    pub bytes_allocated: usize,
    /// Whether this stage was skipped (all items green).
    pub skipped: bool,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct MemoryProfile {
    /// Peak RSS (resident set size) in bytes.
    pub peak_rss: usize,
    /// Total bytes allocated (from allocator).
    pub total_allocated: usize,
    /// Bytes freed (from allocator).
    pub total_freed: usize,
    /// Number of allocation calls.
    pub allocation_count: usize,
    /// Number of deallocation calls.
    pub deallocation_count: usize,
    /// Lifetime of the longest-lived allocation.
    pub max_allocation_lifetime: Duration,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct IncrementalProfile {
    /// Number of items that were recompiled (red).
    pub red_items: usize,
    /// Number of items reused from cache (green).
    pub green_items: usize,
    /// Total number of items.
    pub total_items: usize,
    /// Cache hit ratio (green / total).
    pub cache_hit_ratio: f64,
    /// Time saved by incremental compilation.
    pub time_saved: Duration,
    /// Estimated full-build time (extrapolated).
    pub estimated_full_build_time: Duration,
}
```

The `ProfileCollector` is a thread-local struct that uses `Instant::now()` for timing and (when `jemalloc` is enabled) `jemalloc_ctl::stats::allocated` for memory tracking. It is zero-cost when profiling is disabled (all methods are inlined to no-ops behind a `cfg` flag).

```rust
// crates/glyim-bench/src/collector.rs

use crate::profile::{CompilationProfile, StageName, StageProfile};
use std::cell::RefCell;
use std::collections::HashMap;
use std::time::Instant;

thread_local! {
    static COLLECTOR: RefCell<ProfileCollector> = RefCell::new(ProfileCollector::new());
}

pub struct ProfileCollector {
    profile: CompilationProfile,
    stage_starts: HashMap<StageName, Instant>,
    enabled: bool,
}

impl ProfileCollector {
    /// Begin profiling a stage. No-op when profiling is disabled.
    #[inline]
    pub fn enter_stage(&mut self, stage: StageName) {
        if !self.enabled { return; }
        self.stage_starts.insert(stage, Instant::now());
    }

    /// End profiling a stage and record the metrics.
    #[inline]
    pub fn exit_stage(&mut self, stage: StageName, items: usize, hits: usize, misses: usize) {
        if !self.enabled { return; }
        if let Some(start) = self.stage_starts.remove(&stage) {
            let duration = start.elapsed();
            self.profile.stages.insert(stage, StageProfile {
                duration,
                items_processed: items,
                cache_hits: hits,
                cache_misses: misses,
                bytes_allocated: 0, // filled by allocator hook
                skipped: false,
            });
        }
    }

    /// Mark a stage as skipped (all items were green).
    pub fn skip_stage(&mut self, stage: StageName) {
        if !self.enabled { return; }
        self.profile.stages.insert(stage, StageProfile {
            duration: Duration::ZERO,
            items_processed: 0,
            cache_hits: 0,
            cache_misses: 0,
            bytes_allocated: 0,
            skipped: true,
        });
    }

    /// Get the collected profile.
    pub fn finish(self) -> CompilationProfile {
        self.profile
    }
}
```

### 3.4 Semantic Hash Optimization

The current `semantic_hash_item()` function in `glyim-hir/src/semantic_hash.rs` computes a SHA-256 hash over the normalized HIR for each item on every compilation. This is O(n) in the size of the item, and for large functions with hundreds of expressions, this can be a significant fraction of the incremental pipeline's time budget. Phase 8 optimizes this through three techniques:

1. **Incremental hashing**: Instead of hashing the entire normalized HIR, compute the hash incrementally by tracking which sub-expressions changed. When a function body changes, only the hashes of the changed sub-expressions and their ancestors in the expression tree need to be recomputed. This reduces the hashing cost from O(|HIR|) to O(|changed| * depth).

2. **Hash caching at expression level**: Cache the hash of each `HirExpr` node in the `Hir` data structure. When a sub-expression is unchanged (determined by comparing its byte range in the source against the edit diff), its cached hash is reused. This is similar to how the Merkle tree caches intermediate hashes.

3. **Blake3 instead of SHA-256**: Replace SHA-256 with Blake3 for semantic hashing. Blake3 is approximately 4x faster than SHA-256 on modern hardware while providing equivalent collision resistance for non-cryptographic purposes. Since semantic hashes are used for cache keys (not for security), the weaker pre-image resistance of Blake3 is acceptable.

```rust
// crates/glyim-hir/src/semantic_hash.rs (optimized)

use blake3::Hasher;

/// Compute the semantic hash of a HIR item, with optional caching.
pub fn semantic_hash_item_optimized(
    item: &HirItem,
    interner: &Interner,
    cache: &mut ExprHashCache,
) -> ContentHash {
    match item {
        HirItem::Fn(f) => {
            // Check if the function's source range is unchanged
            if let Some(cached) = cache.get_fn_hash(f.name) {
                return cached;
            }

            let mut hasher = Hasher::new();
            hasher.update(b"fn:");
            hasher.update(interner.resolve(f.name).as_bytes());

            // Hash the body with expression-level caching
            hash_expr_cached(&f.body, interner, cache, &mut hasher);

            let hash = hasher.finalize();
            let content_hash = ContentHash::from_bytes(hash.as_bytes());
            cache.insert_fn_hash(f.name, content_hash);
            content_hash
        }
        // ... other item types ...
    }
}

/// Hash an expression, using cached sub-expression hashes where possible.
fn hash_expr_cached(
    expr: &HirExpr,
    interner: &Interner,
    cache: &mut ExprHashCache,
    hasher: &mut Hasher,
) {
    if let Some(cached) = cache.get_expr_hash(expr.id()) {
        hasher.update(cached.as_bytes());
        return;
    }

    // Recurse into children, then hash the result
    let mut child_hasher = Hasher::new();
    match expr {
        HirExpr::Binary(op, lhs, rhs, _) => {
            child_hasher.update(b"bin:");
            child_hasher.update(&[*op as u8]);
            hash_expr_cached(lhs, interner, cache, &mut child_hasher);
            hash_expr_cached(rhs, interner, cache, &mut child_hasher);
        }
        HirExpr::Call(name, args, _) => {
            child_hasher.update(b"call:");
            child_hasher.update(interner.resolve(*name).as_bytes());
            for arg in args {
                hash_expr_cached(arg, interner, cache, &mut child_hasher);
            }
        }
        // ... other expression variants ...
    }

    let hash = child_hasher.finalize();
    cache.insert_expr_hash(expr.id(), ContentHash::from_bytes(hash.as_bytes()));
    hasher.update(hash.as_bytes());
}
```

The `ExprHashCache` is populated during the first compilation and reused across incremental compilations. When a source file changes, the cache is invalidated for expressions whose byte ranges overlap with the edit, and their hashes are recomputed. Expressions outside the edit range retain their cached hashes.

### 3.5 Merkle Store Optimization

The current `MerkleStore` in `glyim-merkle/src/store.rs` stores all artifacts in the CAS and retrieves them via SHA-256 content hashes. Each `put()` call computes a SHA-256 hash, serializes the data, and writes it to the sharded filesystem. Each `get()` call reads from the filesystem, deserializes, and returns the data. For the incremental pipeline, which may perform hundreds of Merkle store operations per compilation (one per query result per changed item), the I/O overhead can be significant.

Phase 8 optimizes the Merkle store through two techniques:

1. **In-memory LRU cache**: Add an in-memory LRU cache in front of the CAS. The cache holds the most recently accessed artifacts (default: 256 entries, configurable). Cache hits bypass filesystem I/O entirely, reducing lookup latency from disk access times (~1ms) to hash map lookups (~100ns). The cache is bounded in size and evicts the least-recently-used entries when full.

2. **Write batching**: Instead of writing each artifact to the CAS immediately, buffer writes in memory and flush them in a batch at the end of the compilation. This reduces filesystem syscall overhead (multiple `write()` calls become a single batched write) and allows the OS to optimize I/O scheduling.

```rust
// crates/glyim-merkle/src/store.rs (optimized)

use lru::LruCache;
use std::num::NonZeroUsize;
use std::sync::Mutex;

pub struct MerkleStore {
    /// Backing CAS storage.
    cas: Arc<dyn ContentStore>,
    /// In-memory LRU cache for frequently accessed artifacts.
    cache: Mutex<LruCache<ContentHash, Vec<u8>>>,
    /// Write buffer for batching CAS writes.
    write_buffer: Mutex<Vec<(ContentHash, Vec<u8>)>>,
    /// Maximum cache size (number of entries).
    cache_capacity: NonZeroUsize,
    /// Maximum write buffer size before flushing.
    write_buffer_capacity: usize,
}

impl MerkleStore {
    pub fn new(cas: Arc<dyn ContentStore>, cache_capacity: usize) -> Self {
        Self {
            cas,
            cache: Mutex::new(LruCache::new(
                NonZeroUsize::new(cache_capacity).unwrap_or(NonZeroUsize::new(256).unwrap())
            )),
            write_buffer: Mutex::new(Vec::new()),
            cache_capacity: NonZeroUsize::new(cache_capacity.unwrap_or(256)).unwrap(),
            write_buffer_capacity: 64, // flush after 64 buffered writes
        }
    }

    /// Retrieve an artifact, checking the in-memory cache first.
    pub fn get(&self, hash: &ContentHash) -> Option<Vec<u8>> {
        // Check LRU cache
        if let Some(cached) = self.cache.lock().unwrap().get(hash) {
            return Some(cached.clone());
        }

        // Fall back to CAS
        let data = self.cas.retrieve(hash)?;
        
        // Populate cache
        self.cache.lock().unwrap().put(*hash, data.clone());
        
        Some(data)
    }

    /// Store an artifact, buffering the write for batch flushing.
    pub fn put(&self, data: &[u8]) -> ContentHash {
        let hash = ContentHash::compute(data);
        
        // Buffer the write
        let mut buffer = self.write_buffer.lock().unwrap();
        buffer.push((hash, data.to_vec()));
        
        // Flush if buffer is full
        if buffer.len() >= self.write_buffer_capacity {
            self.flush_buffer(&mut buffer);
        }

        // Also populate the cache
        self.cache.lock().unwrap().put(hash, data.to_vec());

        hash
    }

    /// Flush all buffered writes to the CAS.
    fn flush_buffer(&self, buffer: &mut Vec<(ContentHash, Vec<u8>)>) {
        for (hash, data) in buffer.drain(..) {
            let _ = self.cas.store(&data);
        }
    }

    /// Flush any remaining buffered writes. Must be called at the end of compilation.
    pub fn flush(&self) {
        let mut buffer = self.write_buffer.lock().unwrap();
        self.flush_buffer(&mut buffer);
    }
}
```

### 3.6 Memory Optimization Strategy

The memory optimization strategy targets the three largest memory consumers identified by profiling:

1. **Interner deduplication**: The `Interner` in `glyim-interner` stores every unique string encountered during compilation. For large workspaces, this can grow to hundreds of thousands of entries. Phase 8 adds a `Interner::compact()` method that removes interned strings that are no longer referenced by any HIR, using a reference counting scheme. After each incremental compilation, the interner is compacted by walking the current HIR, incrementing reference counts for all encountered symbols, and then removing symbols with zero references.

2. **AnalysisDatabase eviction**: The LSP server's `AnalysisDatabase` holds full HIR data for every open file. For users with many open files, this can consume significant memory. Phase 8 adds an LRU eviction policy to the `hirs`, `csts`, and `source_maps` maps: files that have not been accessed in the last N analysis cycles are evicted from memory, and their data is reloaded from the incremental state on the next access.

3. **E-graph memory budget**: The `egg::EGraph` can consume unbounded memory during equality saturation, especially for functions with complex arithmetic patterns where many equivalent representations exist. Phase 8 adds a memory budget to the e-graph optimizer: if the e-graph exceeds a configurable memory limit (default: 100MB per function), saturation is stopped early, and the best extraction result found so far is used. This prevents the compiler from running out of memory on adversarial or pathological inputs.

```rust
// crates/glyim-compiler/src/egraph_optimizer.rs (extended)

/// Configuration for the e-graph optimizer, extended with memory budget.
pub struct EGraphConfig {
    /// Maximum number of iterations for equality saturation.
    pub max_iterations: usize,
    /// Maximum number of nodes in the e-graph before early termination.
    pub max_nodes: usize,
    /// Maximum memory (in bytes) the e-graph may consume.
    pub memory_budget: usize,
    /// Whether to use the invariant certificate for caching.
    pub use_invariant_certificates: bool,
}

impl Default for EGraphConfig {
    fn default() -> Self {
        Self {
            max_iterations: 50,
            max_nodes: 100_000,
            memory_budget: 100 * 1024 * 1024, // 100 MB
            use_invariant_certificates: true,
        }
    }
}
```

---

## 4. New Crate: `glyim-bench`

### 4.1 Crate Structure

```
crates/glyim-bench/
├── Cargo.toml
└── src/
    ├── lib.rs              — public API, re-exports
    ├── profile.rs          — CompilationProfile, StageProfile, MemoryProfile
    ├── collector.rs        — ProfileCollector (thread-local profiling)
    ├── fixtures.rs         — Fixture generation (programmatic)
    ├── runner.rs           — Benchmark runner (criterion integration)
    ├── regression.rs       — Regression detection against baselines
    ├── memory.rs           — Memory profiling utilities (jemalloc hooks)
    ├── report.rs           — Report generation (JSON, HTML, terminal)
    └── benches/
        ├── bench_full_build.rs
        ├── bench_incremental.rs
        ├── bench_jit.rs
        ├── bench_egraph.rs
        ├── bench_lsp.rs
        ├── bench_mutation.rs
        └── bench_orchestrator.rs
```

### 4.2 Cargo.toml

```toml
[package]
name = "glyim-bench"
version.workspace = true
edition.workspace = true
license.workspace = true
rust-version.workspace = true
description = "Benchmarking and profiling infrastructure for Glyim"

[dependencies]
glyim-compiler = { path = "../glyim-compiler" }
glyim-hir = { path = "../glyim-hir" }
glyim-interner = { path = "../glyim-interner" }
glyim-parse = { path = "../glyim-parse" }
glyim-typeck = { path = "../glyim-typeck" }
glyim-codegen-llvm = { path = "../glyim-codegen-llvm" }
glyim-query = { path = "../glyim-query" }
glyim-merkle = { path = "../glyim-merkle" }
glyim-orchestrator = { path = "../glyim-orchestrator" }
glyim-mutant = { path = "../glyim-mutant" }
glyim-lsp = { path = "../glyim-lsp" }
criterion = { version = "0.5", features = ["html_reports"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
chrono = { version = "0.4", features = ["serde"] }
blake3 = "1"

[target.'cfg(target_os = "linux")'.dependencies]
jemalloc-ctl = "0.5"
jemallocator = "0.5"

[dev-dependencies]
tempfile = "3"

[[bench]]
name = "bench_full_build"
harness = false

[[bench]]
name = "bench_incremental"
harness = false

[[bench]]
name = "bench_egraph"
harness = false
```

### 4.3 Benchmark Runner

The benchmark runner uses `criterion` for statistical rigor and adds custom measurements for incremental efficiency and memory usage:

```rust
// crates/glyim-bench/src/runner.rs

use criterion::{Criterion, BenchmarkId, Throughput};
use crate::fixtures::FixtureGenerator;
use crate::profile::CompilationProfile;

pub fn bench_full_build(c: &mut Criterion) {
    let mut group = c.benchmark_group("full_build");
    
    for size in &[10, 100, 500, 1000] {
        let fixture = FixtureGenerator::single_file(*size);
        
        group.throughput(Throughput::Elements(*size as u64));
        group.bench_with_input(
            BenchmarkId::new("functions", size),
            &fixture,
            |b, fixture| {
                b.iter(|| {
                    let pipeline = QueryPipeline::new(
                        &tempdir(),
                        PipelineConfig::default(),
                    );
                    pipeline.compile(&fixture.source, &fixture.path)
                })
            },
        );
    }
    
    group.finish();
}

pub fn bench_incremental_edit(c: &mut Criterion) {
    let mut group = c.benchmark_group("incremental_edit");
    
    let fixture = FixtureGenerator::single_file(100);
    let pipeline = QueryPipeline::new(&tempdir(), PipelineConfig::default());
    
    // Warm up: full build
    pipeline.compile(&fixture.source, &fixture.path).unwrap();
    
    // Benchmark: single-function edit
    group.bench_function("single_fn_edit", |b| {
        b.iter(|| {
            let edited = fixture.edit_function("fn_42", "fn fn_42() -> i32 { 99 }");
            pipeline.compile_incremental(&edited, &fixture.path)
        })
    });
    
    // Benchmark: multi-function edit
    group.bench_function("multi_fn_edit", |b| {
        b.iter(|| {
            let edited = fixture.edit_functions(&[
                ("fn_42", "fn fn_42() -> i32 { 99 }"),
                ("fn_43", "fn fn_43() -> i32 { 88 }"),
            ]);
            pipeline.compile_incremental(&edited, &fixture.path)
        })
    });
    
    group.finish();
}
```

### 4.4 Regression Detection

The regression detector compares benchmark results against a baseline and reports any performance regressions exceeding a configurable threshold:

```rust
// crates/glyim-bench/src/regression.rs

use crate::profile::CompilationProfile;
use std::collections::HashMap;

/// A regression detector that compares profiles against a baseline.
pub struct RegressionDetector {
    /// The baseline profile to compare against.
    baseline: CompilationProfile,
    /// Maximum allowed regression ratio (e.g., 1.10 = 10% slower is acceptable).
    threshold: f64,
    /// Detected regressions.
    regressions: Vec<Regression>,
}

#[derive(Debug, Clone)]
pub struct Regression {
    pub stage: String,
    pub baseline_duration: std::time::Duration,
    pub current_duration: std::time::Duration,
    pub regression_ratio: f64,
    pub severity: RegressionSeverity,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegressionSeverity {
    /// Within threshold (no action needed).
    Acceptable,
    /// Exceeds threshold by up to 50%.
    Warning,
    /// Exceeds threshold by more than 50%.
    Critical,
}

impl RegressionDetector {
    pub fn new(baseline: CompilationProfile, threshold: f64) -> Self {
        Self {
            baseline,
            threshold,
            regressions: Vec::new(),
        }
    }

    /// Compare a current profile against the baseline.
    pub fn compare(&mut self, current: &CompilationProfile) -> &[Regression] {
        self.regressions.clear();

        for (stage_name, current_stage) in &current.stages {
            if let Some(baseline_stage) = self.baseline.stages.get(stage_name) {
                let ratio = current_stage.duration.as_secs_f64()
                    / baseline_stage.duration.as_secs_f64();
                
                if ratio > self.threshold {
                    let severity = if ratio > self.threshold * 1.5 {
                        RegressionSeverity::Critical
                    } else {
                        RegressionSeverity::Warning
                    };
                    
                    self.regressions.push(Regression {
                        stage: format!("{:?}", stage_name),
                        baseline_duration: baseline_stage.duration,
                        current_duration: current_stage.duration,
                        regression_ratio: ratio,
                        severity,
                    });
                }
            }
        }

        &self.regressions
    }
}
```

---

## 5. Fuzz Testing

### 5.1 Fuzz Target: Parser

The parser fuzz target generates random byte sequences and attempts to parse them. The goal is to ensure that the parser never panics, never enters an infinite loop, and always produces a valid `ParseOutput` (even if the output contains errors).

```rust
// fuzz/fuzz_targets/fuzz_parser.rs

#![no_main]
use libfuzzer_sys::fuzz_target;
use glyim_parse::parse;

fuzz_target!(|data: &[u8]| {
    // Attempt to parse random bytes as Glyim source
    if let Ok(source) = std::str::from_utf8(data) {
        let _ = parse(source);
        // Success: parser did not panic
    }
});
```

### 5.2 Fuzz Target: Type Checker

The type checker fuzz target generates well-formed HIR structures (valid syntax but potentially invalid types) and attempts to type-check them. This requires a structure-aware fuzzer that generates `Hir` values rather than raw bytes.

```rust
// fuzz/fuzz_targets/fuzz_typeck.rs

#![no_main]
use libfuzzer_sys::fuzz_target;
use glyim_hir::{Hir, HirItem, HirFn, HirExpr, HirType};
use glyim_typeck::TypeChecker;
use glyim_interner::Interner;

fuzz_target!(|hir: ArbitraryHir| {
    let mut interner = Interner::new();
    let mut typeck = TypeChecker::new();
    // Type-check the arbitrary HIR; should not panic
    let _ = typeck.check(&hir.0, &interner);
});

/// Structure-aware arbitrary HIR generator.
#[derive(Debug, arbitrary::Arbitrary)]
struct ArbitraryHir(Hir);
// Custom Arbitrary implementation that generates valid-ish HIR structures
// with controlled randomness (e.g., valid expression trees, but potentially
// mismatched types)
```

### 5.3 Fuzz Target: HIR Lowering

The HIR lowering fuzz target generates well-formed AST structures and attempts to lower them to HIR. This tests the `lower_with_declarations()` function's robustness against edge cases in the AST:

```rust
// fuzz/fuzz_targets/fuzz_lower.rs

#![no_main]
use libfuzzer_sys::fuzz_target;
use glyim_parse::parse;
use glyim_hir::lower_with_declarations;
use glyim_interner::Interner;

fuzz_target!(|data: &[u8]| {
    if let Ok(source) = std::str::from_utf8(data) {
        if let Ok(parse_output) = parse(source) {
            let decl_table = glyim_parse::declarations::parse_declarations(&parse_output.ast);
            let _ = lower_with_declarations(
                &parse_output.ast,
                &decl_table,
                &parse_output.interner,
            );
            // Success: lowering did not panic
        }
    }
});
```

### 5.4 Fuzz Integration with CI

The fuzz targets are integrated into the CI pipeline as a separate job that runs for 10 minutes per target. The corpus is stored in the repository under `fuzz/corpus/` and is committed as part of the codebase. Any new crash discovered by the fuzzer is treated as a P0 bug that must be fixed before merging.

---

## 6. Concurrency Safety Audit

### 6.1 LSP Server Concurrency

The LSP server has the most complex concurrency model in the codebase. The `AnalysisDatabase` uses `RwLock` for each field, and the `AnalysisDriver` writes to it from a background thread while LSP request handlers read from it. Phase 8 audits this for the following issues:

1. **Writer starvation**: If many concurrent read requests arrive (e.g., the editor requests completion, hover, and document symbols simultaneously), the analysis driver may never acquire a write lock. Phase 8 replaces `RwLock` with `parking_lot::RwLock`, which is fair and prevents writer starvation, and adds a maximum read hold time after which readers are forced to yield.

2. **Unbounded channel**: The `mpsc::unbounded_channel` between the LSP handler and the analysis driver can grow without limit if the editor sends changes faster than the analysis can process them. Phase 8 replaces this with a bounded channel (capacity 16) and applies backpressure: if the channel is full, incoming `didChange` notifications are coalesced (only the latest version is kept).

3. **Poisoned lock recovery**: If the analysis driver panics while holding a write lock, the `RwLock` becomes poisoned, and all subsequent reads will fail. Phase 8 wraps all lock acquisitions in `lock().unwrap_or_else(|e| e.into_inner())` to recover from poisoned locks, logging the panic but allowing the server to continue operating.

```rust
// crates/glyim-lsp/src/driver.rs (concurrency-hardened)

use tokio::sync::mpsc;
use std::sync::Arc;

/// Message from the LSP handler to the analysis driver.
/// Uses bounded channel for backpressure.
pub enum AnalysisMessage {
    FileChanged { path: PathBuf, content: String, version: i32 },
    FileClosed { path: PathBuf },
    FullReanalysis,
    Shutdown,
}

/// The analysis driver, hardened for production use.
pub struct AnalysisDriver {
    db: Arc<AnalysisDatabase>,
    pipeline: QueryPipeline,
    open_files: HashMap<PathBuf, FileId>,
    source_maps: HashMap<FileId, SourceMap>,
    /// Bounded channel: at most 16 pending messages.
    rx: mpsc::Receiver<AnalysisMessage>,  // bounded
    next_file_id: u32,
    /// Maximum time to hold a write lock (prevents writer starvation).
    max_write_lock_duration: std::time::Duration,
}

impl AnalysisDriver {
    pub fn new(
        db: Arc<AnalysisDatabase>,
        pipeline: QueryPipeline,
        rx: mpsc::Receiver<AnalysisMessage>,
    ) -> Self {
        Self {
            db,
            pipeline,
            open_files: HashMap::new(),
            source_maps: HashMap::new(),
            rx,
            next_file_id: 0,
            max_write_lock_duration: std::time::Duration::from_millis(100),
        }
    }

    /// Run the analysis loop with backpressure.
    pub async fn run(&mut self) {
        while let Some(msg) = self.rx.recv().await {
            match msg {
                AnalysisMessage::FileChanged { path, content, version } => {
                    self.analyze_file(&path, &content);
                }
                AnalysisMessage::FileClosed { path } => {
                    self.close_file(&path);
                }
                AnalysisMessage::FullReanalysis => {
                    self.reanalyze_all();
                }
                AnalysisMessage::Shutdown => {
                    break;
                }
            }

            // Drain and coalesce any pending file changes
            while let Ok(msg) = self.rx.try_recv() {
                match msg {
                    AnalysisMessage::FileChanged { path, content, version } => {
                        // Only process the latest version of each file
                        self.analyze_file(&path, &content);
                    }
                    AnalysisMessage::Shutdown => break,
                    _ => {} // coalesce other messages
                }
            }
        }
    }
}
```

### 6.2 Loom-Based Concurrency Tests

For the most critical concurrency scenarios (LSP database access, JIT dispatch table updates, mutation test result aggregation), Phase 8 introduces `loom`-based concurrency tests that systematically explore all possible thread interleavings:

```rust
// crates/glyim-lsp/tests/concurrency_tests.rs

#[cfg(test)]
mod loom_tests {
    use loom::sync::RwLock;
    use loom::thread;

    #[test]
    fn analysis_database_read_write_no_deadlock() {
        loom::model(|| {
            let db = Arc::new(AnalysisDatabase::new_for_loom());
            let db_clone = db.clone();

            // Writer thread (analysis driver)
            let writer = thread::spawn(move || {
                let mut index = db_clone.symbol_index.write().unwrap();
                index.clear_file(FileId(0));
            });

            // Reader thread (LSP handler)
            let reader = thread::spawn(move || {
                let index = db.symbol_index.read().unwrap();
                let _ = index.lookup_by_name("test");
            });

            writer.join().unwrap();
            reader.join().unwrap();
        });
    }
}
```

### 6.3 JIT Crash Safety

The JIT execution engine uses `unsafe` function pointer calls that can cause segfaults if the called function's signature does not match the expected type. Phase 8 adds a crash safety layer that wraps JIT calls in a `sigaction` handler (on Unix) or a structured exception handler (on Windows):

```rust
// crates/glyim-compiler/src/pipeline.rs (crash-hardened JIT)

/// Execute a function via JIT with crash protection.
pub fn run_jit_safe(
    source: &str,
    function_name: &str,
    timeout: Option<std::time::Duration>,
) -> Result<JitExecutionResult, JitError> {
    #[cfg(unix)]
    {
        // Set up signal handler for SIGSEGV, SIGBUS, SIGFPE
        let old_handler = unsafe {
            libc::sigaction(
                libc::SIGSEGV,
                &libc::sigaction {
                    sa_sigaction: jit_crash_handler,
                    sa_flags: libc::SA_SIGINFO,
                    sa_mask: std::mem::zeroed(),
                },
                std::ptr::null_mut(),
            )
        };

        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            run_jit(source, function_name, timeout)
        }));

        // Restore old signal handler
        unsafe {
            libc::sigaction(libc::SIGSEGV, &old_handler, std::ptr::null_mut());
        }

        result.map_err(|_| JitError::Crash(function_name.to_string()))?
    }

    #[cfg(not(unix))]
    {
        // Use structured exception handling on Windows
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            run_jit(source, function_name, timeout)
        }))
        .map_err(|_| JitError::Crash(function_name.to_string()))?
    }
}
```

---

## 7. API Stabilization & Deprecation

### 7.1 Stability Tiers

Phase 8 introduces a three-tier stability system for all public APIs:

| Tier | Annotation | Compatibility Guarantee | Breaking Change Policy |
|------|-----------|------------------------|----------------------|
| **Stable** | `#[doc(stable)]` | SemVer-compatible; no breaking changes without major version bump | Requires RFC and migration guide |
| **Experimental** | `#[doc(experimental)]` | Best-effort compatibility; breaking changes possible in minor versions | Requires deprecation notice (1 release) |
| **Internal** | `#[doc(hidden)]` | No compatibility guarantee; may change at any time | No notice required |

The stability annotation is implemented as a custom doc attribute:

```rust
/// Compiles Glyim source code to an executable binary.
///
/// # Stability
///
/// This function is **stable** and will not undergo breaking changes
/// without a major version bump.
#[doc(stable)]
pub fn build(input: &Path, output: Option<&Path>, mode: BuildMode) -> Result<PathBuf, PipelineError> {
    // ...
}
```

### 7.2 Deprecated APIs

The following APIs are deprecated in Phase 8, with migration paths:

| Deprecated API | Replacement | Migration Guide |
|---------------|-------------|-----------------|
| `compile_source_to_hir()` | `QueryPipeline::compile()` | Replace linear pipeline with query-driven pipeline; see Phase 4 plan |
| `compile_source_to_hir_incremental()` | `QueryPipeline::compile_incremental()` | Same as above; the `_incremental` variant was already a thin wrapper |
| `PipelineError::Parse(Vec<ParseError>)` with `(usize, usize)` spans | `PipelineError::Parse(Vec<DiagnosticSpan>)` with `FileId`-aware `Span` | Update error match arms to use `DiagnosticSpan` |
| `PipelineError::Type(Vec<TypeError>)` with `(usize, usize)` spans | `PipelineError::Type(Vec<DiagnosticSpan>)` with `FileId`-aware `Span` | Same as above |
| `glyim-fmt::format(source: &str) -> String` (placeholder) | `glyim-fmt::format_with_config(source, &FormatConfig)` (CST-aware) | Pass explicit configuration; default config matches old behavior |
| `glyim-testr::TestDependencyGraph` (placeholder) | `glyim-testr::TestDependencyGraph` (real implementation from Phase 6) | No code change; behavior changes from "return all" to "return affected" |
| `FlakeTracker` (placeholder) | `FlakeTracker` (real implementation from Phase 6) | No code change; flake scores will now be non-zero |

### 7.3 Unified Error Type System

Phase 8 unifies all error types to use `glyim-diag::Span` with `FileId`, eliminating the raw `(usize, usize)` tuples in `ParseError` and `TypeError`:

```rust
// crates/glyim-diag/src/diagnostic.rs

use crate::{Span, FileId, SourceMap};

/// A diagnostic with a source location, severity, and message.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Diagnostic {
    /// The severity of this diagnostic.
    pub severity: DiagnosticSeverity,
    /// The source location of this diagnostic.
    pub span: Span,
    /// The diagnostic message.
    pub message: String,
    /// Optional code identifying the diagnostic.
    pub code: Option<String>,
    /// Optional suggested fix.
    pub suggestion: Option<Suggestion>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum DiagnosticSeverity {
    Error,
    Warning,
    Note,
    Help,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Suggestion {
    pub message: String,
    pub replacement: Option<String>,
    pub span: Span,
}
```

The `PipelineError` enum is updated to use `Diagnostic`:

```rust
// crates/glyim-compiler/src/pipeline.rs (updated)

#[derive(Debug, Clone)]
pub enum PipelineError {
    /// Diagnostics collected from all pipeline stages.
    Diagnostics(Vec<Diagnostic>),
    /// Code generation error (LLVM failure).
    Codegen(String),
    /// Linking error.
    Link(String),
    /// I/O error (file not found, permission denied, etc.).
    Io(std::io::Error),
}
```

### 7.4 Feature Flag Consolidation

Phase 8 consolidates the scattered feature flags into a single `glyim-config` crate that provides a unified configuration system:

```rust
// crates/glyim-config/src/lib.rs

/// Configuration for the Glyim compiler.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CompilerConfig {
    /// Whether to use the query-driven incremental pipeline.
    pub incremental: bool,
    /// Coverage instrumentation mode.
    pub coverage: CoverageMode,
    /// Mutation testing configuration.
    pub mutation: MutationConfig,
    /// LSP server configuration.
    pub lsp: LspConfig,
    /// E-graph optimization configuration.
    pub egraph: EGraphConfig,
    /// Remote cache configuration.
    pub remote_cache: Option<RemoteCacheConfig>,
    /// Performance profiling.
    pub profiling: ProfilingConfig,
}

impl Default for CompilerConfig {
    fn default() -> Self {
        Self {
            incremental: false,  // off by default for backward compatibility
            coverage: CoverageMode::Off,
            mutation: MutationConfig::default(),
            lsp: LspConfig::default(),
            egraph: EGraphConfig::default(),
            remote_cache: None,
            profiling: ProfilingConfig::default(),
        }
    }
}
```

---

## 8. Error Message Quality

### 8.1 Current Error Message Quality

The current error messages are generated by miette's `Diagnostic` derive macro, which produces richly formatted terminal output with labeled spans and suggestions. However, several error messages are unhelpful or confusing:

| Error | Current Message | Improved Message |
|-------|----------------|-----------------|
| Type mismatch | `"type mismatch: expected {expected}, found {found}"` | `"type mismatch: expected `{expected}`, found `{found}`\n  help: this expression has type `{found}` because of the preceding operation\n  note: the function expects `{expected}` because of the parameter type"` |
| Undefined symbol | `"undefined symbol: {name}"` | `"cannot find symbol `{name}` in this scope\n  help: did you mean `{similar}`? (a symbol with a similar name exists at {location})"` |
| Duplicate definition | `"duplicate function: {name}"` | `"function `{name}` is defined multiple times\n  note: first definition at {location1}\n  note: second definition at {location2}\n  help: rename one of the functions or remove the duplicate"` |
| Missing return | `"function must return {type}"` | `"function `{name}` declared to return `{type}` but does not return a value\n  help: add a return expression at the end of the function body\n  note: the last expression in the function body has type `{actual_type}`"` |

### 8.2 Suggestion Engine

Phase 8 adds a suggestion engine that computes edit-distance-based suggestions for undefined symbols, similar to Rust's "did you mean" suggestions:

```rust
// crates/glyim-diag/src/suggest.rs

use glyim_interner::Interner;

/// Suggest similar symbols for an undefined name.
pub fn suggest_similar(
    name: &str,
    interner: &Interner,
    max_suggestions: usize,
) -> Vec<String> {
    let mut candidates: Vec<(usize, String)> = interner
        .all_symbols()
        .filter_map(|sym| {
            let dist = levenshtein_distance(name, sym);
            if dist <= max_edit_distance(name) {
                Some((dist, sym.to_string()))
            } else {
                None
            }
        })
        .collect();

    candidates.sort_by_key(|(dist, _)| *dist);
    candidates.truncate(max_suggestions);
    candidates.into_iter().map(|(_, name)| name).collect()
}

fn max_edit_distance(name: &str) -> usize {
    match name.len() {
        0..=3 => 1,
        4..=6 => 2,
        _ => 3,
    }
}

fn levenshtein_distance(a: &str, b: &str) -> usize {
    // Standard Levenshtein distance implementation
    let a_len = a.chars().count();
    let b_len = b.chars().count();
    let mut matrix = vec![vec![0; b_len + 1]; a_len + 1];

    for (i, row) in matrix.iter_mut().enumerate() {
        row[0] = i;
    }
    for j in 0..=b_len {
        matrix[0][j] = j;
    }

    for (i, a_char) in a.chars().enumerate() {
        for (j, b_char) in b.chars().enumerate() {
            let cost = if a_char == b_char { 0 } else { 1 };
            matrix[i + 1][j + 1] = (matrix[i][j + 1] + 1)
                .min(matrix[i + 1][j] + 1)
                .min(matrix[i][j] + cost);
        }
    }

    matrix[a_len][b_len]
}
```

---

## 9. Documentation Generation

### 9.1 Rustdoc Coverage

Phase 8 ensures that all public APIs have comprehensive rustdoc documentation. The target is 100% coverage for stable APIs and 80% coverage for experimental APIs. The documentation includes:

- **Description**: What the function/struct/enum does and why it exists.
- **Parameters**: What each parameter means and what constraints it has.
- **Return value**: What the function returns and under what conditions.
- **Errors**: What errors the function can return and what they mean.
- **Examples**: Runnable code examples that demonstrate typical usage.
- **Stability**: Which stability tier the API belongs to.
- **Since**: Which version introduced the API.

### 9.2 Architecture Documentation

Phase 8 generates an architecture document (as rustdoc `#[doc]` comments on crate-level modules) that describes the overall compiler architecture, the data flow between crates, and the design decisions behind each phase. This document serves as a reference for new contributors and for anyone modifying the compiler.

```
/// # Glyim Compiler Architecture
///
/// The Glyim compiler is organized into the following layers:
///
/// ## Front-End (Source → HIR)
/// - `glyim-lex`: Lexer producing `Token` stream
/// - `glyim-syntax`: Rowan-style lossless CST (`GreenNode`/`SyntaxNode`)
/// - `glyim-parse`: Parser producing lossy AST (`Item`, `ExprKind`, etc.)
/// - `glyim-macro-vfs`: Wasm-based procedural macro expansion
/// - `glyim-hir`: High-level IR with typed expressions
/// - `glyim-typeck`: Type checker producing `expr_types` and `call_type_args`
///
/// ## Middle-End (HIR Optimization)
/// - `glyim-query`: Query engine with memoization and red/green invalidation
/// - `glyim-merkle`: Merkle IR tree with content-addressed storage
/// - `glyim-egraph`: E-graph optimizer using `egg` for algebraic simplification
///
/// ## Back-End (HIR → Binary)
/// - `glyim-codegen-llvm`: LLVM code generation via Inkwell
/// - `glyim-compiler`: Pipeline orchestration, JIT, linking
///
/// ## Infrastructure
/// - `glyim-interner`: String interning
/// - `glyim-diag`: Diagnostics, spans, source maps
/// - `glyim-pkg`: Package management, dependency resolution, CAS
/// - `glyim-orchestrator`: Cross-module incremental compilation
/// - `glyim-watch`: File watcher for continuous compilation
/// - `glyim-testr`: Test runner with incremental execution
/// - `glyim-mutant`: Mutation testing engine
/// - `glyim-lsp`: Language Server Protocol implementation
/// - `glyim-bench`: Benchmarking and profiling infrastructure
```

---

## 10. Release Preparation

### 10.1 Version Bump

The workspace version is bumped from `0.5.0` to `1.0.0`, reflecting the completion of the incremental compiler transformation. The version is updated in:

1. The root `Cargo.toml` workspace `version` field.
2. The `glyim-cli` binary version string (displayed by `glyim --version`).
3. The LSP server info (displayed in editor status bars).
4. The lockfile format version (if the format has changed).

### 10.2 Binary Distribution

Phase 8 produces binary distributions for the following platforms:

| Platform | Target Triple | Distribution Format |
|----------|--------------|-------------------|
| Linux x86_64 | `x86_64-unknown-linux-gnu` | `.tar.gz` archive |
| Linux aarch64 | `aarch64-unknown-linux-gnu` | `.tar.gz` archive |
| macOS x86_64 | `x86_64-apple-darwin` | `.tar.gz` archive |
| macOS aarch64 | `aarch64-apple-darwin` | `.tar.gz` archive |
| Windows x86_64 | `x86_64-pc-windows-msvc` | `.zip` archive |

Each distribution includes:
- The `glyim` binary (compiler + LSP server)
- The `glyim-lsp` server (same binary, different entry point)
- A `README.md` with quick-start instructions
- A `LICENSE` file
- The VS Code extension files (`.vsix` package)
- The Neovim LSP configuration file

### 10.3 Homebrew Formula

A Homebrew formula is provided for macOS users:

```ruby
# Formula/glyim.rb
class Glyim < Formula
  desc "Incremental compiler for the Glyim programming language"
  homepage "https://github.com/elcoosp/glyim"
  url "https://github.com/elcoosp/glyim/releases/download/v1.0.0/glyim-v1.0.0-aarch64-apple-darwin.tar.gz"
  sha256 "<sha256>"
  version "1.0.0"

  def install
    bin.install "glyim"
  end

  test do
    system "#{bin}/glyim", "--version"
  end
end
```

### 10.4 VS Code Extension Package

The VS Code extension is packaged as a `.vsix` file using `vsce`:

```json
// editors/vscode/package.json
{
  "name": "glyim",
  "displayName": "Glyim Language Support",
  "description": "Language support for Glyim: syntax highlighting, diagnostics, completion, hover, navigation, formatting, and code actions",
  "version": "1.0.0",
  "publisher": "glyim",
  "engines": {
    "vscode": "^1.85.0"
  },
  "categories": ["Programming Languages", "Linters", "Formatters", "Debuggers"],
  "activationEvents": ["onLanguage:glyim"],
  "main": "./extension.js",
  "contributes": {
    "languages": [{
      "id": "glyim",
      "aliases": ["Glyim", "glyim"],
      "extensions": [".g"],
      "configuration": "./language-configuration.json"
    }],
    "grammars": [{
      "language": "glyim",
      "scopeName": "source.glyim",
      "path": "./syntaxes/glyim.tmLanguage.json"
    }]
  }
}
```

### 10.5 Migration Guide

The migration guide documents the changes from v0.5.0 to v1.0.0:

1. **Incremental compilation is now available**: Use `glyim build --incremental` for incremental builds, or set `incremental = true` in `glyim.toml`.
2. **LSP server is available**: Run `glyim lsp` to start the language server, or configure your editor to use it.
3. **Error types have changed**: `ParseError` and `TypeError` now use `Span` with `FileId` instead of raw `(usize, usize)` tuples.
4. **Legacy pipeline is deprecated**: `compile_source_to_hir()` is deprecated; use `QueryPipeline::compile()` instead.
5. **Test runner improvements**: `TestDependencyGraph` and `FlakeTracker` are no longer placeholders.
6. **Formatter is functional**: `glyim fmt` now produces formatted output instead of re-emitting the source unchanged.
7. **Mutation testing is available**: Use `glyim test --mutate` to run mutation tests.
8. **Remote caching is available**: Use `glyim build --remote-cache <URL>` to share compilation artifacts across a team.

---

## 11. Testing Strategy

### 11.1 Unit Tests

| Test | Location | Description |
|------|----------|-------------|
| `semantic_hash_incremental` | `glyim-hir/tests/` | Incremental hash computation matches full hash |
| `merkle_lru_cache_hit` | `glyim-merkle/tests/` | LRU cache returns correct data on hit |
| `merkle_lru_cache_eviction` | `glyim-merkle/tests/` | LRU cache evicts oldest entries when full |
| `merkle_write_batching` | `glyim-merkle/tests/` | Batched writes produce same result as individual writes |
| `interner_compact` | `glyim-interner/tests/` | Compaction removes unreferenced symbols |
| `egraph_memory_budget` | `glyim-egraph/tests/` | E-graph stops early when memory budget exceeded |
| `unified_diagnostic_span` | `glyim-compiler/tests/` | All error types use `Span` with `FileId` |
| `deprecated_api_warning` | `glyim-compiler/tests/` | Deprecated APIs produce compile-time warnings |
| `config_default_values` | `glyim-config/tests/` | Default configuration matches expected values |
| `suggestion_engine_accuracy` | `glyim-diag/tests/` | Suggested symbols have edit distance within threshold |
| `profile_collector_stages` | `glyim-bench/tests/` | Profile collector records all stages correctly |
| `regression_detector_threshold` | `glyim-bench/tests/` | Regression detector flags regressions above threshold |

### 11.2 Integration Tests

| Test | Location | Description |
|------|----------|-------------|
| `full_build_profile` | `glyim-bench/benches/` | Full build produces complete profile |
| `incremental_edit_benchmark` | `glyim-bench/benches/` | Incremental edit is faster than full build |
| `lsp_concurrent_requests` | `glyim-lsp/tests/` | LSP server handles concurrent requests without deadlock |
| `lsp_backpressure` | `glyim-lsp/tests/` | LSP server coalesces rapid edits under backpressure |
| `jit_crash_recovery` | `glyim-compiler/tests/` | JIT crash does not abort the compiler process |
| `fuzz_parser_no_panic` | `fuzz/` | Parser never panics on arbitrary input |
| `fuzz_typeck_no_panic` | `fuzz/` | Type checker never panics on arbitrary HIR |
| `workspace_build_memory` | `glyim-bench/benches/` | Workspace build stays within memory budget |
| `benchmark_regression_ci` | CI pipeline | CI fails if benchmarks regress >10% |

### 11.3 End-to-End Tests

| Test | Description |
|------|-------------|
| `v050_to_v100_migration` | A project built with v0.5.0 CLI can be built with v1.0.0 CLI without changes |
| `vscode_extension_lifecycle` | VS Code extension starts, provides completions, handles shutdown |
| `neovim_lsp_lifecycle` | Neovim LSP client connects, receives diagnostics, handles disconnect |
| `release_binary_self_test` | Release binary compiles its own test fixtures successfully |
| `incremental_state_upgrade` | Incremental state from Phase 4 schema is compatible with Phase 8 schema |

---

## 12. Implementation Timeline

### Week 1–2: Profiling Infrastructure and Baseline

| Day | Task |
|-----|------|
| 1–2 | Create `glyim-bench` crate with `criterion` integration and fixture generator |
| 3–4 | Implement `ProfileCollector` and integrate into `QueryPipeline` |
| 5 | Run baseline profiling on all fixtures; establish benchmark baselines |
| 6–7 | Set up CI benchmark comparison job with regression threshold |
| 8–9 | Profile memory usage with `jemalloc` and `heaptrack`; identify top consumers |
| 10 | Document profiling results and optimization targets |

### Week 3–4: Performance Optimization

| Day | Task |
|-----|------|
| 11–13 | Implement semantic hash optimization (Blake3, incremental hashing, expression-level caching) |
| 14–16 | Implement Merkle store optimization (LRU cache, write batching) |
| 17–18 | Implement e-graph memory budget |
| 19 | Implement Interner compaction |
| 20 | Implement AnalysisDatabase LRU eviction for LSP |

### Week 3–5 (parallel): Fuzz Testing and Concurrency Audit

| Day | Task |
|-----|------|
| 11–13 | Implement parser, type checker, and HIR lowering fuzz targets |
| 14–16 | Run fuzz targets for 10 minutes each; fix any discovered crashes |
| 17–19 | Concurrency audit of LSP server (RwLock fairness, bounded channels) |
| 20–22 | Implement `loom`-based concurrency tests for critical paths |
| 23–24 | JIT crash safety (signal handlers, structured exception handling) |

### Week 5–6: API Stabilization

| Day | Task |
|-----|------|
| 25–26 | Audit all public APIs; add stability tier annotations |
| 27–28 | Deprecate legacy pipeline and old error types |
| 29–30 | Implement unified `Diagnostic` type with `Span` and `FileId` |
| 31–32 | Create `glyim-config` crate with consolidated feature flags |
| 33 | Implement suggestion engine for undefined symbol errors |

### Week 7–8: Documentation and Release

| Day | Task |
|-----|------|
| 34–36 | Write rustdoc for all public APIs (100% stable, 80% experimental) |
| 37 | Write architecture documentation as crate-level doc comments |
| 38 | Write migration guide from v0.5.0 to v1.0.0 |
| 39 | Build release binaries for all platforms |
| 40 | Package VS Code extension and Neovim LSP configuration |
| 41 | Create Homebrew formula |
| 42 | Final integration testing and release |

---

## 13. Crate Dependency Changes

### 13.1 New Crates

| Crate | Description | Dependencies |
|-------|-------------|--------------|
| `glyim-bench` | Benchmarking and profiling | `glyim-compiler`, `glyim-hir`, `glyim-orchestrator`, `glyim-mutant`, `glyim-lsp`, `criterion`, `jemalloc-ctl`, `blake3` |
| `glyim-config` | Unified compiler configuration | `serde`, `glyim-hir` (for `EGraphConfig` types) |

### 13.2 Modified Crates

| Crate | Changes |
|-------|---------|
| `glyim-hir` | Replace SHA-256 with Blake3 in `semantic_hash`; add `ExprHashCache`; optimize hashing |
| `glyim-merkle` | Add LRU cache and write batching to `MerkleStore` |
| `glyim-interner` | Add `compact()` method with reference counting |
| `glyim-egraph` | Add `memory_budget` field to `EGraphConfig`; early termination when budget exceeded |
| `glyim-diag` | Add `Diagnostic` type with `Span` + `FileId`; add `suggest.rs` for edit-distance suggestions |
| `glyim-compiler` | Deprecate `compile_source_to_hir()`; add `ProfileCollector` integration; JIT crash safety |
| `glyim-lsp` | Replace `mpsc::unbounded_channel` with bounded channel; add LRU eviction; poison recovery |
| `glyim-parse` | Update `ParseError` to use `Span` with `FileId` |
| `glyim-typeck` | Update `TypeError` to use `Span` with `FileId` |
| `glyim-cli` | Add `glyim-config` integration; update all commands to use unified config |
| `Cargo.toml` (workspace) | Add `glyim-bench` and `glyim-config` to workspace members |

---

## 14. Risk Register

| Risk | Probability | Impact | Mitigation |
|------|------------|--------|------------|
| Blake3 hash compatibility | Low | Medium | Blake3 produces different hashes than SHA-256; incremental state must be invalidated on upgrade. Provide `glyim clean` command. |
| LRU cache coherence | Medium | Low | Cache may return stale data if CAS is modified externally. Add cache invalidation on CAS store events. |
| `loom` test false positives | Medium | Low | `loom` explores all interleavings, which may include impossible schedules. Validate with real stress tests. |
| Deprecation breaking user code | Medium | Medium | Provide `#[allow(deprecated)]` escape hatch and migration guide. |
| Memory budget too aggressive | Low | Medium | E-graph may stop early on valid code, producing suboptimal but correct output. Make budget configurable. |
| Release binary compatibility | Low | High | Different LLVM versions on different platforms may produce different code. Pin LLVM version in release builds. |
| Fuzz target timeout | Medium | Low | Fuzz targets may take too long in CI. Set a 10-minute timeout per target. |
| Profiling overhead | Low | Low | `ProfileCollector` adds minimal overhead when disabled (all methods inlined to no-ops). |

---

## 15. Performance Targets

| Metric | Current Baseline | Target | Measurement |
|--------|-----------------|--------|-------------|
| Full build (100 functions) | Not measured | < 500ms | `glyim-bench bench_full_build` |
| Incremental edit (1 function changed) | Not measured | < 30ms | `glyim-bench bench_incremental_edit` |
| Incremental edit (5 functions changed) | Not measured | < 80ms | `glyim-bench bench_incremental_edit` |
| Semantic hash per item | Not measured | < 50us | `glyim-bench bench_semantic_hash` |
| Merkle store lookup (cache hit) | Not measured | < 1us | `glyim-bench bench_merkle_store` |
| Merkle store lookup (cache miss) | Not measured | < 1ms | `glyim-bench bench_merkle_store` |
| LSP diagnostics on edit | Not measured | < 50ms | `glyim-bench bench_lsp` |
| LSP completion request | Not measured | < 20ms | `glyim-bench bench_lsp` |
| E-graph saturation (medium function) | Not measured | < 500ms | `glyim-bench bench_egraph` |
| E-graph saturation (large function) | Not measured | < 2s or budget stop | `glyim-bench bench_egraph` |
| Peak RSS (workspace 5 packages) | Not measured | < 512MB | `glyim-bench bench_workspace` |
| Parser throughput | Not measured | > 100K LOC/s | `glyim-bench bench_parser` |
| Mutation testing (50 mutants) | Not measured | < 30s | `glyim-bench bench_mutation` |
| JIT compile + execute (single function) | Not measured | < 100ms | `glyim-bench bench_jit` |

---

## 16. Migration Strategy

### 16.1 Feature Flag Migration

The feature flags introduced across Phases 0–7 are consolidated into the `glyim-config` crate. The migration path is:

1. **Phase 8, Week 5**: Introduce `glyim-config` crate alongside existing feature flags.
2. **Phase 8, Week 6**: Wire CLI commands to use `glyim-config` instead of individual flags.
3. **v1.0.0 release**: All feature flags are mapped to `CompilerConfig` fields. Legacy flags still work (with deprecation warnings).
4. **v1.1.0 (future)**: Remove legacy flag support.

### 16.2 Error Type Migration

The migration from raw `(usize, usize)` spans to `Span` with `FileId` follows the same strategy as Phase 7:

1. **Phase 8, Week 5**: Introduce `Diagnostic` type in `glyim-diag`.
2. **Phase 8, Week 5**: Update `ParseError` and `TypeError` to use `Span` with `FileId`.
3. **Phase 8, Week 6**: Update `PipelineError` to use `Vec<Diagnostic>`.
4. **v1.0.0 release**: All error types use `Diagnostic` with `Span`.
5. **v1.1.0 (future)**: Remove old error type variants.

### 16.3 Incremental State Compatibility

The incremental state format changes between v0.5.0 and v1.0.0 (new fields for item hashes, test dependency graphs, mutation scores). The migration strategy is:

1. When loading incremental state, check the format version.
2. If the version is older, clear the incremental state and rebuild from scratch.
3. Log a warning: "Incremental state format has changed; full rebuild required."
4. This is a one-time cost that only affects users upgrading from v0.5.0.

---

## 17. Success Criteria

### 17.1 Performance Criteria

- [ ] Full build of 100-function fixture completes in under 500ms
- [ ] Incremental edit of 1 function completes in under 30ms
- [ ] LSP diagnostics on edit arrive in under 50ms
- [ ] Peak RSS for workspace build stays under 512MB
- [ ] Parser throughput exceeds 100K LOC/s
- [ ] No performance regression exceeds 10% in any benchmark

### 17.2 Reliability Criteria

- [ ] Parser fuzz target runs for 10 minutes without discovering crashes
- [ ] Type checker fuzz target runs for 10 minutes without discovering crashes
- [ ] HIR lowering fuzz target runs for 10 minutes without discovering crashes
- [ ] LSP server handles 100 concurrent requests without deadlock
- [ ] JIT crash recovery returns an error instead of aborting the process
- [ ] E-graph memory budget prevents OOM on pathological inputs

### 17.3 API Quality Criteria

- [ ] All stable APIs have rustdoc with description, parameters, return value, errors, and examples
- [ ] All deprecated APIs have `#[deprecated]` annotations with migration guidance
- [ ] All error types use `Span` with `FileId` (no raw `(usize, usize)` tuples remain)
- [ ] Legacy pipeline `compile_source_to_hir()` produces a deprecation warning when used
- [ ] `glyim-config` crate provides a single source of truth for all compiler configuration

### 17.4 Release Criteria

- [ ] Binary distributions available for Linux (x86_64, aarch64), macOS (x86_64, aarch64), Windows (x86_64)
- [ ] VS Code extension package (`.vsix`) available and functional
- [ ] Neovim LSP configuration available and functional
- [ ] Homebrew formula available for macOS
- [ ] Migration guide from v0.5.0 to v1.0.0 published
- [ ] All integration tests pass on all supported platforms
