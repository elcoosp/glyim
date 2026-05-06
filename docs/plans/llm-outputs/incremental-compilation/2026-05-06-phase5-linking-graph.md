# Glyim Incremental Compiler — Phase 5 Implementation Plan

## Cross-Module Incremental Linking & Package Graph Orchestration

**Codebase:** elcoosp-glyim v0.5.0  
**Rust Workspace | 20 Crates | LLVM 22.1 / Inkwell 0.9**  
**Date:** 2026-05-07

---

## 1. Executive Summary

Phase 5 extends Glyim's incremental compilation from a single-module model to a fully cross-module, package-graph-aware system. Phases 3 and 4 established per-function incremental recompilation within a single source file, but the current compiler treats each module in isolation: `glyim build` compiles one `main.g` at a time, cross-module calls are opaque externs, and a change in a dependency forces a full rebuild of every downstream consumer. The package manager (`glyim-pkg`) already provides manifest parsing (`PackageManifest`), lockfile generation (`Lockfile`), dependency resolution (`resolver::resolve`), and workspace detection (`workspace::detect_workspace`), but the compiler does not use any of this information to drive incremental builds. The CAS server (`glyim-cas-server`) stores blobs and action results over HTTP and gRPC, but the compiler never queries it for pre-compiled dependency artifacts.

Phase 5 closes these gaps by introducing a package graph orchestrator that coordinates incremental compilation across an entire workspace. When a source file changes, the orchestrator determines which packages are affected by traversing the dependency graph, recomputes only the affected modules using the query-driven pipeline from Phase 4, and links the final binary from a mixture of freshly compiled and cached object code. Cross-module calls are resolved through a combination of symbol tables exported from each package's compiled artifact and the existing `DispatchTable` infrastructure from `glyim-codegen-llvm/src/dispatch.rs`. Pre-compiled dependency artifacts are stored in and retrieved from the CAS, enabling `glyim build` to skip compilation of unchanged dependencies entirely — whether they are local path dependencies, workspace members, or remote registry packages.

The phase also introduces a multi-module linker that replaces the current single-object `cc` invocation with a proper multi-object link step, and a package-level Merkle root that summarizes the entire dependency graph for fast cache lookups. The `glyim fetch` command gains the ability to download pre-compiled object code from the CAS server, and the `glyim build` command gains a `--remote-cache` flag that pushes and pulls compilation artifacts from a shared CAS endpoint.

**Estimated effort:** 28–38 working days.

**Key deliverables:**
- Package graph orchestrator (`glyim-orchestrator` crate)
- Cross-module symbol resolution and linking
- CAS-backed dependency artifact sharing (local and remote)
- Workspace-aware `glyim build` and `glyim test`
- Package-level Merkle roots for whole-graph cache invalidation
- `--remote-cache` CLI flag for shared build caches
- Multi-object linker replacing single-file `cc` invocation

---

## 2. Current Codebase State Assessment

### 2.1 Single-Module Compilation Model

The current pipeline in `glyim-compiler/src/pipeline.rs` is fundamentally single-file. The `build()` function takes one `input: &Path`, loads that single file (with the prelude prepended), compiles it through the linear pipeline, and links the resulting single object file into a binary. The `build_package()` function locates `src/main.g` within a package directory, but it still compiles only that one file. There is no mechanism to compile multiple source files, link object code from multiple packages, or resolve symbols across module boundaries.

The `compile_source_to_hir()` function hardcodes a single source string. The `Codegen::generate()` method accepts a single `Hir` and emits all functions into one LLVM module. The `link_object()` function links exactly one `.o` file with `-lc -no-pie`. Every aspect of the pipeline assumes a one-file-in, one-binary-out model.

### 2.2 Package Management Infrastructure (As-Is)

The `glyim-pkg` crate provides a solid foundation for multi-package workflows, but it is not connected to the compiler:

| Component | File | Status | Gap |
|-----------|------|--------|-----|
| `PackageManifest` | `glyim-pkg/src/manifest.rs` | Fully implemented with `[package]`, `[dependencies]`, `[macros]`, `[dev-dependencies]`, `[target.*]`, `[cache]`, `[workspace]` | Not consumed by the compiler during builds |
| `Lockfile` | `glyim-pkg/src/lockfile.rs` | TOML-based lockfile with `LockedPackage`, `LockSource`, content hashes | Hashes are placeholder zeroes for registry deps; not used for artifact retrieval |
| `Resolver` | `glyim-pkg/src/resolver.rs` | Minimal version selection with lockfile precedence, caret/wildcard constraints | No transitive compilation; only resolves versions |
| `Workspace` | `glyim-pkg/src/workspace.rs` | `detect_workspace()` walks up to find `glyim.toml` with `[workspace]` section | Not used by `build` command |
| `RegistryClient` | `glyim-pkg/src/registry.rs` | HTTP client for fetching package metadata and publishing | Only fetches metadata, not compiled artifacts |
| `CasClient` | `glyim-pkg/src/cas_client.rs` | Wraps `LocalContentStore` or `RemoteContentStore` | Only used for macro caching |
| `lockfile_integration` | `glyim-compiler/src/lockfile_integration.rs` | `resolve_and_write_lockfile()` generates lockfile from manifest | No compilation or artifact fetching |

### 2.3 CAS Infrastructure (As-Is)

The CAS infrastructure is well-designed but underutilized:

| Component | Status | Gap |
|-----------|--------|-----|
| `ContentHash` (SHA-256) | Fully implemented in `glyim-macro-vfs/src/hash.rs` | Not used for package artifact identification |
| `ContentStore` trait | Defined in `glyim-macro-vfs/src/store.rs` with `store`, `retrieve`, `store_action_result`, `has_blobs` | Only implemented for macro expansion caching |
| `LocalContentStore` | Filesystem-backed CAS with sharded object storage | Not used for compilation artifacts |
| `RemoteContentStore` | HTTP-based remote CAS with local cache fallback | Exists but compiler never queries it |
| `ActionResult` | Structured action result with output files and exit codes | Not used for compilation results |
| CAS REST server | Axum server on port 9090 with blob and action endpoints | Running, but only used for macro verification |
| CAS gRPC server | Bazel REv2 `ContentAddressableStorage` on port 9091 | Implemented but no client uses it for builds |

### 2.4 Cross-Compilation and Linking (As-Is)

The cross-compilation support in `glyim-compiler/src/cross.rs` validates target triples and auto-installs cross-compilation toolchains, but the linker (`link_object()`) always invokes `cc` with a single object file. There is no support for linking multiple object files from different packages, no symbol resolution logic, and no awareness of library search paths or sysroot configurations from the manifest's `[target.*]` section.

### 2.5 Critical Gaps That Phase 5 Addresses

| Gap | Impact | Affected Crate | Phase 5 Solution |
|-----|--------|---------------|-------------------|
| No multi-module compilation | Projects limited to single source file | `glyim-compiler` | Package graph orchestrator compiles each module independently |
| No cross-module symbol resolution | Calls to other packages are unresolved externs | `glyim-codegen-llvm`, `glyim-compiler` | Symbol table export/import between packages |
| No dependency artifact caching | Every build recompiles all dependencies from source | `glyim-pkg`, `glyim-compiler` | CAS-backed pre-compiled object code storage and retrieval |
| Workspace not used for builds | Multi-package workspaces are detected but not compiled | `glyim-cli` | Workspace-aware `build` and `test` commands |
| Single-object linking | Cannot combine object code from multiple packages | `glyim-compiler` | Multi-object linker with package search paths |
| Lockfile hashes are placeholders | Cannot verify or retrieve dependency artifacts | `glyim-pkg` | Real content hashes from compiled artifacts |
| No remote build cache | Teams cannot share compilation artifacts | `glyim-pkg`, `glyim-cas-server` | `--remote-cache` flag with push/pull |

---

## 3. Architecture Design

### 3.1 Package Graph Orchestrator

The central architectural component of Phase 5 is the `PackageGraphOrchestrator`, a new struct that coordinates incremental compilation across an entire workspace. The orchestrator:

1. **Discovers the package graph** by reading `glyim.toml` manifests from the workspace root and all member directories, then resolving dependencies using the existing `resolver::resolve()` function.
2. **Computes a topological ordering** of the packages so that dependencies are always compiled before their dependents.
3. **Compiles each package** using the query-driven pipeline from Phase 4, producing per-function object code cached in the Merkle store.
4. **Links the final binary** from the object code of the root package and all its transitive dependencies.

```rust
// crates/glyim-orchestrator/src/lib.rs

use glyim_compiler::pipeline::{BuildMode, PipelineConfig};
use glyim_merkle::MerkleStore;
use glyim_pkg::manifest::PackageManifest;
use glyim_pkg::lockfile::Lockfile;
use glyim_query::IncrementalState;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub struct PackageGraphOrchestrator {
    /// Workspace root directory.
    workspace_root: PathBuf,
    /// All packages in the workspace, keyed by name.
    packages: HashMap<String, PackageNode>,
    /// Resolved dependency graph (topological order).
    build_order: Vec<String>,
    /// Merkle store for cross-package artifact caching.
    merkle: Arc<MerkleStore>,
    /// CAS client for remote artifact sharing.
    cas: Option<glyim_pkg::cas_client::CasClient>,
    /// Build configuration.
    config: OrchestratorConfig,
    /// Diagnostic report.
    report: OrchestratorReport,
}

pub struct PackageNode {
    /// Package manifest.
    manifest: PackageManifest,
    /// Directory containing the package.
    dir: PathBuf,
    /// Source file path (src/main.g).
    main_source: PathBuf,
    /// Content hash of the package's source (for CAS keying).
    source_hash: Option<glyim_macro_vfs::ContentHash>,
    /// Per-package incremental state from Phase 4.
    incremental_state: Option<IncrementalState>,
    /// Compilation result (populated after compilation).
    result: Option<PackageCompilationResult>,
}

#[derive(Debug, Clone)]
pub struct OrchestratorConfig {
    pub mode: BuildMode,
    pub target: Option<String>,
    pub remote_cache_url: Option<String>,
    pub remote_cache_token: Option<String>,
    pub force_rebuild: bool,
}

#[derive(Debug, Clone, Default)]
pub struct OrchestratorReport {
    pub packages_compiled: Vec<String>,
    pub packages_cached: Vec<String>,
    pub packages_failed: Vec<String>,
    pub total_elapsed: std::time::Duration,
    pub per_package_timing: Vec<(String, std::time::Duration)>,
    pub artifacts_pushed: usize,
    pub artifacts_pulled: usize,
}
```

### 3.2 Build Flow

The orchestrator's `build()` method follows this flow:

```
1. Discover workspace / detect single package
2. Load manifests for all packages
3. Resolve dependencies → produce Lockfile
4. Topological sort → build_order
5. For each package in build_order:
   a. Compute source hash
   b. Check CAS for pre-compiled artifact (if remote cache enabled)
   c. If cached: download and skip compilation
   d. If not cached: compile using Phase 4 query pipeline
   e. Store per-function object code in Merkle store
   f. Store package artifact in CAS (if remote cache enabled)
6. Resolve cross-module symbols
7. Link all object code into final binary
8. Report diagnostics
```

### 3.3 Package-Level Merkle Root

Each package gets a Merkle root computed from the semantic hashes of its HIR items (using the existing `compute_root_hash()` from `glyim-merkle/src/root.rs`). This root is the cache key for the entire package's compilation output. When a source file changes in one package, only that package and its transitive dependents need recompilation; all other packages reuse their cached artifacts.

The Merkle root is stored in the lockfile alongside the package version and content hash, replacing the placeholder zeroes currently in `lockfile_integration.rs`. This enables `glyim fetch` to verify that a pre-compiled artifact matches the expected Merkle root before using it.

### 3.4 Cross-Module Symbol Resolution

Cross-module calls in the current compiler are represented as `HirExpr::Call(name, args)` where `name` is a `Symbol` resolved by the interner. In a single-module world, the called function is always defined in the same `Hir`. In a multi-module world, the called function may be defined in a different package.

Phase 5 introduces a `PackageSymbolTable` that maps `(package_name, symbol_name) → ContentHash` for every exported function. When the compiler encounters a `Call` to a function not defined in the current module, it consults the symbol table to determine which package provides that function, and generates an appropriate extern declaration in the LLVM module.

```rust
// crates/glyim-orchestrator/src/symbols.rs

use glyim_interner::Symbol;
use glyim_macro_vfs::ContentHash;
use std::collections::HashMap;

/// Maps exported symbols to their defining package and artifact hash.
pub struct PackageSymbolTable {
    /// (symbol_name) → (package_name, content_hash_of_object_code)
    exports: HashMap<Symbol, (String, ContentHash)>,
    /// (package_name) → Vec<Symbol>  — all symbols exported by a package
    package_exports: HashMap<String, Vec<Symbol>>,
}

impl PackageSymbolTable {
    pub fn new() -> Self { ... }
    pub fn register_export(&mut self, package: &str, symbol: Symbol, artifact_hash: ContentHash) { ... }
    pub fn resolve(&self, symbol: Symbol) -> Option<(&str, ContentHash)> { ... }
    pub fn package_exports(&self, package: &str) -> &[Symbol] { ... }
}
```

The symbol table is populated during the compilation of each package: after `Codegen::generate()` emits the LLVM module, the orchestrator extracts the list of non-static function symbols from the module (using `module.get_first_function()` / `get_next_function()` iteration) and registers them in the table. When a downstream package is compiled, the orchestrator injects the appropriate `extern` declarations for all symbols it needs before passing the HIR to the code generator.

### 3.5 Multi-Object Linking

The current `link_object()` function in `pipeline.rs` links a single `.o` file:

```rust
fn link_object(obj_path: &Path, output_path: &Path, use_lto: bool) -> Result<(), PipelineError>
```

Phase 5 replaces this with a multi-object linker that accepts a list of object file paths:

```rust
// crates/glyim-orchestrator/src/linker.rs

pub fn link_multi_object(
    object_paths: &[PathBuf],
    output_path: &Path,
    config: &LinkConfig,
) -> Result<(), LinkError> { ... }

pub struct LinkConfig {
    pub linker: Option<String>,         // override from [target.*] linker
    pub sysroot: Option<PathBuf>,       // override from [target.*] sysroot
    pub use_lto: bool,                  // thin LTO for release builds
    pub library_search_paths: Vec<PathBuf>,  // -L paths for dependencies
    pub target_triple: Option<String>,
}
```

The multi-object linker invokes `cc` (or the configured linker) with all object file paths, library search paths, and the appropriate flags. Object files from dependency packages are located via their Merkle store entries: each package's compiled object code is stored as a `MerkleNodeData::ObjectCode`, and the linker retrieves the `.o` bytes, writes them to a temporary directory, and passes their paths to the linker.

---

## 4. New Crate: `glyim-orchestrator`

### 4.1 Crate Structure

```
crates/glyim-orchestrator/
├── Cargo.toml
└── src/
    ├── lib.rs           — public API, re-exports
    ├── orchestrator.rs  — PackageGraphOrchestrator
    ├── graph.rs         — package graph discovery and topological sort
    ├── symbols.rs       — PackageSymbolTable
    ├── linker.rs        — multi-object linking
    ├── artifacts.rs     — CAS artifact storage and retrieval
    ├── incremental.rs   — cross-package incremental state coordination
    └── tests/
        ├── mod.rs
        ├── graph_tests.rs
        ├── symbols_tests.rs
        ├── linker_tests.rs
        ├── artifacts_tests.rs
        └── orchestrator_tests.rs
```

### 4.2 Cargo.toml

```toml
[package]
name = "glyim-orchestrator"
version.workspace = true
edition.workspace = true
license.workspace = true
rust-version.workspace = true
description = "Cross-module incremental compilation orchestrator for Glyim"

[dependencies]
glyim-compiler = { path = "../glyim-compiler" }
glyim-hir = { path = "../glyim-hir" }
glyim-interner = { path = "../glyim-interner" }
glyim-codegen-llvm = { path = "../glyim-codegen-llvm" }
glyim-query = { path = "../glyim-query" }
glyim-merkle = { path = "../glyim-merkle" }
glyim-macro-vfs = { path = "../glyim-macro-vfs" }
glyim-pkg = { path = "../glyim-pkg" }
petgraph = "0.7"
serde = { version = "1", features = ["derive"] }
sha2 = "0.11"
tracing = "0.1"
tempfile = "3"

[dev-dependencies]
glyim-parse = { path = "../glyim-parse" }
glyim-typeck = { path = "../glyim-typeck" }
```

### 4.3 Package Graph Discovery (`graph.rs`)

The package graph is discovered by walking the workspace structure:

1. Call `workspace::detect_workspace()` to find the workspace root and member directories.
2. For each member directory, load `glyim.toml` using `manifest::load_manifest()`.
3. Parse the `[dependencies]` and `[macros]` sections to build a directed graph where each node is a package and each edge is a dependency.
4. Topological sort the graph using `petgraph::algo::toposort()`.
5. Detect cycles and report errors.

For single-package projects (no workspace), the graph has one node with no edges. This ensures the orchestrator works for both workspace and standalone projects.

```rust
// crates/glyim-orchestrator/src/graph.rs

use glyim_pkg::manifest::PackageManifest;
use petgraph::graph::DiGraph;
use petgraph::algo::toposort;
use std::collections::HashMap;
use std::path::PathBuf;

pub struct PackageGraph {
    /// The petgraph DiGraph. Node indices correspond to package indices.
    graph: DiGraph<PackageNode, DependencyEdge>,
    /// Package name → node index mapping.
    name_to_idx: HashMap<String, petgraph::graph::NodeIndex>,
}

pub struct PackageNode {
    pub name: String,
    pub dir: PathBuf,
    pub manifest: PackageManifest,
}

pub struct DependencyEdge {
    pub dep_name: String,
    pub is_macro: bool,
}

impl PackageGraph {
    /// Discover the package graph from a workspace root or a single package.
    pub fn discover(root: &std::path::Path) -> Result<Self, GraphError> { ... }

    /// Return packages in topological (build) order.
    pub fn build_order(&self) -> Vec<&PackageNode> {
        let order = toposort(&self.graph, None).map_err(|_| GraphError::Cycle)?;
        order.iter().map(|idx| &self.graph[*idx]).collect()
    }

    /// Return all packages that directly depend on the given package.
    pub fn dependents(&self, package_name: &str) -> Vec<&PackageNode> { ... }

    /// Return all transitive dependents (packages affected by a change).
    pub fn transitive_dependents(&self, package_name: &str) -> Vec<&PackageNode> { ... }
}
```

### 4.4 Cross-Package Incremental State (`incremental.rs`)

Phase 4 introduced per-package `IncrementalState` for single-module incremental compilation. Phase 5 coordinates these states across the package graph. When a source file changes in package A, the orchestrator must:

1. Recompile package A using its local `IncrementalState`.
2. Determine which packages depend on A (transitively).
3. For each dependent package B, check whether any of B's dependencies changed (by comparing A's new Merkle root with the old one stored in B's `IncrementalState`).
4. If B's dependencies changed, invalidate the queries in B that depend on A's artifacts.
5. Recompile B, and continue transitively.

```rust
// crates/glyim-orchestrator/src/incremental.rs

use glyim_macro_vfs::ContentHash;
use glyim_merkle::MerkleRoot;
use std::collections::HashMap;

/// Coordinates incremental state across multiple packages.
pub struct CrossPackageIncremental {
    /// Per-package Merkle roots from the previous build.
    package_roots: HashMap<String, ContentHash>,
    /// Per-package dependency fingerprints.
    /// Maps (package_name) → { (dep_name) → ContentHash_of_dep_at_last_build }
    dep_fingerprints: HashMap<String, HashMap<String, ContentHash>>,
}

impl CrossPackageIncremental {
    pub fn new() -> Self { ... }

    /// Load from the workspace's incremental state directory.
    pub fn load(workspace_root: &std::path::Path) -> Result<Self, String> { ... }

    /// Save to the workspace's incremental state directory.
    pub fn save(&self, workspace_root: &std::path::Path) -> Result<(), String> { ... }

    /// Determine which packages need recompilation given a set of changed packages.
    pub fn compute_affected_packages(
        &self,
        changed_packages: &[String],
        graph: &super::graph::PackageGraph,
    ) -> Vec<String> { ... }

    /// Update the Merkle root for a package after recompilation.
    pub fn update_package_root(&mut self, package: &str, root: ContentHash) { ... }
}
```

### 4.5 CAS Artifact Management (`artifacts.rs`)

The artifacts module manages the storage and retrieval of pre-compiled package artifacts in the CAS. Each package's compilation output is stored as a set of CAS blobs:

```
Package artifact for "math-lib v1.2.0":
  ┌─ merkle_root: ContentHash (key for the entire package)
  ├─ symbol_table: ContentHash → serialized PackageSymbolTable
  ├─ object_code: ContentHash → combined .o file for the package
  ├─ per_fn_objects: ContentHash → { fn_name: ContentHash } mapping
  └─ metadata: ContentHash → { version, target, opt_level, compiler_version }
```

When the orchestrator compiles a package, it stores all blobs in the local CAS and optionally pushes them to the remote CAS (if `--remote-cache` is enabled). When the orchestrator encounters a dependency that has not changed, it retrieves the pre-compiled object code from the CAS instead of recompiling from source.

```rust
// crates/glyim-orchestrator/src/artifacts.rs

use glyim_macro_vfs::{ContentHash, ContentStore};
use glyim_merkle::MerkleStore;
use std::path::PathBuf;
use std::sync::Arc;

/// Manages compilation artifacts in the CAS.
pub struct ArtifactManager {
    cas: Arc<dyn ContentStore>,
    merkle: Arc<MerkleStore>,
}

/// A package's complete compilation output.
#[derive(serde::Serialize, serde::Deserialize)]
pub struct PackageArtifact {
    pub package_name: String,
    pub version: String,
    pub merkle_root: ContentHash,
    pub symbol_table_hash: ContentHash,
    pub object_code_hash: ContentHash,
    pub per_fn_objects: Vec<(String, ContentHash)>,
    pub metadata_hash: ContentHash,
    pub target_triple: Option<String>,
    pub opt_level: String,
    pub compiler_version: String,
}

impl ArtifactManager {
    pub fn new(cas: Arc<dyn ContentStore>, merkle: Arc<MerkleStore>) -> Self { ... }

    /// Store a package's compilation output in the CAS.
    pub fn store_package_artifact(&self, artifact: &PackageArtifact) -> ContentHash { ... }

    /// Retrieve a package's compilation output from the CAS.
    pub fn retrieve_package_artifact(&self, hash: ContentHash) -> Option<PackageArtifact> { ... }

    /// Check if a package artifact exists in the CAS.
    pub fn has_package_artifact(&self, hash: ContentHash) -> bool { ... }

    /// Extract the object code for a package from the CAS and write to a temp file.
    pub fn extract_object_code(&self, artifact: &PackageArtifact) -> Result<PathBuf, String> { ... }

    /// Push a package artifact to the remote CAS.
    pub fn push_to_remote(&self, artifact: &PackageArtifact) -> Result<(), String> { ... }

    /// Pull a package artifact from the remote CAS.
    pub fn pull_from_remote(&self, hash: ContentHash) -> Result<PackageArtifact, String> { ... }
}
```

### 4.6 Orchestrator Entry Points

```rust
impl PackageGraphOrchestrator {
    /// Build the entire workspace (or a single package).
    pub fn build(&mut self) -> Result<PathBuf, OrchestratorError> { ... }

    /// Check all packages for type errors without codegen.
    pub fn check(&mut self) -> Result<(), OrchestratorError> { ... }

    /// Run the main package via JIT.
    pub fn run(&mut self) -> Result<i32, OrchestratorError> { ... }

    /// Run tests for all packages (or a specific package).
    pub fn test(&mut self, filter: Option<&str>) -> Result<TestSummary, OrchestratorError> { ... }

    /// Fetch pre-compiled dependencies from the CAS.
    pub fn fetch(&mut self) -> Result<FetchSummary, OrchestratorError> { ... }

    /// Publish a package to the registry with pre-compiled artifacts.
    pub fn publish(&mut self, dry_run: bool) -> Result<(), OrchestratorError> { ... }
}
```

---

## 5. Cross-Module Compilation Protocol

### 5.1 Compilation Order

Packages are compiled in topological order. For each package:

1. **Resolve symbols from dependencies:** Load the `PackageSymbolTable` for all dependency packages (already compiled). This table tells the compiler which functions are available as externs and their mangled symbol names.
2. **Inject extern declarations:** Before HIR lowering, inject `HirItem::Extern` declarations for every function that the package calls from its dependencies. These externs are derived from the dependency symbol tables.
3. **Compile using Phase 4 pipeline:** Run the query-driven incremental pipeline on the package's source, using the injected externs. The pipeline produces per-function object code.
4. **Export symbols:** After compilation, extract the list of non-static function symbols from the generated LLVM module. Register them in the `PackageSymbolTable`.
5. **Store artifacts:** Store the package's object code, symbol table, and Merkle root in the CAS.

### 5.2 Symbol Name Mangling

Cross-module symbols must be uniquely named to avoid collisions. The mangling scheme follows the existing `glyim-hir/src/monomorphize/mangling.rs` pattern but adds a package prefix:

```
Current:  <fn_name>
Phase 5:  <package_name>::<fn_name>
```

For example, a function `add` in package `math-lib` becomes the LLVM symbol `math_lib::add`. This ensures that two packages can both define a function named `add` without colliding. The mangling is applied during the `declare_fn()` pass in `glyim-codegen-llvm/src/codegen/function.rs`, which already has access to the function name via the interner.

For the root package (the one containing `main`), the `main` function is never mangled — it must remain `main` for the linker and the C runtime.

### 5.3 Extern Declaration Injection

When a package calls a function from a dependency, the compiler needs to know the function's type signature to generate correct LLVM IR. Phase 5 introduces a `DependencyInterface` that summarizes the public API of each package:

```rust
// crates/glyim-orchestrator/src/interface.rs

use glyim_hir::types::HirType;
use glyim_interner::Symbol;
use serde::{Serialize, Deserialize};

/// The public interface of a package, used by dependents for type-safe compilation.
#[derive(Serialize, Deserialize, Clone)]
pub struct DependencyInterface {
    pub package_name: String,
    pub version: String,
    pub functions: Vec<InterfaceFn>,
    pub structs: Vec<InterfaceStruct>,
    pub enums: Vec<InterfaceEnum>,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct InterfaceFn {
    pub name: Symbol,
    pub mangled_name: String,
    pub params: Vec<HirType>,
    pub return_type: HirType,
    pub is_pub: bool,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct InterfaceStruct {
    pub name: Symbol,
    pub fields: Vec<(Symbol, HirType)>,
    pub type_params: Vec<Symbol>,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct InterfaceEnum {
    pub name: Symbol,
    pub variants: Vec<(Symbol, Vec<(Symbol, HirType)>)>,
    pub type_params: Vec<Symbol>,
}
```

The `DependencyInterface` is computed after type checking each package. It contains only the public items (`is_pub: true`) and their fully resolved types (after monomorphization, all types are concrete). The interface is serialized and stored in the CAS alongside the object code, so that downstream packages can type-check calls to dependency functions without recompiling the dependency.

When compiling a downstream package, the orchestrator loads the `DependencyInterface` for each dependency and injects `HirItem::Extern` declarations for all exported functions. This ensures that the type checker can resolve cross-module calls, and the code generator can emit correct `declare` statements in the LLVM module.

---

## 6. CAS-Backed Dependency Sharing

### 6.1 Local CAS Flow

When compiling without a remote cache (`--remote-cache` not specified), all artifacts are stored in the local CAS:

1. After compiling a package, store the object code blob via `LocalContentStore::store()`.
2. Store the `PackageArtifact` metadata (listing all blob hashes) via `ContentStore::store_action_result()`.
3. Register the package name as a CAS name via `ContentStore::register_name()`, mapping to the `PackageArtifact` hash.

On subsequent builds, the orchestrator:

1. Computes the source hash of each package.
2. Looks up the package name in the CAS via `ContentStore::resolve_name()`.
3. If the stored `PackageArtifact`'s Merkle root matches the current source hash, the artifact is valid and the package can be skipped.
4. If the Merkle root differs, the package must be recompiled.

### 6.2 Remote CAS Flow

With `--remote-cache` enabled, the orchestrator also interacts with the remote CAS server:

1. **Before compilation:** For each dependency in the lockfile, query the remote CAS for a `PackageArtifact` matching the dependency's content hash. If found, download the artifact (object code + interface) and skip compilation.
2. **After compilation:** Push all newly compiled artifacts to the remote CAS. This includes the object code blob, the symbol table, the `DependencyInterface`, and the `PackageArtifact` metadata.

The remote CAS interaction uses the existing `RemoteContentStore` from `glyim-macro-vfs/src/remote.rs`, which already implements the `ContentStore` trait with local caching and remote fallback. The `CasClient` from `glyim-pkg/src/cas_client.rs` provides a convenient wrapper that supports both local-only and remote modes.

### 6.3 Artifact Verification

When pulling artifacts from a remote CAS, the orchestrator must verify their integrity:

1. **Hash verification:** The `ContentHash` of each blob is recomputed after download. If the hash doesn't match, the blob is discarded.
2. **Compiler version check:** The `PackageArtifact` metadata includes the `compiler_version` field. If the compiler version doesn't match the current compiler, the artifact is discarded (it may have been compiled with different codegen logic).
3. **Target triple check:** The artifact's `target_triple` must match the current build target. Cross-compiled artifacts are not interchangeable.
4. **Optimization level check:** Release and debug artifacts are not interchangeable (they have different codegen properties).

---

## 7. Lockfile Enhancement

### 7.1 Real Content Hashes

The current lockfile stores placeholder hashes for dependencies. Phase 5 replaces these with real content hashes derived from the Merkle root of each package's compiled output:

```rust
// Enhanced LockedPackage (glyim-pkg/src/lockfile.rs)

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LockedPackage {
    pub name: String,
    pub version: String,
    pub hash: String,                    // NOW: Merkle root hash (SHA-256)
    #[serde(default)]
    pub is_macro: bool,
    pub source: LockSource,
    #[serde(default)]
    pub deps: Vec<String>,
    // NEW FIELDS:
    #[serde(default)]
    pub artifact_hash: Option<String>,   // CAS hash of the PackageArtifact
    #[serde(default)]
    pub interface_hash: Option<String>,  // CAS hash of the DependencyInterface
    #[serde(default)]
    pub target_triple: Option<String>,   // target the artifact was compiled for
}
```

The `artifact_hash` and `interface_hash` enable `glyim fetch` to download pre-compiled artifacts directly from the CAS without needing to recompile. The `target_triple` ensures that artifacts are only reused for the correct target platform.

### 7.2 Lockfile Generation Update

The `resolve_and_write_lockfile()` function in `glyim-compiler/src/lockfile_integration.rs` is updated to compute real Merkle roots for path dependencies and store the artifact/interface hashes for registry dependencies. For registry dependencies, the hashes come from the registry's package metadata (which includes the compiled artifact hashes for each version).

---

## 8. CLI Integration

### 8.1 Modified Commands

| Command | Changes | New Flags |
|---------|---------|-----------|
| `glyim build` | Detects workspace, delegates to orchestrator | `--remote-cache <URL>`, `--target`, `--force-rebuild` |
| `glyim check` | Checks all packages in workspace | `--remote-cache <URL>` |
| `glyim run` | Builds and runs the root package via JIT | `--remote-cache <URL>` |
| `glyim test` | Runs tests across all workspace packages | `--remote-cache <URL>`, `--package <name>` |
| `glyim fetch` | Downloads pre-compiled artifacts from remote CAS | `--remote-cache <URL>` |
| `glyim publish` | Pushes compiled artifacts to registry + CAS | `--with-artifacts` |

### 8.2 Workspace Detection in Build Command

The `cmd_build()` function is refactored to detect whether the input path is within a workspace:

```rust
pub fn cmd_build(
    input: PathBuf,
    output: Option<PathBuf>,
    target: Option<String>,
    release: bool,
    bare: bool,
    incremental: bool,
    remote_cache: Option<String>,  // NEW
) -> i32 {
    let mode = if release { BuildMode::Release } else { BuildMode::Debug };

    if bare || !workspace_detected {
        // Single-file mode (existing behavior)
        pipeline::build_with_mode(&input, output.as_deref(), mode, target.as_deref(), None)
    } else {
        // Workspace mode (Phase 5)
        let config = OrchestratorConfig {
            mode,
            target,
            remote_cache_url: remote_cache,
            ..Default::default()
        };
        let mut orchestrator = PackageGraphOrchestrator::new(&input, config);
        orchestrator.build()
    }
}
```

### 8.3 `--remote-cache` Flag

The `--remote-cache` flag accepts a URL (e.g., `https://cas.company.com:9090`) and an optional authentication token (from `GLYIM_CACHE_TOKEN` environment variable). When specified:

- The orchestrator creates a `CasClient::new_with_remote()` with the given URL.
- Before compiling each package, it checks the remote CAS for a pre-compiled artifact.
- After compiling, it pushes the artifact to the remote CAS.
- The `OrchestratorReport` includes `artifacts_pushed` and `artifacts_pulled` counts.

---

## 9. Multi-Package Testing

### 9.1 Cross-Package Test Collection

The test runner's `collect_tests()` function (in `glyim-testr/src/collector.rs`) currently only collects tests from a single AST. Phase 5 extends it to collect tests from all packages in the workspace:

```rust
// In glyim-testr/src/collector.rs (extended)

pub fn collect_workspace_tests(
    packages: &[(String, glyim_parse::Ast, glyim_interner::Interner)],
    filter: Option<&str>,
    include_ignored: bool,
) -> Vec<WorkspaceTestDef> {
    let mut tests = Vec::new();
    for (pkg_name, ast, interner) in packages {
        let pkg_tests = collect_tests(ast, interner, filter, include_ignored);
        for test in pkg_tests {
            tests.push(WorkspaceTestDef {
                package_name: pkg_name.clone(),
                test_def: test,
            });
        }
    }
    tests
}
```

### 9.2 Incremental Test Execution Across Packages

The `TestDependencyGraph` from Phase 4 is extended to track cross-package dependencies. When a source file changes in package A, only tests that depend on package A (directly or transitively) are re-run. This requires the test dependency graph to include edges from test functions to the packages they import.

### 9.3 Test Execution Order

Tests are executed in dependency order: dependency package tests run before dependent package tests. This ensures that if a dependency has a regression, it is detected before running tests that depend on it.

---

## 10. Dependency Interface Serialization

### 10.1 Interface Computation

The `DependencyInterface` is computed after type checking a package. The orchestrator walks the package's `Hir` and extracts all public items:

1. **Functions:** For each `HirItem::Fn` with `is_pub: true`, record the name, mangled name, parameter types, and return type.
2. **Structs:** For each `HirItem::Struct`, record the name, fields, and type parameters.
3. **Enums:** For each `HirItem::Enum`, record the name, variants, and type parameters.

The types are resolved to their monomorphized forms (all generic parameters are substituted with concrete types) because the interface is consumed by downstream packages that only see the concrete API.

### 10.2 Interface Storage

The interface is serialized using `postcard` (consistent with the rest of the incremental state) and stored in the CAS as a blob. The `ContentHash` of the serialized interface is included in the `PackageArtifact` metadata and in the lockfile.

### 10.3 Interface Loading

When compiling a downstream package, the orchestrator:

1. Reads the lockfile to determine the dependency's `interface_hash`.
2. Queries the CAS for the serialized interface blob.
3. Deserializes the `DependencyInterface`.
4. Injects `HirItem::Extern` declarations for all exported functions.
5. Injects type definitions for all exported structs and enums.

This ensures that the downstream package's type checker can resolve calls to dependency functions without needing the dependency's source code.

---

## 11. Error Handling & Recovery

### 11.1 Package Compilation Failures

If a package fails to compile (parse error, type error, codegen error), the orchestrator:

1. Reports the error with the package name and source file.
2. Skips all packages that depend on the failed package (they cannot be compiled without the dependency).
3. Continues compiling packages that are not affected by the failure.
4. Returns a partial result indicating which packages succeeded and which failed.

### 11.2 CAS Unavailability

If the remote CAS is unavailable (network error, authentication failure):

1. Fall back to local-only compilation (no remote caching).
2. Log a warning that remote cache is unavailable.
3. Continue the build using local artifacts only.

### 11.3 Artifact Corruption

If a CAS artifact fails integrity verification (hash mismatch):

1. Discard the corrupted artifact.
2. Recompile the package from source.
3. Store the new artifact in the CAS, overwriting the corrupted one.
4. Log a warning about the corruption.

### 11.4 Circular Dependencies

If the package graph contains a cycle (detected during topological sort):

1. Report the cycle with the involved package names.
2. Fail the build with a clear error message.
3. Suggest that the user restructure the dependencies to break the cycle.

---

## 12. Testing Strategy

### 12.1 Unit Tests

| Test | Location | Description |
|------|----------|-------------|
| `graph_discovery_single` | `glyim-orchestrator/tests/` | Single-package graph has one node |
| `graph_discovery_workspace` | `glyim-orchestrator/tests/` | Workspace with 3 members produces correct graph |
| `graph_topological_order` | `glyim-orchestrator/tests/` | Dependencies compile before dependents |
| `graph_cycle_detection` | `glyim-orchestrator/tests/` | Circular dependencies are detected and reported |
| `symbol_table_registration` | `glyim-orchestrator/tests/` | Exported functions are registered in symbol table |
| `symbol_table_resolution` | `glyim-orchestrator/tests/` | Cross-module calls resolve to correct package |
| `symbol_mangling` | `glyim-orchestrator/tests/` | Package-qualified names don't collide |
| `artifact_store_retrieve` | `glyim-orchestrator/tests/` | Store and retrieve a package artifact from local CAS |
| `artifact_remote_push_pull` | `glyim-orchestrator/tests/` | Push to remote CAS, pull from another client |
| `interface_serialization` | `glyim-orchestrator/tests/` | DependencyInterface round-trips through serialization |
| `merkle_root_computation` | `glyim-orchestrator/tests/` | Package Merkle root changes when source changes |
| `multi_object_link` | `glyim-orchestrator/tests/` | Link two object files into a working binary |

### 12.2 Integration Tests

| Test | Location | Description |
|------|----------|-------------|
| `workspace_build_two_packages` | `glyim-cli-tests-full/` | Workspace with two packages builds and links correctly |
| `workspace_incremental_change` | `glyim-cli-tests-full/` | Change a dependency, verify dependent is recompiled |
| `workspace_incremental_no_change` | `glyim-cli-tests-full/` | Rebuild with no changes, verify all packages are cached |
| `cross_module_call` | `glyim-cli-tests-full/` | Call a function from a dependency package, verify result |
| `remote_cache_push_pull` | `glyim-cli-tests-full/` | Build with remote cache, then build from another machine |
| `fetch_downloads_artifacts` | `glyim-cli-tests-full/` | `glyim fetch` downloads pre-compiled dependency artifacts |
| `workspace_test_all` | `glyim-cli-tests-full/` | `glyim test` runs tests from all packages |
| `workspace_test_incremental` | `glyim-cli-tests-full/` | Change one package, only its tests and dependent tests re-run |
| `artifact_version_mismatch` | `glyim-cli-tests-full/` | Artifact from different compiler version is rejected |
| `artifact_target_mismatch` | `glyim-cli-tests-full/` | Artifact for wrong target triple is rejected |

### 12.3 Property Tests

| Property | Description |
|----------|-------------|
| `workspace_equals_sequential` | Compiling a workspace via orchestrator produces the same output as compiling each package sequentially |
| `cache_coherence` | A CAS artifact always produces the same binary when linked |
| `idempotent_orchestration` | Running `build` twice with no changes is idempotent |
| `topological_validity` | Build order always respects dependency edges |

---

## 13. Implementation Timeline

### Phase 5A: Package Graph & Discovery (5–7 days)

| Day | Task | Files |
|-----|------|-------|
| 1–2 | Create `glyim-orchestrator` crate with `graph.rs` | `crates/glyim-orchestrator/` |
| 3–4 | Implement `PackageGraph::discover()`, topological sort, cycle detection | `glyim-orchestrator/src/graph.rs` |
| 5–6 | Implement workspace detection integration with `glyim-pkg` | `glyim-orchestrator/src/graph.rs`, `glyim-pkg/src/workspace.rs` |
| 7 | Unit tests for graph discovery and topological sort | `glyim-orchestrator/tests/` |

### Phase 5B: Cross-Module Symbol Resolution (5–7 days)

| Day | Task | Files |
|-----|------|-------|
| 1–2 | Implement `PackageSymbolTable` with registration and resolution | `glyim-orchestrator/src/symbols.rs` |
| 3–4 | Implement symbol name mangling with package prefix | `glyim-codegen-llvm/src/codegen/function.rs` |
| 5–6 | Implement `DependencyInterface` computation and serialization | `glyim-orchestrator/src/interface.rs` |
| 7 | Unit tests for symbol resolution and interface serialization | `glyim-orchestrator/tests/` |

### Phase 5C: CAS Artifact Management (5–7 days)

| Day | Task | Files |
|-----|------|-------|
| 1–2 | Implement `ArtifactManager` with local CAS storage | `glyim-orchestrator/src/artifacts.rs` |
| 3–4 | Implement remote CAS push/pull with `RemoteContentStore` | `glyim-orchestrator/src/artifacts.rs` |
| 5–6 | Implement artifact verification (hash, version, target, opt level) | `glyim-orchestrator/src/artifacts.rs` |
| 7 | Unit tests for artifact storage, retrieval, and verification | `glyim-orchestrator/tests/` |

### Phase 5D: Multi-Object Linking (4–5 days)

| Day | Task | Files |
|-----|------|-------|
| 1–2 | Implement `link_multi_object()` replacing `link_object()` | `glyim-orchestrator/src/linker.rs` |
| 3–4 | Integrate with `LinkConfig` from manifest's `[target.*]` section | `glyim-orchestrator/src/linker.rs`, `glyim-pkg/src/manifest.rs` |
| 5 | Integration test: link two packages into a working binary | `glyim-cli-tests-full/` |

### Phase 5E: Orchestrator Integration (6–8 days)

| Day | Task | Files |
|-----|------|-------|
| 1–2 | Implement `PackageGraphOrchestrator::build()` | `glyim-orchestrator/src/orchestrator.rs` |
| 3–4 | Implement cross-package incremental state coordination | `glyim-orchestrator/src/incremental.rs` |
| 5–6 | Implement `check()`, `run()`, `test()`, `fetch()` entry points | `glyim-orchestrator/src/orchestrator.rs` |
| 7–8 | Integration tests: workspace build, incremental change, remote cache | `glyim-cli-tests-full/` |

### Phase 5F: CLI & Lockfile Updates (3–4 days)

| Day | Task | Files |
|-----|------|-------|
| 1 | Wire `--remote-cache` flag in CLI | `glyim-cli/src/commands/`, `glyim-cli/src/main.rs` |
| 2 | Update lockfile with real content hashes and artifact hashes | `glyim-pkg/src/lockfile.rs`, `glyim-compiler/src/lockfile_integration.rs` |
| 3 | Update `glyim fetch` to download pre-compiled artifacts | `glyim-cli/src/commands/cmd_fetch.rs` |
| 4 | Final integration tests and documentation | All crates |

### Total: 28–38 working days

---

## 14. Crate Dependency Changes

### 14.1 New Crate

| Crate | Tier | Dependencies | Description |
|-------|------|-------------|-------------|
| `glyim-orchestrator` | 5 | `glyim-compiler`, `glyim-hir`, `glyim-interner`, `glyim-codegen-llvm`, `glyim-query`, `glyim-merkle`, `glyim-macro-vfs`, `glyim-pkg`, `petgraph` | Cross-module incremental compilation orchestrator |

### 14.2 Modified Crates

| Crate | Changes |
|-------|---------|
| `glyim-compiler` | `link_object()` refactored to accept multiple paths; `pipeline.rs` gains workspace-aware entry points |
| `glyim-codegen-llvm` | Symbol name mangling with package prefix; extern declaration injection from `DependencyInterface` |
| `glyim-pkg` | `LockedPackage` gains `artifact_hash`, `interface_hash`, `target_triple` fields; `RegistryClient` gains artifact download capability |
| `glyim-testr` | `collect_tests()` extended for workspace; `TestDependencyGraph` extended with cross-package edges |
| `glyim-cli` | `--remote-cache` flag; workspace detection in build/run/test commands; `cmd_fetch` downloads artifacts |

### 14.3 Workspace Cargo.toml Update

```toml
[workspace]
members = [
    # ... existing members ...
    "crates/glyim-orchestrator",
]
```

### 14.4 Tier Assignment

```
Tier 1: glyim-interner, glyim-diag, glyim-syntax
Tier 2: glyim-lex, glyim-parse
Tier 3: glyim-hir, glyim-typeck, glyim-macro-core, glyim-macro-vfs, glyim-egraph
Tier 4: glyim-codegen-llvm
Tier 5: glyim-cli, glyim-cas-server, glyim-watch, glyim-orchestrator
```

`glyim-orchestrator` is tier 5 because it depends on `glyim-compiler` (tier 5). No tier violations are introduced.

---

## 15. Risk Register

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| Symbol name collisions across packages | Medium | Critical | Package-qualified mangling; integration test for collision cases |
| Dependency interface mismatches | Medium | High | Version checking; hash verification on interface load |
| Remote CAS unavailability | High | Medium | Graceful fallback to local-only; warning logged |
| Artifact corruption in CAS | Low | High | Hash verification on every retrieve; recompile on mismatch |
| Topological sort fails on complex graphs | Low | Critical | Cycle detection with clear error messages; petgraph handles this |
| Multi-object linking produces broken binary | Medium | Critical | Byte-level comparison with sequential compilation; property test |
| Lockfile format change breaks backward compatibility | Medium | Medium | Optional fields with `#[serde(default)]`; migration path |
| Performance regression for single-package projects | Medium | Medium | Fast-path detection: if no workspace, skip orchestrator entirely |
| Cross-compilation artifacts are target-specific | Low | Medium | `target_triple` field in artifact metadata; reject mismatches |
| Large workspaces cause excessive CAS queries | Medium | Low | Batch `has_blobs()` queries; cache artifact availability locally |

---

## 16. Performance Targets

| Metric | Target | Measurement |
|--------|--------|-------------|
| Workspace build (5 packages, all cached) | < 20ms | Time from `glyim build` to completion when all packages are in local CAS |
| Workspace build (5 packages, 1 changed) | < 50ms | Incremental build with one package recompiled |
| Remote CAS artifact pull | < 500ms per package | Time to download and verify a pre-compiled dependency |
| Remote CAS artifact push | < 1s per package | Time to upload a compiled package |
| Single-package build (no workspace) | < 5% regression | Must not slow down single-file builds |
| Link time (10 object files) | < 100ms | Multi-object link step |
| Package graph discovery | < 10ms | Scanning a 20-package workspace |

---

## 17. Migration Strategy

### 17.1 Backward Compatibility

The `glyim-orchestrator` crate is introduced as a new dependency of `glyim-cli`, but it is only activated when:

1. The input path is a directory (not a single `.g` file)
2. A `glyim.toml` with `[workspace]` section is detected
3. OR the `--remote-cache` flag is specified

Single-file builds (the current default) continue to use the existing pipeline without any changes. The `--bare` flag explicitly forces single-file mode even when a workspace is detected.

### 17.2 Lockfile Migration

The lockfile format is extended with optional fields (`artifact_hash`, `interface_hash`, `target_triple`). Old lockfiles without these fields continue to work — the `#[serde(default)]` annotation ensures that missing fields default to `None`. When `glyim build` rewrites the lockfile, it populates the new fields.

### 17.3 Gradual Feature Rollout

1. **Phase 5A–5C:** Orchestrator is available but not wired to CLI. Testing happens via unit tests and integration tests.
2. **Phase 5D–5E:** Workspace-aware `glyim build` is activated when a workspace is detected. Single-file mode is unchanged.
3. **Phase 5F:** Remote cache and lockfile updates are activated. `--remote-cache` flag is documented.

---

## 18. Success Criteria

Phase 5 is complete when all of the following are true:

1. `glyim build` in a workspace directory compiles all packages in dependency order and links them into a working binary
2. A change to one package triggers recompilation of only that package and its transitive dependents
3. `glyim fetch` downloads pre-compiled dependency artifacts from the remote CAS
4. `--remote-cache` pushes and pulls compilation artifacts from a shared CAS server
5. Cross-module function calls resolve correctly between packages in a workspace
6. The lockfile contains real content hashes (not placeholder zeroes)
7. `glyim test` runs tests across all workspace packages
8. Single-file builds (no workspace) have less than 5% performance regression
9. All property tests (`workspace_equals_sequential`, `cache_coherence`, `idempotent_orchestration`, `topological_validity`) pass
10. Artifact integrity verification catches corrupted, version-mismatched, and target-mismatched artifacts
