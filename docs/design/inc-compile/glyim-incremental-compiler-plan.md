# Glyim Incremental Compiler: Full Implementation Plan

> **Target**: Transform the Glyim compiler (v0.5.0) from a batch, whole-file compiler into a state-of-the-art incremental compilation platform with JIT-first live semantics, fine-grained caching, e-graph optimization, and integrated mutation testing.
>
> **Codebase**: `elcoosp-glyim` — Rust workspace, 20 crates, LLVM 22.1 / Inkwell 0.9 backend, Wasm procedural macros (wasmtime 44), Bazel REv2 CAS server, sea-orm test history DB.

---

## Table of Contents

1. [Current State Assessment](#1-current-state-assessment)
2. [Architecture Vision](#2-architecture-vision)
3. [Phase 0 — Foundation: Query Engine & Dependency DAG](#3-phase-0--foundation-query-engine--dependency-dag)
4. [Phase 1 — Fine-Grained Incremental Compilation](#4-phase-1--fine-grained-incremental-compilation)
5. [Phase 2 — JIT Live Compiler & Micro-Modules](#5-phase-2--jit-live-compiler--micro-modules)
6. [Phase 3 — E-Graph Middle-End & Algebraic Optimization](#6-phase-3--e-graph-middle-end--algebraic-optimization)
7. [Phase 4 — Speculative & Profile-Guided Optimization](#7-phase-4--speculative--profile-guided-optimization)
8. [Phase 5 — AOT Release Pipeline & ThinLTO Integration](#8-phase-5--aot-release-pipeline--thinlto-integration)
9. [Phase 6 — Integrated Test Runner Revolution](#9-phase-6--integrated-test-runner-revolution)
10. [Phase 7 — Compiler-Level Mutation Testing](#10-phase-7--compiler-level-mutation-testing)
11. [Phase 8 — Self-Healing, Observability & Advanced Safety](#11-phase-8--self-healing-observability--advanced-safety)
12. [New Crate Map](#12-new-crate-map)
13. [Migration & Compatibility Strategy](#13-migration--compatibility-strategy)
14. [Risk Register](#14-risk-register)
15. [Milestone Timeline](#15-milestone-timeline)

---

## 1. Current State Assessment

### 1.1 Compilation Pipeline (as-is)

```
Source → Macro Expand (wasmtime) → Parse (rowan) → DeclTable → HIR Lower
  → TypeCheck → Method Desugar → Monomorphize (BFS) → LLVM Codegen (Inkwell)
  → Object File → Link (cc/gcc) → Binary
                                           ↘ JIT Execute (inkwell JitExecutionEngine)
```

### 1.2 Existing Caching/Incremental Mechanisms

| Mechanism | Location | Granularity | Status |
|---|---|---|---|
| `build_with_cache()` | `glyim-compiler/src/pipeline.rs` | Whole source file (SHA-256) | Working but coarse |
| `MacroExpansionCache` | `glyim-macro-core/src/cache.rs` | Per macro invocation (compiler_ver + target + wasm_hash + ast_hash) | Working, CAS-backed |
| `mono_cache` | `glyim-codegen-llvm/src/codegen/mod.rs` | Per `(Symbol, Vec<HirType>)` LLVM `FunctionValue` | In-memory only, lost on restart |
| `LocalContentStore` | `glyim-macro-vfs/src/local.rs` | Content-addressable blobs (SHA-256) | Working |
| `RemoteContentStore` | `glyim-macro-vfs/src/remote.rs` | HTTP CAS with local cache | Working |
| Bazel REv2 CAS | `glyim-cas-server/` | gRPC + REST, blob + action results | Working |
| Test history DB | `glyim-testr/src/history.rs` | sea-orm/SQLite schema | Schema defined, **not wired** |

### 1.3 Critical Gaps

| Gap | Impact | Crate Affected |
|---|---|---|
| **No query-based dependency tracking** | Every change recompiles everything | `glyim-compiler` |
| **No fine-grained invalidation** | Cannot skip unchanged items within a file | `glyim-compiler` |
| **No e-graph / equality saturation** | No algebraic optimization, no semantic equivalence proofs | *(missing entirely)* |
| **No effect system** | Cannot prove purity, cannot parallelize safely | `glyim-hir`, `glyim-typeck` |
| **No semantic diffing** | Whitespace/comment changes trigger full recompilation | `glyim-compiler` |
| **No module partitioning for JIT** | Single monolithic LLVM `Module` per compilation | `glyim-codegen-llvm` |
| **No OrcV2 lazy reexports** | No lazy compilation, no dylib swapping | `glyim-codegen-llvm` |
| **No IR patching** | Any change rebuilds entire LLVM `FunctionValue` | `glyim-codegen-llvm` |
| **No profile collection** | No JIT→AOT feedback loop | *(missing entirely)* |
| **Incremental test features are stubs** | `DependencyGraph`, `FlakeTracker`, `FileWatcher` are placeholders | `glyim-testr` |
| **No mutation testing** | No compiler-level mutation analysis | *(missing entirely)* |
| **No parallel pipeline** | Single-threaded compilation | `glyim-compiler` |
| **No multi-file module system** | Single-file compilation only | `glyim-compiler`, `glyim-hir` |

### 1.4 What We Can Build On

The existing infrastructure provides several powerful foundations:

- **CAS infrastructure is mature**: `LocalContentStore`, `RemoteContentStore`, and the Bazel REv2 gRPC server give us content-addressable storage at local, remote, and distributed levels. This is the backbone for Merkle caching, sub-function CAS, and remote build caching.
- **Macro expansion cache proves the model**: The `MacroExpansionCache` already demonstrates deterministic cache keys (compiler version + target + input hashes), CAS-backed storage, and cache-first execution with fallback. This exact pattern should be replicated at every pipeline stage.
- **Monomorphization already tracks per-instantiation data**: `MonoContext` already maintains `fn_specs`, `struct_specs`, and `mangle_table` — these are natural cache keys for per-generic-instantiation caching.
- **Inkwell provides OrcV2 access**: The `inkwell` 0.9 crate exposes `ExecutionSession`, `JITDylib`, and related OrcV2 APIs needed for micro-modules, lazy reexports, and dylib swapping.
- **Rowan CST enables structural diffs**: The `glyim-syntax` crate uses rowan 0.16, which provides lossless syntax trees with node IDs — perfect for structural AST diffing without re-parsing.

---

## 2. Architecture Vision

### 2.1 The "Quantum Compiler" Pipeline

The target architecture transforms the compiler from a batch processor into a **live, stateful database of code semantics**:

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                        GLYIM QUANTUM COMPILER                               │
│                                                                             │
│  ┌──────────┐   ┌──────────────┐   ┌───────────┐   ┌──────────────────┐   │
│  │  Source   │──▶│  Semantic    │──▶│  Query    │──▶│  Merkle IR Tree  │   │
│  │  Diffs   │   │  Normalizer  │   │  Engine   │   │  (Content-Addr)  │   │
│  └──────────┘   └──────────────┘   │  (Salsa)  │   └────────┬─────────┘   │
│                                     └─────┬─────┘            │             │
│                                           │                  │             │
│                    ┌──────────────────────┼──────────────────┘             │
│                    │                      │                                │
│              ┌─────▼─────┐        ┌───────▼──────┐                        │
│              │  E-Graph   │        │  Effect      │                        │
│              │  Optimizer │        │  Analyzer    │                        │
│              └─────┬─────┘        └───────┬──────┘                        │
│                    │                      │                                │
│              ┌─────▼──────────────────────▼─────┐                          │
│              │     Invariant Certificate Store   │                          │
│              │  (Optimization Decision Cache)    │                          │
│              └──────────────┬────────────────────┘                          │
│                             │                                               │
│          ┌──────────────────┼──────────────────┐                            │
│          │                  │                  │                            │
│    ┌─────▼─────┐    ┌──────▼──────┐   ┌──────▼──────┐                     │
│    │ JIT Path  │    │ AOT Path    │   │ Test Path   │                     │
│    │ (OrcV2)   │    │ (ThinLTO)   │   │ (Live)      │                     │
│    └─────┬─────┘    └──────┬──────┘   └──────┬──────┘                     │
│          │                  │                  │                            │
│    ┌─────▼─────┐    ┌──────▼──────┐   ┌──────▼──────┐                     │
│    │ Micro-    │    │ Summary-    │   │ Mutation    │                     │
│    │ Modules   │    │ Driven LTO  │   │ Engine      │                     │
│    │ + Dylib   │    │ + PGO       │   │ + E-Graph   │                     │
│    │ Swapping  │    │ + CAS       │   │ Pruning     │                     │
│    └───────────┘    └─────────────┘   └─────────────┘                     │
│                                                                             │
│  ┌─────────────────────────────────────────────────────────────────────┐    │
│  │                   Persistent Profile Database                       │    │
│  │   (JIT types, branches, hot paths → feeds AOT + E-Graph)           │    │
│  └─────────────────────────────────────────────────────────────────────┘    │
│                                                                             │
│  ┌─────────────────────────────────────────────────────────────────────┐    │
│  │                   Sub-Function CAS (Local + Remote)                 │    │
│  │   (Per-chunk cache keys: IR_hash + target + flags + summaries)      │    │
│  └─────────────────────────────────────────────────────────────────────┘    │
└─────────────────────────────────────────────────────────────────────────────┘
```

### 2.2 Design Principles

1. **Content-addressed everywhere**: Every artifact (AST node, HIR item, IR chunk, object code) is identified by its content hash, not its file path or branch name.
2. **Query-driven demand**: Compilation is a graph of memoized queries; only dirty queries are re-executed.
3. **Semantic, not syntactic**: Normalization before fingerprinting ensures that semantically equivalent code shares cache entries.
4. **Speculative by default**: The compiler predicts what will change next and pre-compiles it in the background.
5. **Effect-aware**: Every function carries purity/side-effect metadata, enabling safe parallelism and dead-code elimination.
6. **Fractal granularity**: Cache granularity adapts dynamically to edit velocity and file size.

---

## 3. Phase 0 — Foundation: Query Engine & Dependency DAG

**Goal**: Replace the linear pipeline with a demand-driven, memoized query system. This is the single highest-impact change — every subsequent phase depends on it.

### 3.1 Introduce `glyim-query` Crate

**New crate**: `crates/glyim-query/`

This crate provides the foundational query infrastructure, inspired by Rust's `rustc_query_system` and the `salsa` crate, but tailored for Glyim's needs.

#### Key Types

```rust
/// A query key — uniquely identifies a computation
pub trait QueryKey: Hash + Eq + Clone + Debug + Send + Sync + 'static {}

/// A query definition — describes a memoizable computation
pub struct QueryDef<K, V> {
    pub name: Symbol,
    pub compute: fn(&QueryContext, K) -> V,
    pub hash_inputs: fn(&K) -> ContentHash,
    pub dependencies: fn(&QueryContext, &K) -> Vec<Dependency>,
}

/// Dependency edge between queries
pub enum Dependency {
    Query { key: ContentHash },
    File { path: PathBuf, hash: ContentHash },
    Config { key: String, value: ContentHash },
}

/// The query context — holds memoized results and dependency graph
pub struct QueryContext {
    db: DashMap<ContentHash, QueryResult>,
    dep_graph: RwLock<DepGraph>,
    config_fingerprints: HashMap<String, ContentHash>,
}

/// Stored result of a query computation
pub struct QueryResult {
    pub value: Arc<dyn Any + Send + Sync>,
    pub fingerprint: ContentHash,
    pub dependencies: Vec<Dependency>,
    pub computed_at: Instant,
    pub provenance: Provenance,
}

/// Provenance metadata for self-healing (Phase 8)
pub struct Provenance {
    pub compiler_version: String,
    pub pass_name: Symbol,
    pub input_hashes: Vec<ContentHash>,
    pub timestamp: u64,
}
```

#### Core Queries for Glyim

Define queries that map directly to the current pipeline stages:

| Query | Input Key | Output | Current Location |
|---|---|---|---|
| `parse_file` | `PathBuf` | `ParseTree` | `glyim-parse::parse` |
| `lower_to_hir` | `(PathBuf, DeclTable)` | `Hir` | `glyim-hir::lower_with_declarations` |
| `type_check` | `HirFingerprint` | `(Vec<HirType>, HashMap<ExprId, Vec<HirType>>)` | `glyim-typeck::TypeChecker::check` |
| `desugar_methods` | `(HirFingerprint, TypeCheckResult)` | `Hir` | `glyim-hir::desugar_method_calls` |
| `monomorphize` | `(HirFingerprint, TypeCheckResult)` | `MonoResult` | `glyim-hir::monomorphize::monomorphize` |
| `lower_to_llvm` | `(MonoResultFingerprint, TargetTriple, OptLevel)` | `inkwell::Module` | `glyim-codegen-llvm::Codegen` |
| `optimize_llvm` | `(ModuleFingerprint, OptLevel)` | `inkwell::Module` | `inkwell::PassManager` |
| `codegen` | `(ModuleFingerprint, TargetTriple)` | `ObjectCode` | `inkwell::TargetMachine` |
| `link` | `(Vec<ObjectCode>, LinkConfig)` | `Binary` | `cc::Build` |

#### Red/Green Marking Algorithm

Implement Rust-style red/green marking:

```rust
impl QueryContext {
    /// Mark queries as red (dirty) based on changed inputs
    pub fn invalidate(&self, changed: &[Dependency]) -> InvalidationReport {
        // 1. Find all queries that directly depend on changed inputs
        // 2. Mark those queries RED
        // 3. Propagate: find all queries depending on RED queries, mark them RED
        // 4. Return report: which queries are red, which remain green
    }

    /// Execute a query, reusing green cached results where possible
    pub fn query<K, V>(&self, def: &QueryDef<K, V>, key: K) -> V {
        let fingerprint = (def.hash_inputs)(&key);
        if let Some(result) = self.db.get(&fingerprint) {
            if result.is_green() {
                return result.value.clone().downcast().unwrap();
            }
        }
        // Compute, record dependencies, store
        let value = (def.compute)(self, key.clone());
        self.store_result(fingerprint, &value, /* deps */);
        value
    }
}
```

#### Files to Create

```
crates/glyim-query/
├── Cargo.toml
└── src/
    ├── lib.rs           — public API, re-exports
    ├── context.rs       — QueryContext, QueryResult
    ├── def.rs           — QueryDef, QueryKey trait
    ├── dep_graph.rs     — DependencyGraph (petgraph-based)
    ├── fingerprint.rs   — ContentHash computation for query inputs
    ├── invalidation.rs  — Red/green marking algorithm
    ├── persistence.rs   — Serialize/deserialize query DB to disk
    └── tests/
        ├── mod.rs
        ├── basic_queries.rs
        ├── invalidation_tests.rs
        └── persistence_tests.rs
```

### 3.2 Integrate Query Engine into Pipeline

**Modify**: `crates/glyim-compiler/src/pipeline.rs`

Replace the linear `compile_source_to_hir()` function with a query-driven approach:

```rust
pub fn compile_source_to_hir_query(
    ctx: &QueryContext,
    source_path: PathBuf,
    config: &PipelineConfig,
) -> Result<CompiledHir, PipelineError> {
    // Step 1: Parse (query)
    let parse_tree = ctx.query(&PARSE_FILE_QUERY, source_path.clone())?;

    // Step 2: Build declaration table
    let decl_table = ctx.query(&BUILD_DECL_TABLE_QUERY, source_path.clone())?;

    // Step 3: Lower to HIR (query)
    let hir = ctx.query(&LOWER_TO_HIR_QUERY, (source_path.clone(), decl_table))?;

    // Step 4: Type check (query)
    let tc_result = ctx.query(&TYPE_CHECK_QUERY, hir.fingerprint())?;

    // Step 5: Desugar methods (query)
    let desugared = ctx.query(&DESUGAR_METHODS_QUERY, (hir.fingerprint(), tc_result.fingerprint()))?;

    // Step 6: Monomorphize (query)
    let mono = ctx.query(&MONOMORPHIZE_QUERY, (desugared.fingerprint(), tc_result.fingerprint()))?;

    Ok(CompiledHir { hir: desugared, mono_hir: mono, /* ... */ })
}
```

**Key change**: Each stage is a memoized query. If the source hasn't changed, `parse_file` returns the cached result instantly. If only one function changed, only that function's downstream queries are re-executed.

### 3.3 Persist Query State Between Builds

**Modify**: `crates/glyim-compiler/src/pipeline.rs`

Add `--incremental` flag that:
1. On first build: creates `./.glyim-cache/query-db/` directory
2. After each build: serializes `QueryContext` (fingerprints + dependency graph + green/red status)
3. On subsequent build: loads previous `QueryContext`, invalidates based on changed files, only re-runs red queries

```rust
pub struct IncrementalState {
    query_ctx: QueryContext,
    source_hashes: HashMap<PathBuf, ContentHash>,
    last_build_time: Instant,
}

impl IncrementalState {
    pub fn load_or_create(cache_dir: &Path) -> Self {
        if cache_dir.exists() {
            Self::load_from_disk(cache_dir).unwrap_or_else(|_| Self::fresh())
        } else {
            Self::fresh()
        }
    }

    pub fn apply_changes(&mut self, changed_files: &[PathBuf]) -> InvalidationReport {
        // Re-hash changed files
        // Invalidate queries depending on those files
        // Return report showing what needs recompilation
    }
}
```

### 3.4 Dependency Tracking at Name Level (Name Hashing)

**New file**: `crates/glyim-hir/src/dependency_names.rs`

Implement Zinc/sbt-style name hashing to track dependencies at the symbol level, not just the file level:

```rust
/// Tracks which names a HIR item defines and which names it references
pub struct NameDependencyTable {
    /// Maps each HIR item to the names it defines
    definitions: HashMap<HirItemId, Vec<Symbol>>,
    /// Maps each HIR item to the names it references
    references: HashMap<HirItemId, Vec<Symbol>>,
    /// Reverse index: for each name, which items reference it
    dependents: HashMap<Symbol, HashSet<HirItemId>>,
}

impl NameDependencyTable {
    /// Build from a typed HIR
    pub fn build_from_hir(hir: &Hir, type_info: &TypeCheckResult) -> Self {
        // Walk every expression, record:
        // - Function definitions → their names
        // - Struct/enum definitions → their names
        // - Variable references → which names they use
        // - Type references → which type names they use
        // - Method calls → which impl method names they use
    }

    /// Given a changed item, return all items that transitively depend on it
    pub fn transitive_dependents(&self, changed: &[HirItemId]) -> HashSet<HirItemId> {
        // BFS through the dependency graph using the `dependents` index
    }
}
```

### 3.5 Implementation Steps for Phase 0

| Step | Task | Estimated Effort |
|---|---|---|
| 0.1 | Create `glyim-query` crate with core types | 2-3 days |
| 0.2 | Implement `QueryContext`, `QueryDef`, `DependencyGraph` | 3-4 days |
| 0.3 | Implement red/green marking algorithm | 2-3 days |
| 0.4 | Implement on-disk persistence of query state | 2-3 days |
| 0.5 | Define all core pipeline queries | 2-3 days |
| 0.6 | Refactor `pipeline.rs` to use query engine | 4-5 days |
| 0.7 | Build `NameDependencyTable` in `glyim-hir` | 3-4 days |
| 0.8 | Integrate name-based invalidation into query engine | 2-3 days |
| 0.9 | Add `--incremental` CLI flag | 1 day |
| 0.10 | Write comprehensive tests | 3-4 days |
| **Total** | | **~25-33 days** |

### 3.6 Success Criteria for Phase 0

- `glyim build --incremental` on an unchanged codebase completes in <50ms (cache hit on all queries)
- Changing one function in a 1000-line file only re-runs queries for that function and its dependents
- Query state survives process restart (persisted to disk)
- Name-based invalidation: renaming a local variable triggers zero downstream recompilation

---

## 4. Phase 1 — Fine-Grained Incremental Compilation

**Goal**: Implement semantic diffing, Merkle IR caching, and fractal cache granularity.

### 4.1 Semantic Normalization Pass (Alpha-Equivalence Short-Circuiting)

**New file**: `crates/glyim-hir/src/normalize.rs`

Before fingerprinting any HIR item for the query cache, normalize it to eliminate syntactic differences that don't affect semantics:

```rust
/// Normalizes a HIR item for semantic fingerprinting
pub struct SemanticNormalizer {
    /// Renaming map: original variable name → canonical name
    var_rename_map: HashMap<Symbol, Symbol>,
    /// Counter for generating canonical names
    next_var_id: u32,
}

impl SemanticNormalizer {
    /// Normalize an HIR function for fingerprinting
    pub fn normalize_fn(hir_fn: &HirFn) -> NormalizedHirFn {
        // 1. Rename all local variables to _v0, _v1, _v2, ... in order of first appearance
        // 2. Strip comments (already done — HIR doesn't carry comments)
        // 3. Normalize associative operations: sort operands of commutative ops
        //    (a + b) → (b + a) if a > b lexicographically
        // 4. Normalize boolean expressions: double negation elimination
        // 5. Normalize literal representations: 007 → 7, 0xFF → 255
        // 6. Return a structurally identical but semantically canonical version
    }

    /// Compute a semantic hash that ignores purely syntactic changes
    pub fn semantic_hash(hir_fn: &HirFn) -> ContentHash {
        let normalized = Self::normalize_fn(hir_fn);
        ContentHash::hash(&normalized)
    }
}
```

**Integration point**: Modify the `LOWER_TO_HIR_QUERY` to use `semantic_hash` instead of raw source hash as the query key. This means:
- Auto-formatting a file (prettier/rustfmt equivalent) → semantic hash unchanged → all downstream queries are green → **zero recompilation**
- Renaming a local variable → semantic hash unchanged → **zero recompilation**
- Adding/removing comments → already handled (comments don't reach HIR)

### 4.2 Merkle IR Tree (Branch-Agnostic Caching)

**New crate**: `crates/glyim-merkle/`

Store HIR and LLVM IR in a Merkle DAG where each node's hash depends only on its content and the hashes of its children. This makes the cache completely independent of Git branches.

```rust
/// A node in the Merkle IR tree
pub struct MerkleNode {
    /// Content hash of this node
    hash: ContentHash,
    /// Hashes of child nodes
    children: Vec<ContentHash>,
    /// The actual data (HIR item, LLVM IR chunk, etc.)
    data: MerkleNodeData,
    /// Provenance metadata
    provenance: Provenance,
}

pub enum MerkleNodeData {
    HirItem(HirItemId, Arc<HirItem>),
    HirFn(HirItemId, Arc<HirFn>),
    LlvmFunction(String, Arc<[u8]>),  // Serialized LLVM function
    ObjectCode(String, Arc<[u8]>),    // Compiled machine code
}

/// The Merkle IR store — content-addressed, branch-agnostic
pub struct MerkleStore {
    /// Backed by the existing CAS infrastructure
    cas: Arc<dyn ContentStore>,
    /// In-memory cache of deserialized nodes
    cache: DashMap<ContentHash, Arc<MerkleNode>>,
}

impl MerkleStore {
    /// Store a HIR item as a Merkle node
    pub fn store_hir_item(&self, item: &HirItem, deps: &[ContentHash]) -> ContentHash {
        let hash = ContentHash::hash_with_deps(item, deps);
        let node = MerkleNode { hash, children: deps.to_vec(), /* ... */ };
        self.cas.store(&node.serialize())?;
        self.cache.insert(hash, Arc::new(node));
        hash
    }

    /// Look up a node by hash — works across branches
    pub fn get(&self, hash: &ContentHash) -> Option<Arc<MerkleNode>> {
        // Check in-memory cache first, then CAS
        self.cache.get(hash).cloned().or_else(|| {
            self.cas.retrieve(hash).ok().and_then(|data| {
                let node = MerkleNode::deserialize(&data)?;
                self.cache.insert(*hash, Arc::new(node.clone()));
                Some(Arc::new(node))
            })
        })
    }

    /// Find the root hash for a given "build state" (source file set)
    /// This is simply the hash of all top-level item hashes
    pub fn compute_root(&self, items: &[(HirItemId, ContentHash)]) -> ContentHash {
        ContentHash::hash_ordered(items.iter().map(|(_, h)| h))
    }
}
```

**Branch switching as O(1)**:
1. On `main` branch: compute Merkle root from all item hashes → `root_main`
2. Switch to `feature` branch: compute new Merkle root → `root_feature`
3. Items that are identical between branches share the same Merkle nodes (same content hash)
4. Only items that differ need recompilation; shared subtrees are reused from cache

**Integration with existing CAS**: `MerkleStore` wraps the existing `ContentStore` trait from `glyim-macro-vfs`, so it works with both `LocalContentStore` and `RemoteContentStore` out of the box.

### 4.3 Fractal Cache Granularity (Adaptive Zooming)

**New file**: `crates/glyim-query/src/granularity.rs`

The compiler dynamically adjusts its caching granularity based on edit patterns:

```rust
/// Tracks edit velocity per file to determine optimal cache granularity
pub struct GranularityMonitor {
    /// Per-file edit history
    edit_history: DashMap<PathBuf, EditHistory>,
    /// Current granularity setting per file
    granularity: DashMap<PathBuf, CacheGranularity>,
}

#[derive(Clone, Copy, PartialEq)]
pub enum CacheGranularity {
    /// Cache per function/expression — for files with localized edits
    FineGrained,
    /// Cache per module — for small files or stable code
    Module,
    /// Cache whole-file result — for high-churn files during refactoring
    CoarseGrained,
}

pub struct EditHistory {
    /// Recent edit locations (line ranges)
    recent_edits: Vec<(Instant, std::ops::Range<usize>)>,
    /// Total number of edits in the last N minutes
    edit_count: u32,
    /// Whether edits are concentrated in one area or spread across the file
    edit_concentration: f64, // 0.0 = spread out, 1.0 = concentrated
}

impl GranularityMonitor {
    /// Observe an edit and update granularity
    pub fn observe_edit(&self, path: &Path, range: std::ops::Range<usize>) {
        let mut history = self.edit_history.entry(path.to_path_buf()).or_default();
        history.recent_edits.push((Instant::now(), range));
        history.edit_count += 1;

        // Recompute concentration
        history.edit_concentration = compute_concentration(&history.recent_edits);

        // Adapt granularity
        let new_granularity = if history.edit_count > 20 && history.edit_concentration < 0.3 {
            // High churn, spread across file → zoom out to avoid overhead
            CacheGranularity::CoarseGrained
        } else if history.edit_concentration > 0.7 {
            // Concentrated edits → zoom in for fine-grained caching
            CacheGranularity::FineGrained
        } else {
            CacheGranularity::Module
        };

        self.granularity.insert(path.to_path_buf(), new_granularity);
    }
}
```

**Integration**: The `QueryContext` consults `GranularityMonitor` when deciding query key granularity. In `FineGrained` mode, queries are per-function. In `CoarseGrained` mode, queries are per-file.

### 4.4 Implementation Steps for Phase 1

| Step | Task | Estimated Effort |
|---|---|---|
| 1.1 | Implement `SemanticNormalizer` in `glyim-hir` | 3-4 days |
| 1.2 | Integrate semantic hashing into query keys | 1-2 days |
| 1.3 | Create `glyim-merkle` crate with `MerkleNode`, `MerkleStore` | 3-4 days |
| 1.4 | Integrate Merkle store with existing CAS infrastructure | 2-3 days |
| 1.5 | Implement branch-switching logic (root hash lookup) | 1-2 days |
| 1.6 | Implement `GranularityMonitor` | 2-3 days |
| 1.7 | Integrate granularity into query key computation | 2-3 days |
| 1.8 | Add `--cache-branch-agnostic` CLI flag | 1 day |
| 1.9 | Write tests | 3-4 days |
| **Total** | | **~18-26 days** |

### 4.5 Success Criteria for Phase 1

- Auto-formatting a 500-line file triggers zero recompilation
- Switching between two Git branches that share 90% of code only recompiles the 10% that differs
- The compiler automatically adjusts between fine-grained and coarse-grained caching based on edit patterns
- Semantic hash remains stable across variable renames and comment changes

---

## 5. Phase 2 — JIT Live Compiler & Micro-Modules

**Goal**: Transform the JIT path from a single monolithic module into a micro-module architecture with OrcV2 lazy reexports, double-buffered dylib swapping, and a tier-0 interpreter.

### 5.1 Lego-Block Micro-Module Architecture

**Modify**: `crates/glyim-codegen-llvm/src/codegen/mod.rs`

Currently, `Codegen` creates one `inkwell::Module` per compilation. We need to partition code into micro-modules (one per top-level item or per file), enabling independent compilation and hot-swapping.

```rust
/// Micro-module manager for incremental JIT compilation
pub struct MicroModuleManager<'ctx> {
    /// The LLVM context (shared across all modules)
    context: &'ctx Context,
    /// One module per source item
    modules: HashMap<Symbol, inkwell::module::Module<'ctx>>,
    /// Module dependencies (which module calls which)
    module_deps: HashMap<Symbol, HashSet<Symbol>>,
    /// The JIT execution session
    execution_session: ExecutionSession<'ctx>,
    /// JIT dylibs: one per micro-module
    dylibs: HashMap<Symbol, JITDylib<'ctx>>,
    /// Global function pointer table for cross-module calls
    dispatch_table: Arc<DispatchTable>,
}

impl<'ctx> MicroModuleManager<'ctx> {
    /// Compile a single HIR item into its own LLVM module
    pub fn compile_item(&mut self, item: &HirItem, mono_result: &MonoResult) -> Result<(), CodegenError> {
        let module_name = format!("glyim_item_{}", item.name());
        let module = self.context.create_module(&module_name);

        // Create a dedicated codegen for this module
        let mut item_codegen = Codegen::new_for_module(module, self.target_triple.clone());

        // Compile just this item
        item_codegen.codegen_item(item, mono_result)?;

        // Create a JITDylib for this module
        let dylib = self.execution_session.create_dylib(&module_name)?;

        // Add the module to the dylib using lazy reexports
        self.add_with_lazy_reexports(&module, &dylib, item.name())?;

        self.modules.insert(item.name(), module);
        self.dylib.insert(item.name(), dylib);
        Ok(())
    }

    /// Add a module with lazy reexports for cross-module calls
    fn add_with_lazy_reexports(
        &mut self,
        module: &inkwell::module::Module<'ctx>,
        dylib: &JITDylib<'ctx>,
        item_name: Symbol,
    ) -> Result<(), CodegenError> {
        // Use OrcV2's IRCompileLayer with lazy compilation
        // Cross-module calls go through the dispatch table (indirect jumps)
        // The actual target is resolved on first call (lazy materialization)
        // ...
    }

    /// Rebuild a single item after a source change
    pub fn rebuild_item(&mut self, item: &HirItem, mono_result: &MonoResult) -> Result<(), CodegenError> {
        // 1. Remove the old module and dylib for this item
        self.modules.remove(&item.name());
        self.dylib.remove(&item.name());

        // 2. Compile the new version into a fresh module/dylib
        self.compile_item(item, mono_result)?;

        // 3. Update the dispatch table pointer atomically
        let new_address = self.resolve_symbol(item.name())?;
        self.dispatch_table.update(item.name(), new_address);

        Ok(())
    }
}
```

### 5.2 Double-Buffered JIT Dylibs (Zero-Downtime Swapping)

**New file**: `crates/glyim-codegen-llvm/src/live.rs`

```rust
/// Double-buffered JIT execution: Active and Staging dylibs
pub struct DoubleBufferedJIT<'ctx> {
    /// The currently active dylib (being executed)
    active: JITDylib<'ctx>,
    /// The staging dylib (being compiled into)
    staging: Option<JITDylib<'ctx>>,
    /// Global function pointer table
    dispatch: Arc<DispatchTable>,
    /// Compilation thread handle
    compile_thread: Option<JoinHandle<()>>,
}

/// Thread-safe function pointer dispatch table
pub struct DispatchTable {
    /// Maps function names to their current machine code addresses
    pointers: DashMap<Symbol, AtomicUsize>,
}

impl DispatchTable {
    /// Get the current address for a function
    pub fn get_address(&self, name: Symbol) -> usize {
        self.pointers.get(&name).map(|p| p.load(Ordering::Acquire)).unwrap_or(0)
    }

    /// Atomically swap a function's pointer to a new address
    pub fn update(&self, name: Symbol, new_address: usize) {
        if let Some(ptr) = self.pointers.get(&name) {
            ptr.store(new_address, Ordering::Release);
        } else {
            self.pointers.insert(name, AtomicUsize::new(new_address));
        }
    }
}

impl<'ctx> DoubleBufferedJIT<'ctx> {
    /// Start background compilation of changed items
    pub fn stage_changes(&mut self, changed_items: Vec<HirItem>, mono: &MonoResult) {
        // 1. Create a new staging dylib
        let staging = self.execution_session.create_dylib("glyim_staging")?;

        // 2. Spawn a background thread to compile changed items into the staging dylib
        let dispatch = self.dispatch.clone();
        self.compile_thread = Some(std::thread::spawn(move || {
            for item in changed_items {
                // Compile item into staging dylib
                let new_address = compile_into_dylib(&staging, &item, mono);
                // Update dispatch table atomically
                dispatch.update(item.name(), new_address);
            }
        }));

        self.staging = Some(staging);
    }

    /// Check if background compilation is complete
    pub fn is_staging_ready(&self) -> bool {
        self.compile_thread.as_ref().map_or(true, |t| t.is_finished())
    }

    /// Promote staging to active (atomic swap)
    pub fn promote_staging(&mut self) {
        if let Some(staging) = self.staging.take() {
            let old_active = std::mem::replace(&mut self.active, staging);
            // Old active is dropped; its memory is freed
            drop(old_active);
        }
    }
}
```

### 5.3 Tier-0 Bytecode Interpreter (Sub-Millisecond Feedback)

**New crate**: `crates/glyim-bytecode/`

For instant feedback during editing, compile HIR to a custom bytecode instead of LLVM IR. The interpreter runs in microseconds. When code stabilizes, promote to LLVM JIT.

```rust
/// Glyim bytecode instruction set
pub enum Bytecode {
    /// Push a literal integer
    PushI64(i64),
    /// Push a literal float
    PushF64(f64),
    /// Push a literal bool
    PushBool(bool),
    /// Load local variable by index
    LoadLocal(u32),
    /// Store local variable by index
    StoreLocal(u32),
    /// Binary operation
    BinOp(BinOpKind),
    /// Call function by dispatch table index
    Call(u32),
    /// Return from function
    Return,
    /// Jump
    Jump(u32),
    /// Jump if false
    JumpIfFalse(u32),
    /// Allocate struct
    AllocStruct { field_count: u32 },
    /// Access struct field
    FieldAccess { index: u32 },
    // ... etc.
}

/// Compiled bytecode function
pub struct BytecodeFn {
    pub instructions: Vec<Bytecode>,
    pub local_count: u32,
    pub param_count: u32,
}

/// The bytecode compiler: HIR → Bytecode
pub struct BytecodeCompiler {
    fns: HashMap<Symbol, BytecodeFn>,
}

impl BytecodeCompiler {
    pub fn compile_hir(hir: &Hir) -> HashMap<Symbol, BytecodeFn> {
        // Walk each HirFn, emit bytecode instructions
        // Much simpler than LLVM IR emission — no SSA, no types at this level
    }
}

/// The bytecode interpreter
pub struct BytecodeInterpreter {
    /// Stack for computation
    stack: Vec<Value>,
    /// Function dispatch table (can point to bytecode or JIT functions)
    dispatch: Arc<DispatchTable>,
}

impl BytecodeInterpreter {
    /// Execute a bytecode function
    pub fn execute(&mut self, fn_name: Symbol, args: &[Value]) -> Value {
        let bc_fn = &self.bytecode[&fn_name];
        // Simple stack-based interpreter loop
        // ~1-5 microseconds per function call
    }
}
```

### 5.4 Tiered Compilation Manager

**New file**: `crates/glyim-codegen-llvm/src/tiered.rs`

```rust
/// Manages the transition between Tier-0 (bytecode) and Tier-1 (LLVM JIT)
pub struct TieredCompiler<'ctx> {
    /// Tier-0 interpreter
    interpreter: BytecodeInterpreter,
    /// Tier-1 LLVM JIT
    jit: DoubleBufferedJIT<'ctx>,
    /// Execution counters per function
    execution_counts: DashMap<Symbol, u64>,
    /// Promotion threshold
    promotion_threshold: u64, // e.g., 100 executions
    /// Time since last edit
    idle_timer: Instant,
    /// Idle threshold for batch promotion
    idle_threshold: Duration, // e.g., 500ms
}

impl<'ctx> TieredCompiler<'ctx> {
    /// Execute a function, starting in Tier-0 and promoting to Tier-1 when hot
    pub fn execute(&mut self, fn_name: Symbol, args: &[Value]) -> Value {
        // Increment execution counter
        *self.execution_counts.entry(fn_name).or_insert(0) += 1;

        // Check if we should promote to Tier-1
        let count = self.execution_counts[&fn_name];
        if count >= self.promotion_threshold {
            // Promote: compile with LLVM and update dispatch table
            self.promote_function(fn_name);
        }

        // Execute via dispatch table (automatically routes to bytecode or JIT)
        self.dispatch.execute(fn_name, args)
    }

    /// Background: promote all recently-edited functions after idle period
    pub fn promote_idle(&mut self) {
        if self.idle_timer.elapsed() > self.idle_threshold {
            for (name, count) in self.execution_counts.iter() {
                if *count.value() > 0 && !self.jit.is_compiled(*name.key()) {
                    self.promote_function(*name.key());
                }
            }
        }
    }
}
```

### 5.5 Stateful Hot-Patching (Live Code Patching)

**Modify**: `crates/glyim-codegen-llvm/src/live.rs`

The `DispatchTable` from §5.2 already enables hot-patching. We add a file-watcher integration:

```rust
/// Live code patcher that integrates with file watching
pub struct LivePatcher<'ctx> {
    tiered: TieredCompiler<'ctx>,
    watcher: FileWatcher,
    query_ctx: Arc<QueryContext>,
}

impl<'ctx> LivePatcher<'ctx> {
    /// Run the live patching loop
    pub async fn run(&mut self, entry_point: Symbol) -> ! {
        // 1. Initial compilation: Tier-0 bytecode for everything
        // 2. Execute the program's main loop
        // 3. When the watcher fires:
        //    a. Determine what changed (via query engine)
        //    b. Re-compile changed functions to Tier-0 bytecode (instant)
        //    c. Update dispatch table (atomic swap)
        //    d. In background: promote to Tier-1 LLVM JIT
        //    e. The running program picks up new code on next call
        loop {
            tokio::select! {
                // Run user code
                _ = self.tiered.execute(entry_point, &[]) => {},
                // Watch for file changes
                Some(changes) = self.watcher.next_change() => {
                    self.handle_changes(changes);
                }
            }
        }
    }

    fn handle_changes(&mut self, changes: Vec<PathBuf>) {
        // 1. Invalidate queries for changed files
        let report = self.query_ctx.invalidate(
            &changes.iter().map(|p| Dependency::File { path: p.clone(), hash: /* new hash */ }).collect::<Vec<_>>()
        );

        // 2. Re-compile only the red queries
        for red_query in report.red_queries {
            // Re-compile to bytecode (instant)
            let bytecodes = self.bytecode_compiler.recompile_query(&red_query);
            // Update dispatch table
            for (name, bc_fn) in bytecodes {
                self.tiered.update_bytecode(name, bc_fn);
            }
        }
    }
}
```

### 5.6 AST-Diffing to IR-Diffing (The "Stitcher")

**New file**: `crates/glyim-codegen-llvm/src/stitcher.rs`

For small changes within a function, patch the LLVM IR in-place instead of rebuilding the entire function:

```rust
/// Patches LLVM IR in-place based on AST diffs
pub struct IrStitcher<'ctx> {
    /// Maps source locations to LLVM BasicBlocks
    source_map: HashMap<SourceSpan, BasicBlock<'ctx>>,
    /// Maps source locations to LLVM instructions
    instruction_map: HashMap<SourceSpan, InstructionValue<'ctx>>,
}

impl<'ctx> IrStitcher<'ctx> {
    /// Apply a diff to an existing LLVM function
    pub fn patch_function(
        &mut self,
        fn_value: FunctionValue<'ctx>,
        builder: &Builder<'ctx>,
        diff: &AstDiff,
    ) -> Result<(), StitchError> {
        for hunk in &diff.hunks {
            match hunk.kind {
                DiffKind::Insert(span, new_expr) => {
                    // Position builder at the end of the BasicBlock before the insert point
                    let bb = self.source_map[&span.before()];
                    builder.position_at_end(bb);
                    // Emit the new expression's IR
                    let new_value = self.codegen_expr(builder, new_expr)?;
                    // Update the instruction map
                    self.instruction_map.insert(span.clone(), new_value.as_instruction_value().unwrap());
                }
                DiffKind::Delete(span) => {
                    // Replace the deleted instruction with `undef` or a no-op
                    let instr = self.instruction_map[&span];
                    instr.erase_from_basic_block();
                    // Or: replace all uses with undef
                }
                DiffKind::Replace(span, new_expr) => {
                    // Delete old, insert new (in-place replacement)
                    let old_instr = self.instruction_map[&span];
                    let bb = old_instr.get_parent().unwrap();
                    builder.position_at_end(bb);
                    let new_value = self.codegen_expr(builder, new_expr)?;
                    old_instr.replace_all_uses_with(new_value);
                    old_instr.erase_from_basic_block();
                }
            }
        }
        // Verify the function is still valid
        if fn_value.verify(true) {
            Ok(())
        } else {
            Err(StitchError::InvalidIr)
        }
    }
}
```

**Caveat**: SSA form makes in-place patching tricky for changes that affect PHI nodes or control flow. The stitcher falls back to full function rebuild for complex changes (control flow restructuring, new variables). It only patches simple expression changes.

### 5.7 Pre-Optimized Bitcode Foundation

**Modify**: `crates/glyim-codegen-llvm/src/codegen/mod.rs`

On first startup, compile the standard library to optimized bitcode and cache it:

```rust
/// Manages pre-compiled standard library bitcode
pub struct StdlibBitcodeCache<'ctx> {
    /// Pre-optimized modules, keyed by target triple
    cached_modules: HashMap<String, inkwell::module::Module<'ctx>>,
    /// CAS store for persistence
    cas: Arc<dyn ContentStore>,
}

impl<'ctx> StdlibBitcodeCache<'ctx> {
    /// Load or compile the standard library
    pub fn load_or_compile(
        context: &'ctx Context,
        target: &str,
        opt_level: OptimizationLevel,
        cas: &Arc<dyn ContentStore>,
    ) -> Self {
        let cache_key = format!("stdlib-{}-{:?}", target, opt_level);
        let cache_hash = ContentHash::hash(cache_key.as_bytes());

        // Try to load from CAS
        if let Some(data) = cas.retrieve(&cache_hash).ok() {
            let memory_buffer = MemoryBuffer::create_from_memory_range(&data, "stdlib");
            let module = Module::parse_bitcode_from_buffer(&memory_buffer, context).unwrap();
            // Skip optimization — already optimized!
            return Self { cached_modules: hashmap! { target.to_string() => module }, cas: cas.clone() };
        }

        // Not cached — compile the stdlib
        let module = Self::compile_stdlib(context, target, opt_level);
        // Cache the optimized bitcode
        let bitcode = module.write_bitcode_to_memory().as_slice().to_vec();
        cas.store(&cache_hash, &bitcode).unwrap();

        Self { cached_modules: hashmap! { target.to_string() => module }, cas: cas.clone() }
    }
}
```

### 5.8 Implementation Steps for Phase 2

| Step | Task | Estimated Effort |
|---|---|---|
| 2.1 | Refactor `Codegen` to support per-item module compilation | 4-5 days |
| 2.2 | Implement `MicroModuleManager` with OrcV2 integration | 5-6 days |
| 2.3 | Implement `DoubleBufferedJIT` and `DispatchTable` | 4-5 days |
| 2.4 | Create `glyim-bytecode` crate (compiler + interpreter) | 5-6 days |
| 2.5 | Implement `TieredCompiler` with automatic promotion | 3-4 days |
| 2.6 | Implement `LivePatcher` with file watcher integration | 3-4 days |
| 2.7 | Implement `IrStitcher` for in-place IR patching | 4-5 days |
| 2.8 | Implement `StdlibBitcodeCache` | 2-3 days |
| 2.9 | Update CLI `run` command to use micro-module JIT | 2-3 days |
| 2.10 | Write tests | 4-5 days |
| **Total** | | **~36-46 days** |

### 5.9 Success Criteria for Phase 2

- Changing one function in a 1000-line file only recompiles that function's LLVM module (~50ms)
- The UI never freezes: compilation happens on a background thread, dispatch table swaps atomically
- First edit → Tier-0 bytecode execution in <5ms
- Idle for 500ms → automatic promotion to native LLVM JIT
- Standard library loads from pre-optimized bitcode in <10ms
- Hot-patching: a running program picks up new code without restart

---

## 6. Phase 3 — E-Graph Middle-End & Algebraic Optimization

**Goal**: Add an e-graph based optimization layer between the HIR and LLVM lowering, enabling algebraic simplifications, semantic equivalence proofs, and optimization invariant caching.

### 6.1 Introduce `glyim-egraph` Crate

**New crate**: `crates/glyim-egraph/`

Use the `egg` crate (Rust e-graph library) as the foundation, defining a Glyim-specific e-graph language:

```toml
# crates/glyim-egraph/Cargo.toml
[dependencies]
egg = "0.6"
glyim-hir = { path = "../glyim-hir" }
glyim-interner = { path = "../glyim-interner" }
```

```rust
use egg::{EGraph, Rewrite, Runner, Applier, Var, Id, Language};

/// Glyim e-graph language — represents HIR expressions as e-nodes
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum GlyimExpr {
    /// Integer literal
    Num(i64),
    /// Float literal
    FNum(f64), // Wrapped in ordered float for hashing
    /// Boolean literal
    Bool(bool),
    /// Variable reference
    Var(Symbol),
    /// Binary operation: (+ a b), (- a b), etc.
    BinOp(BinOpKind, Id, Id),
    /// Unary operation
    UnOp(UnOpKind, Id),
    /// Function call: (call fn_name args...)
    Call(Symbol, Vec<Id>),
    /// If-then-else: (if cond then else)
    If(Id, Id, Id),
    /// Field access: (get obj field_name)
    Get(Id, Symbol),
    /// Method call: (method obj name args...)
    Method(Id, Symbol, Vec<Id>),
    /// Let binding: (let name value body)
    Let(Symbol, Id, Id),
}

/// HIR → E-Graph conversion
pub fn hir_fn_to_egraph(hir_fn: &HirFn) -> EGraph<GlyimExpr, GlyimAnalysis> {
    let mut egraph = EGraph::new(GlyimAnalysis::default());
    // Walk the HIR expression tree, adding each node to the e-graph
    hir_expr_to_egraph(&hir_fn.body, &mut egraph);
    egraph
}

/// Core rewrite rules for algebraic optimization
pub fn core_rewrites() -> Vec<Rewrite<GlyimExpr, GlyimAnalysis>> {
    vec![
        // Identity elimination
        rewrite!("add-zero"; "(+ a 0)" => "a"),
        rewrite!("mul-one"; "(* a 1)" => "a"),
        rewrite!("sub-zero"; "(- a 0)" => "a"),

        // Strength reduction
        rewrite!("mul-by-2"; "(* a 2)" => "(<< a 1)"),
        rewrite!("mul-by-power-2"; "(* a ?n)" => "(<< a ?shift)" if is_power_of_2("?n", "?shift")),

        // Commutativity (for reordering)
        rewrite!("add-comm"; "(+ a b)" => "(+ b a)"),
        rewrite!("mul-comm"; "(* a b)" => "(* b a)"),

        // Associativity
        rewrite!("add-assoc"; "(+ (+ a b) c)" => "(+ a (+ b c))"),

        // Double negation
        rewrite!("double-neg"; "(- (- a))" => "a"),

        // Constant folding (will be applied iteratively)
        // This is handled by the analysis's merge function

        // Identity removal for booleans
        rewrite!("and-true"; "(&& a true)" => "a"),
        rewrite!("or-false"; "(|| a false)" => "a"),
    ]
}

/// Analysis data stored in each e-class
#[derive(Default)]
pub struct GlyimAnalysis {
    /// Constant value (if the e-class represents a constant)
    constant: Option<ConstValue>,
    /// Whether the expression is pure (no side effects)
    is_pure: bool,
    /// Estimated cost (for extraction)
    cost: Option<f64>,
}

/// Run equality saturation on a function's e-graph
pub fn optimize_fn(hir_fn: &HirFn, rules: &[Rewrite<GlyimExpr, GlyimAnalysis>]) -> HirFn {
    let egraph = hir_fn_to_egraph(hir_fn);

    let runner = Runner::default()
        .with_egraph(egraph)
        .run(rules)
        .iter_rules()
        .for_each(|rule| { /* log rule application */ });

    let best = runner.egraph.extract(
        // Cost function: prefer fewer operations, prefer shifts over muls, etc.
        AstSizeCostFn
    );

    // Convert the extracted expression back to HIR
    egraph_expr_to_hir(&best)
}
```

### 6.2 E-Graph as Equivalent Mutant Pruner

The e-graph naturally supports proving that two expressions are semantically equivalent. This is the key to zero-overhead equivalent mutant pruning (Phase 7):

```rust
/// Check if two HIR expressions are semantically equivalent via e-graph
pub fn are_equivalent(expr_a: &HirExpr, expr_b: &HirExpr) -> bool {
    let mut egraph = EGraph::new(GlyimAnalysis::default());
    let id_a = hir_expr_to_egraph(expr_a, &mut egraph);
    let id_b = hir_expr_to_egraph(expr_b, &mut egraph);

    // Run equality saturation with our rewrite rules
    let runner = Runner::default()
        .with_egraph(egraph)
        .run(&core_rewrites())
        .with_node_limit(10000)
        .with_time_limit(Duration::from_millis(50));

    // If both expressions are in the same e-class, they are equivalent
    id_a == id_b || runner.egraph.find(id_a) == runner.egraph.find(id_b)
}
```

### 6.3 Optimization Invariant Certificates

**New file**: `crates/glyim-egraph/src/invariant.rs`

Cache the *properties* of code, not just the code itself. If optimization invariants haven't changed, skip re-optimization:

```rust
/// Certificate summarizing the optimization-relevant properties of a function
#[derive(Serialize, Deserialize, Hash, PartialEq, Eq)]
pub struct InvariantCertificate {
    /// Function signature hash (types of params + return)
    pub signature_hash: ContentHash,
    /// Whether the function is pure (no side effects)
    pub is_pure: bool,
    /// Whether the function may panic
    pub may_panic: bool,
    /// Whether the function allocates
    pub may_allocate: bool,
    /// Cyclomatic complexity
    pub complexity: u32,
    /// Size in IR instructions (approximate)
    pub ir_size: u32,
    /// Which functions this one calls
    pub callees: Vec<Symbol>,
    /// Which functions inline into this one (and their invariant hashes)
    pub inlined_from: Vec<(Symbol, ContentHash)>,
    /// The e-graph canonical form hash
    pub canonical_form_hash: ContentHash,
}

impl InvariantCertificate {
    /// Compute an invariant certificate for a function
    pub fn compute(hir_fn: &HirFn, effect_info: &EffectInfo) -> Self {
        Self {
            signature_hash: ContentHash::hash(&hir_fn.signature()),
            is_pure: effect_info.is_pure(hir_fn.name),
            may_panic: effect_info.may_panic(hir_fn.name),
            may_allocate: effect_info.may_allocate(hir_fn.name),
            complexity: compute_cyclomatic_complexity(hir_fn),
            ir_size: count_ir_instructions(hir_fn),
            callees: extract_callees(hir_fn),
            inlined_from: vec![], // Populated during optimization
            canonical_form_hash: compute_canonical_hash(hir_fn),
        }
    }

    /// If this certificate matches a cached one, the optimization result is reusable
    pub fn matches_cached(&self, cached: &InvariantCertificate) -> bool {
        self == cached
    }
}
```

**Integration**: After the e-graph optimization pass, store the `InvariantCertificate` alongside the optimized IR in the Merkle store. On the next build, compute the certificate for the new HIR. If it matches the cached certificate, skip LLVM optimization entirely.

### 6.4 Effect System

**New file**: `crates/glyim-hir/src/effects.rs`

Annotate every function with effect information:

```rust
/// Effects that a function may have
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct EffectSet {
    pub may_read: bool,      // Reads external state
    pub may_write: bool,     // Writes external state
    pub may_allocate: bool,  // Allocates memory
    pub may_panic: bool,     // May panic / throw
    pub may_diverge: bool,   // May not terminate (infinite loop)
    pub is_pure: bool,       // No effects at all
}

/// Effect analysis: infers effects for every function
pub struct EffectAnalyzer {
    /// Per-function inferred effects
    effects: HashMap<Symbol, EffectSet>,
}

impl EffectAnalyzer {
    /// Analyze effects for all functions in a HIR
    pub fn analyze(hir: &Hir) -> Self {
        // Bottom-up analysis:
        // 1. Start with known effects for builtins (print → writes, malloc → allocates)
        // 2. For each function:
        //    - If it calls a function with effect X, it may have effect X
        //    - If it contains a while loop without a guaranteed break, may_diverge
        //    - If it has only pure operations, is_pure
        // 3. Iterate to fixpoint (effects propagate upward through call graph)
    }

    /// Is this function pure? (Safe for CSE, LICM, etc.)
    pub fn is_pure(&self, name: Symbol) -> bool {
        self.effects.get(&name).map_or(false, |e| e.is_pure)
    }

    /// Can two functions be safely executed in parallel?
    pub fn can_parallelize(&self, fn_a: Symbol, fn_b: Symbol) -> bool {
        let a = self.effects.get(&fn_a);
        let b = self.effects.get(&fn_b);
        // Safe if at most one writes, and neither has data races
        // Simplification: safe if both are pure, or if only one has any effects
        matches!((a, b), (Some(a), Some(b)) if a.is_pure || b.is_pure || (!a.may_write && !b.may_write))
    }
}
```

**Integration with type checker**: After `type_check`, run `EffectAnalyzer::analyze()`. The effect information feeds into:
- The query engine (pure functions have more stable fingerprints)
- The e-graph optimizer (pure expressions can be freely reordered)
- The test runner (pure tests can be parallelized without locks)
- LLVM hints (pure functions get `readonly` attribute, noalias, etc.)

### 6.5 Implementation Steps for Phase 3

| Step | Task | Estimated Effort |
|---|---|---|
| 3.1 | Create `glyim-egraph` crate with `egg` dependency | 1 day |
| 3.2 | Define `GlyimExpr` language and HIR↔e-graph conversion | 4-5 days |
| 3.3 | Implement core algebraic rewrite rules | 3-4 days |
| 3.4 | Implement cost function and extraction | 2-3 days |
| 3.5 | Implement `InvariantCertificate` computation and caching | 3-4 days |
| 3.6 | Implement `EffectAnalyzer` in `glyim-hir` | 4-5 days |
| 3.7 | Integrate e-graph pass into the query pipeline (between type check and LLVM lowering) | 3-4 days |
| 3.8 | Add LLVM metadata hints from effect analysis | 2-3 days |
| 3.9 | Wire invariant certificates into the Merkle store | 2-3 days |
| 3.10 | Write tests | 4-5 days |
| **Total** | | **~28-37 days** |

### 6.6 Success Criteria for Phase 3

- The e-graph optimizer can prove `x + 0 ≡ x`, `x * 1 ≡ x`, and `x * 2 ≡ x << 1`
- Running an auto-formatter triggers zero recompilation (semantic hash unchanged)
- Pure functions are automatically annotated with `readonly` in LLVM IR
- Functions whose `InvariantCertificate` hasn't changed skip LLVM optimization
- Equivalent mutant pruning eliminates at least 50% of trivially equivalent mutants

---

## 7. Phase 4 — Speculative & Profile-Guided Optimization

**Goal**: Add speculative pre-compilation, profile-guided optimization, JIT→AOT feedback, and guarded type specialization.

### 7.1 Speculative Pre-Compilation (Predictive Caching)

**New file**: `crates/glyim-compiler/src/speculative.rs`

Observe developer behavior and pre-compile predicted next edits:

```rust
/// Tracks edit patterns and predicts likely next edits
pub struct EditPredictor {
    /// Sequence of recent edits (file, timestamp, AST region)
    edit_history: VecDeque<EditEvent>,
    /// Transition matrix: after editing function A, probability of editing function B
    transitions: HashMap<Symbol, HashMap<Symbol, f64>>,
    /// Minimum observations before making predictions
    min_observations: usize,
}

pub struct EditEvent {
    file: PathBuf,
    function: Symbol,
    timestamp: Instant,
    edit_type: EditType,
}

impl EditPredictor {
    /// Record an edit event
    pub fn observe(&mut self, event: EditEvent) {
        // Update transition matrix
        if let Some(prev) = self.edit_history.back() {
            let transitions = self.transitions.entry(prev.function).or_default();
            let count = transitions.entry(event.function).or_insert(0.0);
            *count += 1.0;
            // Normalize
            let total: f64 = transitions.values().sum();
            for v in transitions.values_mut() {
                *v /= total;
            }
        }
        self.edit_history.push_back(event);
    }

    /// Predict which functions are likely to be edited next
    pub fn predict(&self, current: Symbol, top_k: usize) -> Vec<(Symbol, f64)> {
        self.transitions.get(&current)
            .map(|t| {
                let mut sorted: Vec<_> = t.iter().collect();
                sorted.sort_by(|a, b| b.1.partial_cmp(a.1).unwrap());
                sorted.into_iter().take(top_k).map(|(s, p)| (*s, *p)).collect()
            })
            .unwrap_or_default()
    }

    /// Pre-compile predicted functions in the background
    pub fn speculative_compile(
        &self,
        current: Symbol,
        tiered: &mut TieredCompiler,
        query_ctx: &QueryContext,
    ) {
        let predictions = self.predict(current, 3);
        for (fn_name, probability) in predictions {
            if probability > 0.5 {
                // Clone the query context (cheap — it's just pointers + CAS lookups)
                let ctx = query_ctx.clone();
                let name = fn_name;
                std::thread::spawn(move || {
                    // Compile to LLVM IR in background
                    // Store in staging dylib, ready for atomic swap
                    if let Ok(module) = compile_fn_to_module(&ctx, name) {
                        tiered.stage_speculative(name, module);
                    }
                });
            }
        }
    }
}
```

### 7.2 Persistent Runtime Profiles (JIT → AOT Feedback)

**New crate**: `crates/glyim-profile/`

```rust
/// Runtime profile data collected during JIT execution
#[derive(Serialize, Deserialize)]
pub struct RuntimeProfile {
    /// Per-function execution counts
    pub fn_counts: HashMap<Symbol, u64>,
    /// Per-branch taken/not-taken counts
    pub branch_counts: HashMap<SourceSpan, (u64, u64)>,
    /// Per-call-site observed types (for specialization)
    pub observed_types: HashMap<SourceSpan, Vec<TypeObservation>>,
    /// Hot call graph edges
    pub hot_edges: HashMap<(Symbol, Symbol), u64>,
    /// Per-function allocation counts
    pub alloc_counts: HashMap<Symbol, u64>,
}

#[derive(Serialize, Deserialize)]
pub struct TypeObservation {
    pub observed_type: HirType,
    pub count: u64,
}

/// Profile collector — instrumented into JIT code
pub struct ProfileCollector {
    profile: Arc<RwLock<RuntimeProfile>>,
}

impl ProfileCollector {
    /// Instrument a function for profiling during LLVM codegen
    pub fn instrument_fn(&self, fn_value: FunctionValue, fn_name: Symbol) -> FunctionValue {
        // Insert counter increment at function entry
        // Insert branch counters at each conditional
        // Insert type observation points at each call site
        // These write to shared memory (the profile Arc)
    }

    /// Save profile to disk
    pub fn save(&self, path: &Path) -> Result<()> {
        let profile = self.profile.read().unwrap();
        let json = serde_json::to_string(&*profile)?;
        std::fs::write(path, json)?;
        Ok(())
    }

    /// Load profile from disk (for AOT builds)
    pub fn load(path: &Path) -> Result<RuntimeProfile> {
        let json = std::fs::read_to_string(path)?;
        Ok(serde_json::from_str(&json)?)
    }
}
```

### 7.3 Guarded Type/Shape Specialization

**Modify**: `crates/glyim-codegen-llvm/src/codegen/mod.rs`

For hot functions observed with specific argument types, emit specialized versions with guards:

```rust
/// Generates guarded specialized versions of hot functions
pub struct SpecializationGenerator<'ctx> {
    codegen: &'ctx Codegen<'ctx>,
    profile: &'ctx RuntimeProfile,
}

impl<'ctx> SpecializationGenerator<'ctx> {
    /// Generate a specialized version of a function for observed types
    pub fn specialize(
        &self,
        fn_name: Symbol,
        observed_types: &[HirType],
    ) -> Result<SpecializedFunction<'ctx>, CodegenError> {
        // 1. Create a new function with the specialized name: fn_name__i32
        let specialized_name = format!("{}__{}", fn_name, type_suffix(observed_types));
        let specialized_fn = self.codegen.declare_fn(&specialized_name, /* specialized signature */)?;

        // 2. Emit the function body with concrete types (no generic dispatch)
        self.codegen.codegen_fn_body(&specialized_fn, /* specialized HIR */)?;

        // 3. Create a guard function that dispatches based on runtime type
        let guard_fn = self.create_guard(fn_name, &specialized_name, observed_types)?;

        Ok(SpecializedFunction {
            original: fn_name,
            specialized_name,
            guard_fn,
        })
    }

    /// Create a guard that checks types and dispatches
    fn create_guard(
        &self,
        fn_name: Symbol,
        specialized_name: &str,
        expected_types: &[HirType],
    ) -> Result<FunctionValue<'ctx>, CodegenError> {
        // Pseudocode for the guard:
        // fn fn_name_guard(args...) {
        //     if type_of(args[0]) == ExpectedType {
        //         return specialized_name(args...)  // fast path
        //     } else {
        //         return fn_name_generic(args...)    // slow path
        //     }
        // }
    }
}
```

### 7.4 Profile-Guided E-Graph Rewrites

**Modify**: `crates/glyim-egraph/src/`

Annotate e-classes with profile data and prefer rewrites that benefit hot paths:

```rust
/// Extended analysis that incorporates profile data
pub struct ProfileGuidedAnalysis {
    /// Base analysis
    base: GlyimAnalysis,
    /// Execution frequency (from JIT profile)
    frequency: u64,
    /// Observed types at this expression (from JIT profile)
    observed_types: Vec<HirType>,
}

/// Profile-guided cost function for extraction
pub struct ProfileGuidedCostFn<'a> {
    profile: &'a RuntimeProfile,
}

impl<'a> egg::CostFunction<GlyimExpr> for ProfileGuidedCostFn<'a> {
    type Cost = f64;

    fn cost<C>(&mut self, enode: &GlyimExpr, mut costs: C) -> Self::Cost
    where
        C: FnMut(Id) -> Self::Cost,
    {
        let base_cost = match enode {
            GlyimExpr::Num(_) | GlyimExpr::Bool(_) => 1.0,
            GlyimExpr::BinOp(op, ..) => match op {
                BinOpKind::Mul => 3.0,
                BinOpKind::Div => 10.0,
                BinOpKind::Shl => 1.0,
                _ => 1.0,
            },
            _ => 1.0,
        };

        // Weight by execution frequency
        let frequency = self.get_frequency(enode);
        base_cost * (1.0 + (frequency as f64).log2())
    }
}
```

### 7.5 Speculative Trace Building

**New file**: `crates/glyim-compiler/src/tracing.rs`

For long-running loops, build linear traces and compile them as optimized LLVM functions:

```rust
/// A linear trace through a hot loop
pub struct Trace {
    /// Sequence of operations in the trace
    ops: Vec<TraceOp>,
    /// Guards that must hold for the trace to be valid
    guards: Vec<TraceGuard>,
    /// Loop variable initial values
    loop_vars: HashMap<Symbol, Value>,
}

pub enum TraceOp {
    BinOp(BinOpKind, Value, Value),
    LoadLocal(Symbol),
    StoreLocal(Symbol, Value),
    Call(Symbol, Vec<Value>),
    GuardCheck(TraceGuard),
}

pub enum TraceGuard {
    TypeCheck { var: Symbol, expected: HirType },
    BoundsCheck { array: Symbol, index: Value, length: Value },
    NotNull { ptr: Value },
}

/// Trace recorder: observes execution in the bytecode interpreter and records traces
pub struct TraceRecorder {
    /// Hot loop headers and their back-edge counts
    hot_loops: HashMap<SourceSpan, u64>,
    /// Recorded traces
    traces: Vec<Trace>,
    /// Threshold for starting trace recording
    hot_threshold: u64,
}

impl TraceRecorder {
    /// Called on each back-edge in the interpreter
    pub fn observe_back_edge(&mut self, loop_header: SourceSpan) {
        let count = self.hot_loops.entry(loop_header).or_insert(0);
        *count += 1;
        if *count == self.hot_threshold {
            self.start_recording(loop_header);
        }
    }

    /// Compile a recorded trace to LLVM IR with guards
    pub fn compile_trace(&self, trace: &Trace, codegen: &Codegen) -> FunctionValue {
        // 1. Create a linear LLVM function
        // 2. Emit guard checks at the top (if any guard fails, deoptimize)
        // 3. Emit the trace operations as straight-line code
        // 4. Inline any called functions that are small and pure
    }
}
```

### 7.6 Implementation Steps for Phase 4

| Step | Task | Estimated Effort |
|---|---|---|
| 4.1 | Create `glyim-profile` crate | 2-3 days |
| 4.2 | Implement `ProfileCollector` with JIT instrumentation | 4-5 days |
| 4.3 | Implement profile persistence and loading | 2-3 days |
| 4.4 | Implement `EditPredictor` for speculative pre-compilation | 3-4 days |
| 4.5 | Integrate speculative compilation into `LivePatcher` | 2-3 days |
| 4.6 | Implement `SpecializationGenerator` for guarded type specialization | 4-5 days |
| 4.7 | Implement OSR (on-stack replacement) for loop hot-paths | 5-6 days |
| 4.8 | Implement `TraceRecorder` and trace compilation | 5-6 days |
| 4.9 | Add profile-guided cost function to e-graph extraction | 2-3 days |
| 4.10 | Implement `--profile` CLI flag (enable/disable profiling) | 1-2 days |
| 4.11 | Write tests | 4-5 days |
| **Total** | | **~34-45 days** |

### 7.7 Success Criteria for Phase 4

- JIT runs automatically collect execution profiles and save them to disk
- `glyim build --release` loads JIT profiles and uses them for PGO-guided optimization
- The compiler predicts which function the developer will edit next with >60% accuracy
- Hot functions are speculatively pre-compiled in background threads
- Long-running loops are traced and compiled to optimized LLVM IR with guards
- Type specialization reduces execution time by >30% for hot generic functions

---

## 8. Phase 5 — AOT Release Pipeline & ThinLTO Integration

**Goal**: Make AOT release builds as incremental and cache-friendly as CI/IDE builds, with ThinLTO, summary-driven linking, and multi-target projection caches.

### 8.1 Summary-Driven ThinLTO

**Modify**: `crates/glyim-codegen-llvm/`

Emit per-module summaries that guide ThinLTO's import decisions:

```rust
/// Language-level module summary for ThinLTO guidance
#[derive(Serialize, Deserialize)]
pub struct ModuleSummary {
    /// Module identifier
    pub module_id: Symbol,
    /// Exported function signatures
    pub exports: Vec<FunctionSummary>,
    /// Imported function names
    pub imports: Vec<Symbol>,
    /// Type definitions that are part of this module's ABI
    pub abi_types: Vec<TypeSummary>,
    /// Inlining hints (from effect analysis + profiles)
    pub inline_hints: Vec<InlineHint>,
    /// Total module size (for import cost estimation)
    pub total_ir_size: usize,
}

#[derive(Serialize, Deserialize)]
pub struct FunctionSummary {
    pub name: Symbol,
    pub signature: HirType,
    pub is_pure: bool,
    pub may_panic: bool,
    pub estimated_size: usize,
    pub hotness: Option<f64>, // From profile data
    pub inline_recommendation: InlineRecommendation,
}

#[derive(Serialize, Deserialize)]
pub enum InlineRecommendation {
    Always,    // Small and pure — always inline
    IfHot,     // Inline only if profile says it's hot
    Never,     // Large or recursive — never inline
    Default,   // Let LLVM decide
}

/// Generate ThinLTO summaries from a MonoResult
pub fn generate_thinlto_summaries(
    mono: &MonoResult,
    effects: &EffectAnalyzer,
    profile: Option<&RuntimeProfile>,
) -> Vec<ModuleSummary> {
    // Partition the MonoResult into logical modules (one per source file or per namespace)
    // For each module, compute:
    //   - Which functions it exports (public API)
    //   - Which functions it imports (external calls)
    //   - Purity and effect info for each function
    //   - Inline recommendations based on size + hotness
}
```

### 8.2 Sub-Function Granular CAS (Xcode 26-style)

**Modify**: `crates/glyim-codegen-llvm/`, `crates/glyim-merkle/`

Cache compiled artifacts at sub-function granularity:

```rust
/// Sub-function CAS cache key
#[derive(Hash, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubFunctionCacheKey {
    /// Hash of the source IR chunk (function body or hot region)
    pub ir_hash: ContentHash,
    /// Target triple
    pub target: String,
    /// Optimization level
    pub opt_level: u8,
    /// Compiler version
    pub compiler_version: String,
    /// Feature flags
    pub feature_flags: Vec<String>,
}

/// Sub-function CAS store
pub struct SubFunctionCas {
    /// Backed by the existing CAS infrastructure
    local: Arc<LocalContentStore>,
    /// Optional remote CAS
    remote: Option<Arc<RemoteContentStore>>,
}

impl SubFunctionCas {
    /// Look up a compiled artifact by key
    pub fn get(&self, key: &SubFunctionCacheKey) -> Option<CachedArtifact> {
        let hash = ContentHash::hash(&key);
        // Check local CAS first
        if let Some(data) = self.local.retrieve(&hash).ok() {
            return Some(CachedArtifact::deserialize(&data));
        }
        // Check remote CAS
        if let Some(remote) = &self.remote {
            if let Some(data) = remote.retrieve(&hash).ok() {
                // Cache locally for future hits
                self.local.store(&hash, &data).ok();
                return Some(CachedArtifact::deserialize(&data));
            }
        }
        None
    }

    /// Store a compiled artifact
    pub fn put(&self, key: &SubFunctionCacheKey, artifact: &CachedArtifact) {
        let hash = ContentHash::hash(&key);
        let data = artifact.serialize();
        self.local.store(&hash, &data).ok();
        if let Some(remote) = &self.remote {
            remote.store(&hash, &data).ok();
        }
    }
}

pub struct CachedArtifact {
    /// Optimized LLVM bitcode
    pub optimized_bitcode: Vec<u8>,
    /// Or pre-compiled object code (if we cached post-codegen)
    pub object_code: Option<Vec<u8>>,
    /// The invariant certificate that was valid when this was compiled
    pub certificate: InvariantCertificate,
}
```

### 8.3 Multi-Target Projection Cache

**Modify**: `crates/glyim-merkle/`

Structure cache keys so that cross-target builds share frontend work:

```rust
/// Multi-target cache key — separates frontend work from backend work
pub struct MultiTargetKey {
    /// Frontend IR hash (shared across targets)
    pub frontend_hash: ContentHash,
    /// Target triple (determines codegen)
    pub target: String,
    /// Target-specific flags
    pub features: Vec<String>,
    /// Optimization level
    pub opt_level: u8,
}

/// The projection cache: given a frontend hash, cache codegen per target
pub struct ProjectionCache {
    /// Maps frontend_hash → set of available target caches
    available_targets: DashMap<ContentHash, HashSet<String>>,
    /// The actual CAS
    cas: Arc<dyn ContentStore>,
}

impl ProjectionCache {
    /// When building for a new target, reuse all frontend work
    pub fn build_for_target(
        &self,
        frontend_hash: ContentHash,
        target: &str,
    ) -> Option<CachedArtifact> {
        // 1. Check if we already have codegen for this (frontend_hash, target)
        let key = MultiTargetKey { frontend_hash, target: target.to_string(), features: vec![], opt_level: 2 };
        let cache_hash = ContentHash::hash(&key);
        self.cas.retrieve(&cache_hash).ok().map(|data| CachedArtifact::deserialize(&data))
    }
}
```

### 8.4 Safe Partial LTO with Risk Profiles

**New file**: `crates/glyim-compiler/src/lto_profile.rs`

```rust
/// LTO risk profile per module
#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LtoRiskProfile {
    /// Fully participate in LTO: aggressive inlining, cross-module optimization
    Hot,
    /// Import from other modules but don't export internals
    Cold,
    /// Never participate in cross-module inlining (sealed boundary)
    Sealed,
}

/// Configure LTO based on risk profiles
pub struct LtoPlanner {
    /// Per-module risk profiles (from annotations or automatic inference)
    profiles: HashMap<Symbol, LtoRiskProfile>,
}

impl LtoPlanner {
    /// Plan the ThinLTO import sets based on risk profiles
    pub fn plan_imports(&self, summaries: &[ModuleSummary]) -> ThinLtoPlan {
        let mut plan = ThinLtoPlan::default();

        for summary in summaries {
            let profile = self.profiles.get(&summary.module_id).copied().unwrap_or(LtoRiskProfile::Cold);

            match profile {
                LtoRiskProfile::Hot => {
                    // Export everything, import aggressively
                    plan.export_all(summary.module_id);
                    plan.import_from_all(summary.module_id);
                }
                LtoRiskProfile::Cold => {
                    // Export public API only, import from Hot modules
                    plan.export_public_only(summary.module_id);
                    plan.import_from_hot_only(summary.module_id);
                }
                LtoRiskProfile::Sealed => {
                    // No cross-module optimization
                    plan.no_cross_module(summary.module_id);
                }
            }
        }

        plan
    }
}
```

### 8.5 JIT → AOT Cache Warming

**New CLI command**: `glyim build --warm-from-jit`

When the user has been running JIT builds, their profile data and compiled artifacts can warm the AOT cache:

```rust
/// Warm the AOT cache from JIT session data
pub fn warm_aot_cache_from_jit(
    jit_profile: &RuntimeProfile,
    jit_cache: &MerkleStore,
    aot_cache: &SubFunctionCas,
) -> WarmReport {
    let mut report = WarmReport::default();

    for (fn_name, count) in &jit_profile.fn_counts {
        if *count > 100 {
            // This function was hot in JIT — pre-populate AOT cache
            // Look up the function's IR in the JIT Merkle store
            // Compile for AOT target (potentially different from JIT target)
            // Store in AOT cache
            report.warmed_functions += 1;
        }
    }

    report
}
```

### 8.6 Implementation Steps for Phase 5

| Step | Task | Estimated Effort |
|---|---|---|
| 5.1 | Implement `ModuleSummary` generation | 3-4 days |
| 5.2 | Integrate ThinLTO with custom summaries into the build pipeline | 5-6 days |
| 5.3 | Implement `SubFunctionCas` with local + remote backends | 4-5 days |
| 5.4 | Implement `ProjectionCache` for multi-target builds | 3-4 days |
| 5.5 | Implement `LtoPlanner` with risk profiles | 3-4 days |
| 5.6 | Implement JIT→AOT cache warming | 2-3 days |
| 5.7 | Add `--lto-profile`, `--warm-from-jit`, `--target` CLI flags | 2-3 days |
| 5.8 | Integrate with existing `glyim-cas-server` for remote caching | 3-4 days |
| 5.9 | Write tests | 4-5 days |
| **Total** | | **~29-38 days** |

### 8.7 Success Criteria for Phase 5

- ThinLTO builds use language-level summaries for better import decisions
- Sub-function CAS achieves >80% cache hit rate on CI for unchanged functions
- Cross-target builds reuse all frontend work; only codegen differs
- LTO risk profiles reduce ThinLTO overhead by 40-60% for large projects
- `--warm-from-jit` reduces first AOT build time by >50% after a JIT session

---

## 9. Phase 6 — Integrated Test Runner Revolution

**Goal**: Transform the test runner from a batch executor into a live, incremental, effect-driven, profile-collecting system.

### 9.1 Real File Watcher

**Replace**: `crates/glyim-testr/src/watcher.rs`

The current `FileWatcher` is a stub. Replace it with a real implementation using `notify-debouncer-full` (already a dependency):

```rust
use notify_debouncer_full::{new_debouncer, notify::RecommendedWatcher, DebouncedEvent};

pub struct FileWatcher {
    /// The notify debouncer
    debouncer: notify_debouncer_full::Debouncer<RecommendedWatcher>,
    /// Channel to receive debounced events
    rx: mpsc::Receiver<Vec<DebouncedEvent>>,
    /// Debounce delay (e.g., 100ms to batch rapid saves)
    debounce_delay: Duration,
}

impl FileWatcher {
    pub fn new(paths: &[PathBuf], debounce: Duration) -> Result<Self, WatchError> {
        let (tx, rx) = mpsc::channel();
        let debouncer = new_debouncer(debounce, None, move |result| {
            if let Ok(events) = result {
                let _ = tx.send(events);
            }
        })?;

        for path in paths {
            debouncer.watcher().watch(path, RecursiveMode::Recursive)?;
        }

        Ok(Self { debouncer, rx, debounce_delay: debounce })
    }

    /// Wait for the next batch of file changes
    pub async fn next_change(&mut self) -> Vec<PathBuf> {
        loop {
            tokio::task::spawn_blocking(|| {
                self.rx.recv().ok()
            }).await.ok().flatten().map(|events| {
                events.into_iter()
                    .filter_map(|e| match e.kind {
                        EventKind::Modify(_) | EventKind::Create(_) | EventKind::Remove(_) => {
                            Some(e.paths).unwrap_or_default()
                        }
                        _ => vec![],
                    })
                    .flatten()
                    .collect()
            }).unwrap_or_default()
        }
    }
}
```

### 9.2 Real Incremental Test Selection

**Replace**: `crates/glyim-testr/src/incremental.rs`

Replace the placeholder `DependencyGraph` with a real implementation backed by the query engine:

```rust
/// Real dependency graph for incremental test selection
pub struct TestDependencyGraph {
    /// Maps each test to the HIR items it depends on
    test_deps: HashMap<Symbol, HashSet<HirItemId>>,
    /// Maps each HIR item to the tests that depend on it
    item_to_tests: HashMap<HirItemId, HashSet<Symbol>>,
    /// Name dependency table for fine-grained invalidation
    name_deps: NameDependencyTable,
}

impl TestDependencyGraph {
    /// Build from a typed HIR + test definitions
    pub fn build(hir: &Hir, tests: &[TestDef], type_info: &TypeCheckResult) -> Self {
        let mut test_deps = HashMap::new();
        let mut item_to_tests = HashMap::new();

        for test in tests {
            let deps = Self::analyze_test_dependencies(test, hir, type_info);
            for dep_id in &deps {
                item_to_tests.entry(*dep_id).or_insert_with(HashSet::new).insert(test.name);
            }
            test_deps.insert(test.name, deps);
        }

        Self {
            test_deps,
            item_to_tests,
            name_deps: NameDependencyTable::build_from_hir(hir, type_info),
        }
    }

    /// Given changed HIR items, return only the affected tests
    pub fn affected_tests(&self, changed_items: &[HirItemId]) -> Vec<Symbol> {
        let mut affected = HashSet::new();

        // Direct dependents
        for item_id in changed_items {
            if let Some(tests) = self.item_to_tests.get(item_id) {
                affected.extend(tests);
            }
        }

        // Transitive dependents (via name hashing)
        let transitive = self.name_deps.transitive_dependents(changed_items);
        for item_id in &transitive {
            if let Some(tests) = self.item_to_tests.get(item_id) {
                affected.extend(tests);
            }
        }

        affected.into_iter().collect()
    }
}
```

### 9.3 Effect-Driven Parallel Test Execution

**Modify**: `crates/glyim-testr/src/executor.rs`

Use the effect system to guarantee safe parallelism:

```rust
/// Parallel test executor that uses effect analysis for safety
pub struct EffectAwareExecutor {
    /// Effect analyzer from the compiler
    effects: EffectAnalyzer,
    /// Thread pool for parallel execution
    pool: tokio::runtime::Runtime,
    /// Maximum concurrent tests
    max_concurrent: usize,
}

impl EffectAwareExecutor {
    /// Execute tests, parallelizing where the effect system proves it safe
    pub async fn execute_all(
        &self,
        tests: &[TestDef],
        artifact: &CompiledArtifact,
    ) -> Vec<TestResult> {
        // Partition tests into:
        // 1. Pure tests: can all run in parallel (no shared mutable state)
        // 2. Effectful tests: run sequentially (may have shared state)
        let (pure, effectful): (Vec<_>, Vec<_>) = tests.iter().partition(|t| {
            self.effects.is_pure(t.function_name)
        });

        let mut results = Vec::new();

        // Run pure tests in parallel
        let pure_futures: Vec<_> = pure.iter().map(|t| {
            self.execute_single(t, artifact)
        }).collect();
        let pure_results = futures::future::join_all(pure_futures).await;
        results.extend(pure_results);

        // Run effectful tests sequentially
        for test in effectful {
            results.push(self.execute_single(&test, artifact).await);
        }

        results
    }
}
```

### 9.4 Tests as PGO Training

**Modify**: `crates/glyim-testr/src/runner.rs`

When running tests via JIT, automatically collect profiles:

```rust
/// Test runner that also collects PGO data
pub struct ProfileCollectingRunner {
    inner: TestRunner,
    profile: Arc<ProfileCollector>,
}

impl ProfileCollectingRunner {
    pub async fn run_tests(&mut self, config: &TestConfig) -> TestRunSummary {
        // 1. Compile test suite with profiling instrumentation
        let artifact = self.compile_with_profiling(config)?;

        // 2. Execute tests (profiling happens automatically via instrumented code)
        let results = self.inner.execute_all(&artifact.test_defs, &artifact).await;

        // 3. Save profile data for AOT builds
        let profile_path = config.project_dir.join(".glyim-cache/test-profile.json");
        self.profile.save(&profile_path)?;

        // 4. Return results
        TestRunSummary { results, profile_path: Some(profile_path) }
    }
}
```

### 9.5 Real Flaky Test Detection

**Replace**: `crates/glyim-testr/src/flaky.rs`

```rust
/// Real flaky test detector using test history
pub struct FlakeDetector {
    /// Historical test results (from sea-orm DB)
    history: Arc<TestHistoryDb>,
    /// Threshold: if a test passes and fails intermittently, it's flaky
    flake_threshold: f64,
}

impl FlakeDetector {
    /// Compute a flakiness score for a test (0.0 = stable, 1.0 = very flaky)
    pub fn flakiness_score(&self, test_name: &Symbol) -> f64 {
        let recent = self.history.get_recent_results(test_name, 20);
        if recent.len() < 3 {
            return 0.0; // Not enough data
        }

        // Count transitions between pass and fail
        let transitions: usize = recent.windows(2)
            .filter(|w| w[0].outcome != w[1].outcome)
            .count();

        // Flakiness = transition rate
        transitions as f64 / (recent.len() - 1) as f64
    }

    /// Identify all flaky tests
    pub fn find_flaky_tests(&self, tests: &[TestDef]) -> Vec<(Symbol, f64)> {
        tests.iter()
            .map(|t| (t.name, self.flakiness_score(&t.name)))
            .filter(|(_, score)| *score > self.flake_threshold)
            .collect()
    }
}
```

### 9.6 Real Test Prioritization

**Replace**: `crates/glyim-testr/src/prioritizer.rs`

```rust
/// Real test prioritizer using history and flakiness data
pub struct SmartPrioritizer {
    flake_detector: FlakeDetector,
    history: Arc<TestHistoryDb>,
}

impl SmartPrioritizer {
    pub fn sort_tests(&self, tests: &mut [TestDef], mode: PriorityMode) {
        match mode {
            PriorityMode::DeclarationOrder => { /* no reordering */ }
            PriorityMode::FastFirst => {
                // Sort by historical median duration (fastest first)
                tests.sort_by_key(|t| {
                    self.history.get_median_duration(t.name).unwrap_or(Duration::from_millis(100))
                });
            }
            PriorityMode::RecentFailuresFirst => {
                // Sort: recently failed first, then by flakiness score
                tests.sort_by(|a, b| {
                    let a_failed = self.history.last_outcome(a.name).map_or(false, |o| !o.is_pass());
                    let b_failed = self.history.last_outcome(b.name).map_or(false, |o| !o.is_pass());
                    b_failed.cmp(&a_failed) // Failed first
                        .then_with(|| {
                            let a_flake = self.flake_detector.flakiness_score(&a.name);
                            let b_flake = self.flake_detector.flakiness_score(&b.name);
                            b_flake.partial_cmp(&a_flake).unwrap_or(Ordering::Equal)
                        })
                });
            }
        }
    }
}
```

### 9.7 Implementation Steps for Phase 6

| Step | Task | Estimated Effort |
|---|---|---|
| 6.1 | Replace `FileWatcher` stub with real `notify`-based implementation | 2-3 days |
| 6.2 | Implement `TestDependencyGraph` with name-based invalidation | 3-4 days |
| 6.3 | Implement `EffectAwareExecutor` for parallel test execution | 3-4 days |
| 6.4 | Implement `ProfileCollectingRunner` | 2-3 days |
| 6.5 | Replace `FlakeDetector` stub with real implementation | 2-3 days |
| 6.6 | Replace `SmartPrioritizer` with real implementation | 2-3 days |
| 6.7 | Wire sea-orm history DB into the runner | 2-3 days |
| 6.8 | Add `--watch`, `--profile`, `--parallel` CLI flags | 1-2 days |
| 6.9 | Write tests | 3-4 days |
| **Total** | | **~20-29 days** |

### 9.8 Success Criteria for Phase 6

- File watcher detects changes within 100ms and triggers incremental re-testing
- Changing a comment triggers zero test re-execution (semantic hash unchanged)
- Pure tests run in parallel across all CPU cores with zero risk of race conditions
- Test runs automatically collect PGO profiles that feed AOT builds
- Flaky tests are detected and reported with flakiness scores
- Failed tests are prioritized to run first, reducing feedback latency

---

## 10. Phase 7 — Compiler-Level Mutation Testing

**Goal**: Build deeply integrated mutation testing that leverages the JIT, e-graph, and dispatch table infrastructure for unprecedented speed.

### 7.1 New Crate: `glyim-mutate`

**New crate**: `crates/glyim-mutate/`

```rust
/// Mutation testing engine integrated with the compiler
pub struct MutationEngine<'ctx> {
    /// Access to the JIT for in-memory mutant execution
    tiered: &'ctx mut TieredCompiler<'ctx>,
    /// E-graph for equivalent mutant pruning
    egraph: &'ctx mut GlyimEgraph,
    /// The dispatch table for atomic mutant swapping
    dispatch: Arc<DispatchTable>,
    /// Mutant generation strategies
    strategies: Vec<Box<dyn MutationStrategy>>,
    /// Results of mutation testing
    results: Vec<MutantResult>,
}

pub struct MutantResult {
    /// The mutated function name
    function: Symbol,
    /// Description of the mutation
    description: String,
    /// Whether the mutant was killed by any test
    killed: bool,
    /// Which test killed it (if any)
    killed_by: Option<Symbol>,
    /// Whether the mutant was pruned as equivalent
    equivalent: bool,
    /// Execution time for this mutant
    duration: Duration,
}

/// A mutation strategy generates mutants
pub trait MutationStrategy {
    /// Generate mutants for a function
    fn generate(&self, hir_fn: &HirFn) -> Vec<Mutant>;
    /// Name of this strategy
    fn name(&self) -> &str;
}

/// Standard source-level mutants
pub struct SourceMutator;

impl MutationStrategy for SourceMutator {
    fn generate(&self, hir_fn: &HirFn) -> Vec<Mutant> {
        let mut mutants = Vec::new();
        // Walk the HIR, generate mutants at each expression:
        // - Replace + with -, * with /, && with ||, etc.
        // - Replace == with !=, < with >=, etc.
        // - Remove condition checks
        // - Replace return values with constants
        // - Insert early returns
        for expr in hir_fn.walk_exprs() {
            match expr {
                HirExpr::BinOp(op, lhs, rhs) => {
                    for alt_op in op.mutation_alternates() {
                        mutants.push(Mutant {
                            function: hir_fn.name,
                            location: expr.span(),
                            original: format!("{:?} {}, {}", op, lhs, rhs),
                            replacement: format!("{:?} {}, {}", alt_op, lhs, rhs),
                            apply: Box::new(move |hir: &mut HirFn| {
                                // Apply the mutation to the HIR
                                replace_binop(hir, expr.id(), alt_op);
                            }),
                        });
                    }
                }
                // ... other expression types
                _ => {}
            }
        }
        mutants
    }
}
```

### 7.2 In-Memory Quantum Mutant Multiplexing

**New file**: `crates/glyim-mutate/src/multiplexer.rs`

Compile all mutants simultaneously into hidden JITDylibs and swap function pointers:

```rust
/// Manages in-memory mutant compilation and dispatch
pub struct MutantMultiplexer<'ctx> {
    /// The original function's JITDylib
    original_dylib: JITDylib<'ctx>,
    /// One JITDylib per mutant (hidden, not in the main dispatch table)
    mutant_dylibs: Vec<(MutantId, JITDylib<'ctx>)>,
    /// The dispatch table (shared with the test runner)
    dispatch: Arc<DispatchTable>,
    /// Backup of original function addresses
    original_addresses: HashMap<Symbol, usize>,
}

impl<'ctx> MutantMultiplexer<'ctx> {
    /// Compile all mutants for a function into separate JITDylibs
    pub fn compile_mutants(
        &mut self,
        fn_name: Symbol,
        mutants: &[Mutant],
        codegen: &mut Codegen<'ctx>,
    ) -> Result<Vec<MutantId>, MutateError> {
        let mut ids = Vec::new();

        for (i, mutant) in mutants.iter().enumerate() {
            // 1. Clone the HIR and apply the mutation
            let mut mutated_hir = self.hir.clone();
            (mutant.apply)(&mut mutated_hir);

            // 2. Compile the mutated function into a new JITDylib
            let dylib_name = format!("mutant_{}_{}", fn_name, i);
            let dylib = self.execution_session.create_dylib(&dylib_name)?;

            // 3. Lower mutated HIR to LLVM IR and add to dylib
            let module = codegen.compile_item(&mutated_hir)?;
            self.add_module_to_dylib(&module, &dylib)?;

            // 4. Resolve the function's address in the mutant dylib
            let mutant_id = MutantId(i);
            self.mutant_dylibs.push((mutant_id, dylib));
            ids.push(mutant_id);
        }

        Ok(ids)
    }

    /// Activate a specific mutant (swap the dispatch table pointer)
    pub fn activate_mutant(&self, fn_name: Symbol, mutant_id: MutantId) {
        let (_, dylib) = self.mutant_dylibs.iter().find(|(id, _)| *id == mutant_id).unwrap();
        let address = self.resolve_in_dylib(dylib, fn_name);
        self.dispatch.update(fn_name, address);
    }

    /// Restore the original function
    pub fn restore_original(&self, fn_name: Symbol) {
        let original = self.original_addresses[&fn_name];
        self.dispatch.update(fn_name, original);
    }

    /// Run a test against a specific mutant
    pub fn test_mutant(
        &self,
        fn_name: Symbol,
        mutant_id: MutantId,
        test: &TestDef,
        test_runner: &mut TestRunner,
    ) -> MutantResult {
        // 1. Activate the mutant
        self.activate_mutant(fn_name, mutant_id);

        // 2. Run the test
        let result = test_runner.execute_single(test);

        // 3. Restore original
        self.restore_original(fn_name);

        MutantResult {
            function: fn_name,
            description: format!("Mutant {:?}", mutant_id),
            killed: result.outcome != TestOutcome::Passed,
            killed_by: if result.outcome != TestOutcome::Passed { Some(test.name) } else { None },
            equivalent: false,
            duration: result.duration,
        }
    }
}
```

### 7.3 E-Graph Meta-Mutations (Testing the Optimizer)

**New file**: `crates/glyim-mutate/src/meta_mutator.rs`

Instead of mutating source code, mutate the compiler's optimization rules:

```rust
/// Meta-mutation: tests the compiler's optimization passes, not the source code
pub struct MetaMutator {
    /// The full set of rewrite rules
    all_rules: Vec<Rewrite<GlyimExpr, GlyimAnalysis>>,
}

impl MetaMutator {
    /// Generate meta-mutants by disabling individual rewrite rules
    pub fn generate_meta_mutants(&self) -> Vec<MetaMutant> {
        self.all_rules.iter().enumerate().map(|(i, rule)| {
            MetaMutant {
                disabled_rule_index: i,
                disabled_rule_name: rule.name.to_string(),
                description: format!("Disable optimization rule: {}", rule.name),
            }
        }).collect()
    }

    /// Run a test with a specific optimization rule disabled
    pub fn test_meta_mutant(
        &self,
        meta: &MetaMutant,
        hir: &Hir,
        tests: &[TestDef],
        test_runner: &mut TestRunner,
    ) -> MetaMutantResult {
        // 1. Create a rule set with the target rule disabled
        let mut rules = self.all_rules.clone();
        rules.remove(meta.disabled_rule_index);

        // 2. Optimize the HIR with the reduced rule set
        let sub_optimal_hir = optimize_with_rules(hir, &rules);

        // 3. Compile and execute tests
        let results = test_runner.compile_and_run(&sub_optimal_hir, tests);

        // 4. Compare with baseline (all rules enabled)
        MetaMutantResult {
            rule_name: meta.disabled_rule_name.clone(),
            tests_changed: results.iter().zip(baseline_results.iter())
                .filter(|(a, b)| a.outcome != b.outcome)
                .count(),
            description: meta.description.clone(),
        }
    }
}
```

### 7.4 Zero-Overhead Equivalent Mutant Pruning via E-Graph

**New file**: `crates/glyim-mutate/src/pruner.rs`

```rust
/// Prune equivalent mutants using e-graph analysis
pub struct EquivalentMutantPruner {
    egraph_engine: GlyimEgraph,
}

impl EquivalentMutantPruner {
    /// Check if a mutant is semantically equivalent to the original
    pub fn is_equivalent(&self, original: &HirFn, mutant: &HirFn) -> bool {
        // 1. Convert both to e-graphs
        let mut egraph = hir_fn_to_egraph(original);
        let mutant_root = hir_fn_to_egraph_with_root(mutant, &mut egraph);

        // 2. Run equality saturation with our rewrite rules
        let runner = Runner::default()
            .with_egraph(egraph)
            .run(&core_rewrites())
            .with_node_limit(50000)
            .with_time_limit(Duration::from_millis(100));

        // 3. If the original and mutant are in the same e-class, they're equivalent
        let original_root = runner.egraph.roots[0];
        runner.egraph.find(original_root) == runner.egraph.find(mutant_root)
    }

    /// Prune all equivalent mutants from a list
    pub fn prune(&self, original: &HirFn, mutants: &[Mutant]) -> Vec<Mutant> {
        mutants.iter().filter(|mutant| {
            let mutated_hir = apply_mutant(original, mutant);
            !self.is_equivalent(original, &mutated_hir)
        }).cloned().collect()
    }
}
```

### 7.5 Guard Mutation Testing

**New file**: `crates/glyim-mutate/src/guard_mutator.rs`

Test that deoptimization fallback paths work correctly by forcefully inverting speculative guards:

```rust
/// Mutates speculative optimization guards to test fallback paths
pub struct GuardMutator {
    /// All guard conditions in the compiled code
    guards: Vec<GuardInfo>,
}

pub struct GuardInfo {
    function: Symbol,
    guard_type: GuardType,
    location: SourceSpan,
}

pub enum GuardType {
    TypeCheck { var: Symbol, expected: HirType },
    BoundsCheck { array: Symbol },
    NotNull { ptr: Symbol },
}

impl GuardMutator {
    /// Generate guard-inversion mutants
    pub fn generate(&self) -> Vec<GuardMutant> {
        self.guards.iter().map(|guard| {
            GuardMutant {
                function: guard.function,
                location: guard.location,
                description: format!("Invert guard: {:?}", guard.guard_type),
                // The mutation: force the guard to always fail → take the slow path
            }
        }).collect()
    }

    /// Test a guard mutant: force all guards to fail, verify the slow path produces correct results
    pub fn test_guard_mutant(
        &self,
        mutant: &GuardMutant,
        tiered: &mut TieredCompiler,
        test_runner: &mut TestRunner,
    ) -> GuardMutantResult {
        // 1. Force the specific guard to fail (modify the JIT dispatch)
        tiered.force_guard_failure(mutant.function, mutant.location);

        // 2. Run tests — they should still pass (slow path must be correct)
        let results = test_runner.run_tests();

        // 3. Check if any test produced different results
        let failures = results.iter().filter(|r| r.outcome != TestOutcome::Passed).count();

        GuardMutantResult {
            guard_mutant: mutant.clone(),
            failures,
            correct: failures == 0,
        }
    }
}
```

### 7.6 Mutation-Aware Incremental Compilation (Red/Green Mutant DAG)

**New file**: `crates/glyim-mutate/src/dag.rs`

When source code changes, only re-test mutants that depend on the changed code:

```rust
/// Mutation-aware dependency graph
pub struct MutantDependencyGraph {
    /// Maps each mutant to the HIR items it depends on
    mutant_deps: HashMap<MutantId, HashSet<HirItemId>>,
    /// Maps each HIR item to the mutants that depend on it
    item_to_mutants: HashMap<HirItemId, HashSet<MutantId>>,
    /// Cached mutant results from previous runs
    cached_results: HashMap<MutantId, MutantResult>,
}

impl MutantDependencyGraph {
    /// Build from a set of mutants and the HIR
    pub fn build(mutants: &[Mutant], hir: &Hir) -> Self {
        let mut mutant_deps = HashMap::new();
        let mut item_to_mutants = HashMap::new();

        for mutant in mutants {
            let deps = Self::mutant_dependencies(mutant, hir);
            for dep_id in &deps {
                item_to_mutants.entry(*dep_id).or_insert_with(HashSet::new).insert(mutant.id);
            }
            mutant_deps.insert(mutant.id, deps);
        }

        Self { mutant_deps, item_to_mutants, cached_results: HashMap::new() }
    }

    /// Given changed HIR items, return which mutants need re-testing
    pub fn affected_mutants(&self, changed_items: &[HirItemId]) -> (Vec<MutantId>, Vec<MutantId>) {
        let mut red = HashSet::new();  // Need re-testing
        let mut green = HashSet::new(); // Can skip (cached result still valid)

        // Find all mutants that depend on changed items
        for item_id in changed_items {
            if let Some(mutants) = self.item_to_mutants.get(item_id) {
                red.extend(mutants);
            }
        }

        // All mutants not in red are green
        for id in self.mutant_deps.keys() {
            if !red.contains(id) {
                green.insert(*id);
            }
        }

        (red.into_iter().collect(), green.into_iter().collect())
    }

    /// Get cached results for green (unaffected) mutants
    pub fn get_cached_results(&self, green: &[MutantId]) -> Vec<&MutantResult> {
        green.iter().filter_map(|id| self.cached_results.get(id)).collect()
    }
}
```

### 7.7 Implementation Steps for Phase 7

| Step | Task | Estimated Effort |
|---|---|---|
| 7.1 | Create `glyim-mutate` crate with core types | 2-3 days |
| 7.2 | Implement `SourceMutator` strategy | 3-4 days |
| 7.3 | Implement `MutantMultiplexer` with JITDylib-based execution | 5-6 days |
| 7.4 | Implement `EquivalentMutantPruner` via e-graph | 3-4 days |
| 7.5 | Implement `MetaMutator` for optimizer testing | 3-4 days |
| 7.6 | Implement `GuardMutator` for deoptimization testing | 3-4 days |
| 7.7 | Implement `MutantDependencyGraph` for incremental mutation | 3-4 days |
| 7.8 | Add `glyim test --mutate` CLI command | 2-3 days |
| 7.9 | Write tests | 4-5 days |
| **Total** | | **~28-37 days** |

### 7.8 Success Criteria for Phase 7

- `glyim test --mutate` generates, compiles, and executes 1000 mutants in <30 seconds
- Equivalent mutant pruning eliminates ≥50% of mutants before compilation
- Mutant execution is zero-overhead: no process spawning, just function pointer swaps
- Guard mutations catch deoptimization bugs that manual testing misses
- After a source change, only mutants touching the changed code are re-tested
- Meta-mutations detect at least one optimization-dependent correctness issue

---

## 11. Phase 8 — Self-Healing, Observability & Advanced Safety

**Goal**: Add cryptographic provenance, self-healing caches, shadow execution, and replayable optimization traces.

### 8.1 Self-Healing Provenance Graphs

**Modify**: `crates/glyim-merkle/`, `crates/glyim-query/`

Attach provenance to every cached artifact and implement a background verification daemon:

```rust
/// Provenance attached to every cached artifact
#[derive(Serialize, Deserialize)]
pub struct ArtifactProvenance {
    /// What produced this artifact
    pub producer: PassIdentity,
    /// What inputs were used
    pub input_hashes: Vec<ContentHash>,
    /// When it was produced
    pub timestamp: u64,
    /// The compiler version that produced it
    pub compiler_version: String,
    /// Whether this artifact has been verified
    pub verified: bool,
}

#[derive(Serialize, Deserialize)]
pub struct PassIdentity {
    /// Which compiler pass produced this
    pub pass_name: String,
    /// Hash of the pass's implementation (deterministic)
    pub pass_hash: ContentHash,
}

/// Background cache verifier
pub struct CacheVerifier {
    store: Arc<MerkleStore>,
    /// How often to run verification
    interval: Duration,
}

impl CacheVerifier {
    /// Verify the integrity of the cache
    pub fn verify_all(&self) -> VerificationReport {
        let mut report = VerificationReport::default();

        // Walk all nodes in the Merkle store
        for node in self.store.iter_all() {
            // 1. Re-hash the node's data and compare with stored hash
            let computed_hash = ContentHash::hash(&node.data);
            if computed_hash != node.hash {
                report.corrupted_nodes.push(node.hash);
                continue;
            }

            // 2. Verify that all referenced children still exist
            for child_hash in &node.children {
                if self.store.get(child_hash).is_none() {
                    report.missing_children.push((node.hash, *child_hash));
                }
            }

            // 3. Verify provenance: re-run the producing pass and compare
            if let Some(provenance) = &node.provenance {
                // (Optional: expensive) Re-run the pass with the same inputs
                // and verify the output matches
            }
        }

        // Auto-heal: remove corrupted nodes and their dependents
        for hash in &report.corrupted_nodes {
            self.store.remove_subtree(hash);
            report.healed_nodes += 1;
        }

        report
    }

    /// Run the verifier in a background loop
    pub async fn run_background(&self) {
        let mut interval = tokio::time::interval(self.interval);
        loop {
            interval.tick().await;
            let report = self.verify_all();
            if !report.corrupted_nodes.is_empty() {
                tracing::warn!("Cache verification: {} corrupted nodes, {} healed",
                    report.corrupted_nodes.len(), report.healed_nodes);
            }
        }
    }
}
```

### 8.2 Shadow Execution for Speculative Optimization Safety

**New file**: `crates/glyim-compiler/src/shadow.rs`

```rust
/// Shadow execution: run a "safe" interpreter alongside optimized JIT code
pub struct ShadowExecutor {
    /// The bytecode interpreter (always correct, never speculatively optimized)
    interpreter: BytecodeInterpreter,
    /// The optimized JIT code
    jit: DoubleBufferedJIT<'static>,
    /// Checkpoints where shadow and JIT values are compared
    checkpoints: Vec<CheckpointLocation>,
    /// Divergences detected
    divergences: Vec<Divergence>,
}

pub struct CheckpointLocation {
    function: Symbol,
    /// Source location
    span: SourceSpan,
    /// Which variables to compare
    variables: Vec<Symbol>,
}

pub struct Divergence {
    checkpoint: CheckpointLocation,
    shadow_value: Value,
    jit_value: Value,
    /// The speculative optimization that may have caused the divergence
    suspected_optimization: Option<String>,
}

impl ShadowExecutor {
    /// Execute a function with shadow checking
    pub fn execute_with_shadow(&mut self, fn_name: Symbol, args: &[Value]) -> Value {
        // 1. Run the JIT-optimized version
        let jit_result = self.jit.execute(fn_name, args);

        // 2. Run the interpreter version
        let shadow_result = self.interpreter.execute(fn_name, args);

        // 3. Compare at checkpoints
        if jit_result != shadow_result {
            self.divergences.push(Divergence {
                checkpoint: self.find_nearest_checkpoint(fn_name),
                shadow_value: shadow_result,
                jit_value: jit_result,
                suspected_optimization: self.identify_suspect_optimization(fn_name),
            });
            // Return the shadow result (safe) and log the divergence
            tracing::error!("Shadow divergence in {}: JIT={:?}, Shadow={:?}",
                fn_name, jit_result, shadow_result);
            return shadow_result;
        }

        jit_result
    }
}
```

### 8.3 Replayable Optimization Traces

**New file**: `crates/glyim-compiler/src/trace.rs`

```rust
/// A complete trace of optimization decisions for a function
#[derive(Serialize, Deserialize)]
pub struct OptimizationTrace {
    function: Symbol,
    /// Each pass applied, in order
    passes: Vec<PassTrace>,
    /// Final IR hash
    final_hash: ContentHash,
}

#[derive(Serialize, Deserialize)]
pub struct PassTrace {
    /// Pass name
    pass_name: String,
    /// Input IR hash
    input_hash: ContentHash,
    /// Output IR hash
    output_hash: ContentHash,
    /// Whether this pass changed anything
    changed: bool,
    /// Key decisions made (e.g., "inlined foo into bar", "unrolled loop 4x")
    decisions: Vec<String>,
    /// Profile data used (if any)
    profile_snapshot: Option<ProfileSnapshot>,
}

/// Replay an optimization trace to reproduce the exact same compilation
pub fn replay_trace(
    trace: &OptimizationTrace,
    hir: &HirFn,
    codegen: &mut Codegen,
) -> Result<FunctionValue, ReplayError> {
    for pass_trace in &trace.passes {
        // Re-run the pass with the same inputs
        let result = codegen.run_pass(&pass_trace.pass_name, hir)?;
        // Verify the output matches
        let output_hash = ContentHash::hash(&result);
        if output_hash != pass_trace.output_hash {
            return Err(ReplayError::Divergence {
                pass: pass_trace.pass_name.clone(),
                expected: pass_trace.output_hash,
                actual: output_hash,
            });
        }
    }
    Ok(codegen.current_function())
}
```

### 8.4 Continuous Autotuning

**New file**: `crates/glyim-compiler/src/autotune.rs`

```rust
/// Self-tuning compiler that learns optimal optimization parameters
pub struct Autotuner {
    /// Per-function optimization parameter history
    params: HashMap<Symbol, OptParams>,
    /// Performance measurements
    measurements: HashMap<Symbol, Vec<Measurement>>,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct OptParams {
    optimization_level: u8,
    inline_threshold: usize,
    unroll_factor: usize,
    vectorize: bool,
}

pub struct Measurement {
    params: OptParams,
    compile_time_ms: u64,
    execution_time_ns: u64,
    code_size_bytes: usize,
}

impl Autotuner {
    /// Choose optimization parameters for a function using multi-armed bandit
    pub fn choose_params(&self, fn_name: Symbol) -> OptParams {
        let measurements = self.measurements.get(&fn_name);
        if measurements.is_none() || measurements.unwrap().len() < 5 {
            // Not enough data — use defaults
            return OptParams::default();
        }

        // UCB1 algorithm: balance exploration vs exploitation
        let measurements = measurements.unwrap();
        let total_count = measurements.len() as f64;

        let mut best_score = f64::NEG_INFINITY;
        let mut best_params = OptParams::default();

        // Group by params
        let grouped = group_by_params(measurements);
        for (params, group) in &grouped {
            let count = group.len() as f64;
            let avg_exec = group.iter().map(|m| m.execution_time_ns as f64).sum::<f64>() / count;
            let avg_compile = group.iter().map(|m| m.compile_time_ms as f64).sum::<f64>() / count;

            // Score: minimize execution time, penalize compile time
            let score = -avg_exec - 0.1 * avg_compile
                + (2.0 * (total_count.ln() / count).sqrt()); // UCB1 exploration term

            if score > best_score {
                best_score = score;
                best_params = params.clone();
            }
        }

        best_params
    }
}
```

### 8.5 Implementation Steps for Phase 8

| Step | Task | Estimated Effort |
|---|---|---|
| 8.1 | Implement `ArtifactProvenance` in `glyim-merkle` | 2-3 days |
| 8.2 | Implement `CacheVerifier` with auto-healing | 3-4 days |
| 8.3 | Implement `ShadowExecutor` | 4-5 days |
| 8.4 | Implement `OptimizationTrace` recording | 3-4 days |
| 8.5 | Implement trace replay | 2-3 days |
| 8.6 | Implement `Autotuner` with UCB1 | 3-4 days |
| 8.7 | Add `--shadow`, `--trace`, `--autotune` CLI flags | 1-2 days |
| 8.8 | Write tests | 3-4 days |
| **Total** | | **~21-29 days** |

### 8.6 Success Criteria for Phase 8

- Corrupted cache nodes are automatically detected and purged with a diagnostic message
- Shadow execution catches at least 1 speculative optimization bug per month of development
- Optimization traces can be replayed to reproduce exact compilation results
- The autotuner reduces compile time by >20% for hot functions without sacrificing execution speed

---

## 12. New Crate Map

After all phases, the workspace will include these new crates:

| Crate | Tier | Dependencies | Purpose |
|---|---|---|---|
| `glyim-query` | 1 | `dashmap`, `petgraph`, `serde` | Query engine, memoization, red/green invalidation |
| `glyim-merkle` | 2 | `glyim-macro-vfs`, `glyim-interner`, `serde` | Merkle IR tree, branch-agnostic caching |
| `glyim-bytecode` | 3 | `glyim-hir`, `glyim-interner` | Tier-0 bytecode compiler and interpreter |
| `glyim-egraph` | 3 | `egg`, `glyim-hir`, `glyim-interner` | E-graph optimization, invariant certificates, effect analysis |
| `glyim-profile` | 3 | `glyim-hir`, `glyim-interner`, `serde` | Runtime profile collection and persistence |
| `glyim-mutate` | 5 | `glyim-hir`, `glyim-codegen-llvm`, `glyim-egraph`, `glyim-testr` | Compiler-level mutation testing |

Updated tier structure:
- **Tier 1**: `glyim-interner`, `glyim-diag`, `glyim-syntax`, `glyim-query`
- **Tier 2**: `glyim-lex`, `glyim-parse`, `glyim-merkle`
- **Tier 3**: `glyim-hir`, `glyim-typeck`, `glyim-macro-core`, `glyim-macro-vfs`, `glyim-bytecode`, `glyim-egraph`, `glyim-profile`
- **Tier 4**: `glyim-codegen-llvm`
- **Tier 5**: `glyim-cli`, `glyim-cas-server`, `glyim-mutate`

---

## 13. Migration & Compatibility Strategy

### 13.1 Feature Flags

Each phase is behind a feature flag to allow incremental adoption:

```toml
# glyim-compiler/Cargo.toml
[features]
default = []
incremental = ["glyim-query"]           # Phase 0
semantic-cache = ["incremental"]         # Phase 1
live-jit = ["semantic-cache"]            # Phase 2
egraph-opt = ["glyim-egraph"]            # Phase 3
speculative = ["live-jit", "egraph-opt"] # Phase 4
aot-thinlto = ["incremental"]            # Phase 5
smart-tests = ["incremental"]            # Phase 6
mutation = ["egraph-opt", "live-jit"]    # Phase 7
safety = ["incremental"]                 # Phase 8
full = ["safety", "mutation"]            # All features
```

### 13.2 Backward Compatibility

- `glyim build` without `--incremental` behaves exactly as before (linear pipeline)
- `glyim run` without `--live` uses the existing JIT path (single module)
- `glyim test` without `--mutate` uses the existing test runner
- The existing CAS server remains unchanged; new features add optional endpoints
- The existing `build_with_cache()` function continues to work for simple cases

### 13.3 Data Migration

- The `.glyim-cache/` directory is created automatically on first use of `--incremental`
- Old cache formats (from `build_with_cache`) are automatically migrated
- The `query-db/` directory uses a versioned format; version mismatches trigger a clean rebuild
- Profile data format is versioned; old profiles are silently ignored (not an error)

---

## 14. Risk Register

| Risk | Likelihood | Impact | Mitigation |
|---|---|---|---|
| OrcV2 API instability in Inkwell | Medium | High | Pin Inkwell version; test against specific LLVM 22.x builds; have fallback to monolithic JIT |
| E-graph scalability for large functions | Medium | Medium | Set node limits and time budgets; fall back to greedy optimization for large functions |
| Query engine memory overhead | Low | Medium | Implement LRU eviction for query results; persist to disk when memory pressure is high |
| SSA patching complexity in the Stitcher | High | Low | Fall back to full function rebuild for complex changes; only patch simple expression diffs |
| Profile data staleness | Low | Low | Include profile timestamp in PGO decisions; re-collect profiles periodically |
| Remote CAS latency | Medium | Low | Asynchronous uploads; local-first with background sync; timeout-based fallback to local |
| Mutation testing explosion | Medium | Medium | Equivalent mutant pruning; limit per-function mutants; `--mutate-mode=conservative|aggressive` |
| Feature flag combinatorics | Low | Low | Test matrix: `default`, `incremental`, `live-jit`, `full`; CI covers all combos |

---

## 15. Milestone Timeline

| Milestone | Phase(s) | Duration | Key Deliverable |
|---|---|---|---|
| **M1: Query Foundation** | Phase 0 | 5-6 weeks | `--incremental` flag works; unchanged files are skipped |
| **M2: Semantic Caching** | Phase 1 | 3-4 weeks | Auto-formatting triggers zero recompilation; branch switching is cheap |
| **M3: Live JIT** | Phase 2 | 6-8 weeks | `--live` flag; hot-patching works; tiered compilation; sub-50ms edit latency |
| **M4: E-Graph Optimizer** | Phase 3 | 5-6 weeks | Algebraic optimizations work; invariant certificates skip redundant work |
| **M5: Speculative & PGO** | Phase 4 | 5-7 weeks | JIT profiles feed AOT; speculative pre-compilation; type specialization |
| **M6: AOT ThinLTO** | Phase 5 | 5-6 weeks | Release builds use ThinLTO with language summaries; CAS hits on CI |
| **M7: Smart Tests** | Phase 6 | 3-5 weeks | Incremental test selection; effect-driven parallelism; real flaky detection |
| **M8: Mutation Testing** | Phase 7 | 4-6 weeks | `--mutate` flag; quantum multiplexing; e-graph pruning; meta-mutations |
| **M9: Safety & Observability** | Phase 8 | 3-5 weeks | Self-healing cache; shadow execution; replay traces; autotuning |
| **Total** | | **39-53 weeks** | Full "Quantum Compiler" |

### Recommended Starting Priority

If you can only implement 3 phases, do these in order:

1. **Phase 0** (Query Engine) — Everything else depends on this
2. **Phase 2** (JIT Micro-Modules) — Biggest UX improvement for developers
3. **Phase 3** (E-Graph) — Enables equivalent mutant pruning and algebraic optimization

These three phases alone give you:
- Incremental compilation with fine-grained invalidation
- Sub-50ms edit/run loop with hot-patching
- Semantic equivalence proofs for optimization and testing

---

## Appendix A: Existing Infrastructure Reuse Map

| New Feature | Existing Code to Reuse | Modification Needed |
|---|---|---|
| Merkle store | `glyim-macro-vfs::LocalContentStore` | Wrap with Merkle node serialization |
| Merkle store | `glyim-macro-vfs::RemoteContentStore` | Use as-is for remote Merkle node storage |
| Sub-function CAS | `glyim-cas-server` gRPC + REST | Add sub-function key endpoints |
| Query persistence | `glyim-macro-vfs::ContentStore` | Store query DB as CAS blobs |
| Profile persistence | `glyim-testr::history` (sea-orm) | Extend schema with profile columns |
| Semantic hashing | `glyim-hir::lower` (already strips comments) | Add normalization pass |
| Effect analysis | `glyim-typeck::TypeChecker` (already tracks types) | Add effect inference to type checker |
| Dispatch table | `glyim-codegen-llvm::mono_cache` | Extend to support atomic updates |
| Test collection | `glyim-testr::collector` | Add HIR item dependency tracking |
| File watching | `notify-debouncer-full` (already a dependency) | Replace stub with real implementation |

## Appendix B: Key Dependency Additions

| Library | Version | Purpose | Used In |
|---|---|---|---|
| `egg` | 0.6 | E-graph / equality saturation | `glyim-egraph` |
| `dashmap` | 6.x | Concurrent hash map for query cache | `glyim-query` |
| `petgraph` | 0.7 | Dependency graph (DAG) | `glyim-query` |
| `serde` + `serde_json` | 1.x | Serialization for persistence | All new crates |
| `bincode` | 1.x | Fast binary serialization for CAS blobs | `glyim-merkle`, `glyim-query` |

## Appendix C: Performance Targets

| Metric | Current | After Phase 0 | After Phase 2 | After Full |
|---|---|---|---|---|
| Unchanged file rebuild | ~2-5s | <50ms | <50ms | <10ms |
| Single-function change | ~2-5s | ~500ms | ~50ms | ~5ms |
| Auto-format (no semantic change) | ~2-5s | ~2-5s | <50ms | <1ms |
| Branch switch (90% shared) | ~2-5s | ~2-5s | ~2-5s | ~100ms |
| Test suite (unchanged) | ~10-30s | <100ms | <100ms | <10ms |
| Mutation testing (1000 mutants) | N/A | N/A | N/A | ~30s |
| First JIT execution | ~200ms | ~200ms | ~5ms (bytecode) | ~1ms (bytecode) |
| Hot JIT execution | ~200ms | ~200ms | ~50ms (LLVM) | ~5ms (pre-compiled) |
