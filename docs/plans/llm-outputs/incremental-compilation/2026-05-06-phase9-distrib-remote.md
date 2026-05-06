# Glyim Incremental Compiler — Phase 9 Implementation Plan

## Distributed Compilation, Remote Build Execution & Build Farm Orchestration

**Codebase:** elcoosp-glyim v0.5.0  
**Rust Workspace | 21+ Crates | LLVM 22.1 / Inkwell 0.9**  
**Date:** 2026-05-07

---

## 1. Executive Summary

Phase 9 extends Glyim from a single-machine incremental compiler into a distributed compilation platform that supports remote build execution, team-scale artifact sharing, and build farm orchestration. Phases 0 through 8 assembled a production-ready incremental compiler with query-driven memoization, Merkle content-addressed storage, JIT micro-modules, e-graph optimization, cross-module incremental linking with CAS-backed dependency sharing, test-aware compilation with mutation testing, LSP-based IDE integration, and performance-hardened production readiness. Phase 5 introduced the `PackageGraphOrchestrator` with a `--remote-cache` flag that can push and pull pre-compiled artifacts from a shared CAS endpoint, and the `glyim-cas-server` crate already implements both REST and gRPC servers with Bazel REv2 `ContentAddressableStorage` and `ActionResult` APIs. However, the current remote caching is limited to whole-package artifact sharing: the orchestrator pushes a fully compiled package artifact after local compilation and pulls one before compilation starts. There is no mechanism to distribute individual compilation tasks to remote workers, schedule builds across a farm of machines, speculatively pre-compile dependencies, stream incremental results between workers, or enforce access control and quota management on shared infrastructure.

Phase 9 closes these gaps through four interconnected workstreams. **Remote execution** implements the Bazel REv2 `Execution` API alongside the existing `ContentAddressableStorage` API, enabling the orchestrator to send individual compilation actions (per-function codegen, per-package type checking, per-package optimization) to remote workers that execute them and return results. The remote execution protocol uses the existing `ActionResult` structure from `glyim-cas-server` but extends it with execution metadata (worker ID, execution time, resource usage) and supports streaming of stdout/stderr for long-running actions. Workers are stateless compute nodes that pull inputs from the CAS, execute the compilation action in a sandboxed environment, push outputs to the CAS, and return the `ActionResult`. This design means that any machine with the Glyim compiler binary can serve as a remote worker, and the orchestrator can distribute work across heterogeneous hardware (high-core-count build servers for parallel codegen, machines with large RAM for e-graph saturation, ARM nodes for cross-compilation).

**Build farm orchestration** introduces a `glyim-scheduler` crate that manages a pool of remote workers, assigns compilation actions to workers based on resource requirements and availability, implements priority scheduling (interactive LSP requests get highest priority, CI builds get medium priority, speculative pre-compilation gets lowest priority), and handles worker health monitoring and fault recovery. The scheduler is a standalone service that communicates with both the orchestrator (which submits build requests) and the workers (which request work). It maintains a work queue ordered by priority and dependency constraints, tracks in-flight actions for deduplication, and retries failed actions on different workers. The scheduler also implements speculative pre-compilation: when a developer opens a file in the IDE, the scheduler predicts which other files might change (based on the reference graph from Phase 7) and pre-compiles them on idle workers, so that incremental recompilation results are already cached when the actual edit arrives.

**Team-scale artifact sharing** extends the remote CAS with access control, quota management, and artifact retention policies. The current CAS server has no authentication, no per-team isolation, and no retention policy — blobs accumulate indefinitely. Phase 9 adds token-based authentication (OAuth 2.0 bearer tokens or API keys), per-namespace artifact isolation (teams can share artifacts within their namespace but not across namespaces), reference-counted garbage collection (artifacts that are no longer referenced by any lockfile are eligible for deletion), and configurable retention policies (keep the last N versions of each artifact, or keep artifacts for D days). The CAS server also gains a garbage collection daemon that runs periodically, identifies unreferenced blobs, and deletes them to reclaim disk space.

**Cross-compilation at scale** leverages the remote execution infrastructure to enable cross-compilation without requiring developers to install cross-compilation toolchains locally. The orchestrator can submit compilation actions with a `target_triple` field, and workers that match the target (e.g., ARM workers for `aarch64-unknown-linux-gnu` builds) execute the action using their locally installed cross-compilation toolchain. This eliminates the need for developers to install and maintain cross-compilation sysroots, which is a significant operational burden for teams that target multiple platforms.

**Estimated effort:** 35–48 working days.

**Key deliverables:**
- Bazel REv2 `Execution` API implementation in `glyim-cas-server`
- Remote worker daemon (`glyim-worker`) that executes compilation actions from the CAS
- Build scheduler service (`glyim-scheduler`) with priority queuing, deduplication, and worker health monitoring
- Speculative pre-compilation based on reference graph analysis
- Orchestrator integration: local compilation falls back to remote execution for oversized actions
- Token-based authentication and per-namespace isolation for the CAS server
- Reference-counted garbage collection with configurable retention policies
- Cross-compilation dispatch to target-matching workers
- Streaming execution logs (stdout/stderr) for long-running actions
- `glyim build --remote-execution <scheduler-url>` CLI flag
- `glyim worker` CLI command for starting remote worker daemons
- `glyim scheduler` CLI command for starting the scheduler service
- Integration tests for distributed compilation with simulated worker pools

---

## 2. Current Codebase State Assessment

### 2.1 CAS Infrastructure (As-Is)

The `glyim-cas-server` crate provides both REST and gRPC servers for content-addressed storage:

| Component | Status | Gap |
|-----------|--------|-----|
| REST CAS server (`axum` on port 9090) | Fully implemented with blob upload/download, action result storage, name registration, capability reporting | No authentication, no namespace isolation, no retention policy |
| gRPC CAS server (Bazel REv2 `ContentAddressableStorage` on port 9091) | Implements `FindMissingBlobs`, `BatchUpdateBlobs`, `BatchReadBlobs`, `GetTree` | No `Execution` API — the `Action`/`Command`/`Execute` RPCs are not implemented |
| `LocalContentStore` | Filesystem-backed CAS with sharded object storage, `store()`, `retrieve()`, `store_action_result()`, `has_blobs()`, `register_name()` | No garbage collection; blobs accumulate indefinitely |
| `RemoteContentStore` | HTTP client for remote CAS with local cache fallback | No streaming, no authentication headers |
| `ContentHash` (SHA-256) | Fully implemented in `glyim-macro-vfs/src/hash.rs` | Used consistently; no issues |
| `ActionResult` | Structured result with output file digests, exit code, stdout/stderr digests | Not used for actual compilation actions; only for macro verification |
| `CasClient` (in `glyim-pkg`) | Wraps `LocalContentStore` or `RemoteContentStore` | No remote execution support; only push/pull |

### 2.2 Orchestrator (As-Is from Phase 5)

The `PackageGraphOrchestrator` in `glyim-orchestrator` coordinates incremental compilation across a workspace:

| Component | Status | Gap |
|-----------|--------|-----|
| Package graph discovery | Fully implemented with workspace detection, manifest loading, topological sort | No distributed awareness — assumes all packages are locally available |
| `PackageSymbolTable` | Cross-module symbol resolution for multi-package builds | Not exposed for remote consumption |
| `DependencyInterface` | Serialized type signatures for cross-package compilation | Stored in CAS but not used for remote execution planning |
| Multi-object linker | Links object code from multiple packages | Always runs locally |
| CAS artifact management (`ArtifactManager`) | Stores/retrieves `PackageArtifact` in local and remote CAS | Only stores final artifacts; no per-action granularity |
| `--remote-cache` flag | Pushes/pulls pre-compiled artifacts to/from remote CAS | Only caching, no execution — the orchestrator always compiles locally first, then pushes |
| `OrchestratorReport` | Reports timing, cache hits, artifacts pushed/pulled | No remote execution metrics |
| `CrossPackageIncremental` | Tracks per-package Merkle roots for incremental invalidation | No support for distributed incremental state |

### 2.3 Existing Protocols (As-Is)

| Protocol | Status | Gap |
|----------|--------|-----|
| Bazel REv2 CAS | gRPC server on port 9091 | Only `ContentAddressableStorage` API; no `Execution` API |
| Bazel REv2 Capabilities | `GetCapabilities()` returns CAS and execution capabilities | Reports `Execution` as unsupported |
| CAS REST API | `axum` on port 9090 with blob, action, and name endpoints | No streaming, no chunked uploads, no multipart |
| gRPC reflection | Not implemented | Useful for debugging and client generation |

### 2.4 Build Scheduling (As-Is)

There is no build scheduling infrastructure. The orchestrator compiles packages sequentially in topological order, with no parallelism, no priority, and no resource awareness. The only concurrency is within a single package's compilation (the query engine from Phase 4 can execute independent queries in parallel within a single `QueryContext`, but this is single-threaded in the current implementation because `QueryContext` requires `&mut self`).

| Concern | Current State | Gap |
|---------|--------------|-----|
| Package-level parallelism | Sequential topological iteration | Independent packages at the same depth level could compile in parallel |
| Action scheduling | No scheduling; all work is local | No remote execution, no worker pool, no priority queue |
| Deduplication | No deduplication; same action may execute multiple times | Multiple developers building the same code waste resources |
| Speculative pre-compilation | Not implemented | IDE edits trigger synchronous recompilation; no predictive caching |
| Resource management | No resource tracking | No awareness of worker CPU, memory, or target platform |

### 2.5 Critical Gaps That Phase 9 Addresses

| Gap | Impact | Affected Crate | Phase 9 Solution |
|-----|--------|---------------|-------------------|
| No remote execution | Large workspaces compile slowly on developer laptops; cannot leverage CI/CD build farms | `glyim-orchestrator`, `glyim-cas-server` | Bazel REv2 `Execution` API + `glyim-worker` daemon |
| No build scheduler | No way to distribute work across multiple workers; no priority or deduplication | (missing) | New `glyim-scheduler` crate with priority queue and worker pool |
| No speculative pre-compilation | IDE edits always wait for synchronous compilation; no predictive caching | `glyim-orchestrator` | Reference-graph-driven speculative compilation on idle workers |
| No CAS authentication | Anyone with network access can read/write artifacts; no team isolation | `glyim-cas-server` | Token-based auth, per-namespace isolation, RBAC |
| No CAS garbage collection | CAS disk usage grows without bound; no retention policy | `glyim-cas-server` | Reference-counted GC daemon with configurable retention |
| No cross-compilation dispatch | Developers must install cross-compilation toolchains locally | `glyim-orchestrator`, `glyim-compiler` | Target-aware worker selection; dispatch to matching workers |
| No streaming execution logs | Long-running remote actions provide no feedback until completion | `glyim-cas-server`, `glyim-worker` | gRPC streaming for stdout/stderr during execution |
| No package-level parallelism | Independent packages compile sequentially even when resources are available | `glyim-orchestrator` | Parallel compilation of independent packages in topological layers |
| No execution metrics | Cannot measure remote execution efficiency, worker utilization, or cache effectiveness | `glyim-bench` | Distributed build metrics in `CompilationProfile` |

---

## 3. Architecture Design

### 3.1 Distributed Compilation Architecture

The distributed compilation architecture extends the existing CAS-based workflow with remote execution, scheduling, and worker management:

```
┌──────────────────────────────────────────────────────────────────────┐
│                     Developer Machine (Client)                        │
│  ┌────────────────────┐    ┌─────────────────────────────────────┐  │
│  │  glyim CLI / LSP   │───▶│  PackageGraphOrchestrator           │  │
│  │  (build, check,    │    │  (submits actions to scheduler,     │  │
│  │   lsp, test)       │    │   falls back to local compilation)  │  │
│  └────────────────────┘    └───────────────┬─────────────────────┘  │
│                                            │                        │
│                    ┌───────────────────────▼───────────────────┐    │
│                    │         CAS Server (port 9090/9091)        │    │
│                    │  Blob Storage | Action Results | Names     │    │
│                    │  Auth | Namespaces | GC Daemon             │    │
│                    └───────────────────────┬───────────────────┘    │
│                                            │                        │
└────────────────────────────────────────────┼────────────────────────┘
                                             │ gRPC / HTTP
                    ┌─────────────────────────▼──────────────────────┐
                    │          Scheduler Service (port 9092)          │
                    │  ┌──────────────────────────────────────────┐  │
                    │  │  Priority Queue                          │  │
                    │  │  Critical > High > Medium > Low > Spec   │  │
                    │  └──────────────────┬───────────────────────┘  │
                    │  ┌──────────────────▼───────────────────────┐  │
                    │  │  Action Deduplication                    │  │
                    │  │  (same action digest → same result)      │  │
                    │  └──────────────────┬───────────────────────┘  │
                    │  ┌──────────────────▼───────────────────────┐  │
                    │  │  Worker Pool Manager                     │  │
                    │  │  Health | Capacity | Target Matching     │  │
                    │  └──────────────────────────────────────────┘  │
                    └─────────────────────────────────────────────────┘
                                             │
              ┌──────────────────────────────┼──────────────────────────┐
              │                              │                          │
   ┌──────────▼──────────┐     ┌─────────────▼──────────┐   ┌─────────▼──────────┐
   │   Worker Node A     │     │    Worker Node B       │   │   Worker Node C    │
   │  (x86_64-linux)     │     │  (aarch64-linux)       │   │  (x86_64-darwin)   │
   │  32 cores, 64GB     │     │  8 cores, 16GB         │   │  12 cores, 32GB    │
   │  ┌───────────────┐  │     │  ┌───────────────────┐ │   │  ┌───────────────┐  │
   │  │ glyim-worker  │  │     │  │  glyim-worker     │ │   │  │ glyim-worker  │  │
   │  │ (pulls work,  │  │     │  │  (pulls work,     │ │   │  │ (pulls work,  │  │
   │  │  executes,    │  │     │  │   executes,       │ │   │  │  executes,    │  │
   │  │  pushes       │  │     │  │   pushes           │ │   │  │  pushes       │  │
   │  │  results)     │  │     │  │   results)         │ │   │  │  results)     │  │
   │  └───────────────┘  │     │  └───────────────────┘ │   │  └───────────────┘  │
   └─────────────────────┘     └────────────────────────┘   └────────────────────┘
```

### 3.2 Remote Execution Protocol

The remote execution protocol follows the Bazel REv2 specification. The orchestrator (acting as the execution client) submits an `Action` to the scheduler, which assigns it to a worker. The worker executes the action and returns an `ActionResult`.

An `Action` in the Glyim context is a compilation step that can be executed independently. The granularities of actions are:

| Action Type | Description | Inputs | Outputs | Typical Duration |
|-------------|-------------|--------|---------|------------------|
| `ParseAndLower` | Parse source → HIR lowering → type check | Source file, prelude | Serialized `CompiledHir` | 10–100ms |
| `EgraphOptimize` | Run e-graph equality saturation on a function | Serialized `HirFn`, config | Optimized `HirFn` | 50–2000ms |
| `CodegenFunction` | LLVM codegen for a single function | Monomorphized `HirFn`, target triple | Object code bytes | 50–500ms |
| `CodegenPackage` | LLVM codegen for all functions in a package | `CompiledHir`, target triple | Combined `.o` file | 200ms–5s |
| `LinkBinary` | Link object files into executable | Object files, link config | Binary | 50–200ms |

The `Action` and `Command` structures follow the Bazel REv2 protobuf definitions:

```rust
// crates/glyim-cas-server/src/execution/types.rs

use serde::{Serialize, Deserialize};
use glyim_macro_vfs::ContentHash;

/// A compilation action that can be executed remotely.
/// Follows the Bazel REv2 Action proto structure.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Action {
    /// The digest of the Command message (what to execute).
    pub command_digest: Digest,
    /// The digest of the input root Directory message (input files).
    pub input_root_digest: Digest,
    /// Timeout for the action. If the worker exceeds this, the action is killed.
    pub timeout: Option<std::time::Duration>,
    /// Whether to accept cached results from a previous execution.
    pub do_not_cache: bool,
    /// Platform requirements for the worker (target triple, min RAM, etc.).
    pub platform: Platform,
}

/// The command to execute on the worker.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Command {
    /// The arguments to pass to the Glyim compiler binary.
    pub arguments: Vec<String>,
    /// Environment variables to set.
    pub environment_variables: Vec<EnvironmentVariable>,
    /// Files that the command is expected to produce (output paths).
    pub output_paths: Vec<String>,
    /// Working directory relative to the input root.
    pub working_directory: Option<String>,
}

/// A digest of a message (hash + size).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct Digest {
    /// The SHA-256 hash of the message.
    pub hash: ContentHash,
    /// The size of the message in bytes.
    pub size_bytes: u64,
}

/// Platform constraints for action execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Platform {
    /// Key-value properties describing the required platform.
    /// Example: [("target_triple", "aarch64-unknown-linux-gnu"),
    ///           ("min_ram_gb", "16")]
    pub properties: Vec<PlatformProperty>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlatformProperty {
    pub name: String,
    pub value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnvironmentVariable {
    pub name: String,
    pub value: String,
}
```

### 3.3 Action Serialization and CAS Storage

Before submitting an action for remote execution, the orchestrator serializes the `Command` and the input files (source code, serialized HIR, configuration) into the CAS. The `Action` message references these blobs by their content hashes. This ensures that:

1. **Inputs are immutable**: The worker reads inputs from the CAS by hash, so there is no risk of race conditions from concurrent modifications.
2. **Actions are deduplicatable**: Two identical actions (same command, same inputs) have the same `Action` digest, so the scheduler can return the cached result instead of executing the action twice.
3. **Results are cacheable**: The `ActionResult` is keyed by the `Action` digest, so subsequent requests for the same action can be served from cache.

The input root is a directory tree serialized using the Bazel REv2 `Directory` / `DirectoryNode` / `FileNode` protocol:

```rust
// crates/glyim-cas-server/src/execution/tree.rs

/// A directory in the input root, following Bazel REv2 Tree proto.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Directory {
    /// Files in this directory.
    pub files: Vec<FileNode>,
    /// Subdirectories.
    pub directories: Vec<DirectoryNode>,
    /// Symlinks (not used by Glyim but required by REv2).
    pub symlinks: Vec<SymlinkNode>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileNode {
    /// The file name (not the full path).
    pub name: String,
    /// The digest of the file's contents.
    pub digest: Digest,
    /// Whether the file is executable.
    pub is_executable: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DirectoryNode {
    /// The directory name.
    pub name: String,
    /// The digest of the child Directory message.
    pub digest: Digest,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SymlinkNode {
    pub name: String,
    pub target: String,
}
```

### 3.4 Execution Result Streaming

Long-running compilation actions (especially e-graph saturation and full-package codegen) need to provide incremental feedback. Phase 9 implements execution result streaming using gRPC server-streaming RPCs. The `Execute` method returns a stream of `Operation` messages, each containing a partial result:

```rust
// crates/glyim-cas-server/src/execution/operation.rs

/// An operation representing an ongoing or completed execution.
/// Follows the Bazel REv2 Operation proto (google.longrunning.Operation).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Operation {
    /// The name of the operation (unique identifier).
    pub name: String,
    /// Whether the operation is done.
    pub done: bool,
    /// The result of the operation (present when done = true).
    pub result: Option<OperationResult>,
    /// Metadata about the execution (present during and after execution).
    pub metadata: Option<ExecuteOperationMetadata>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum OperationResult {
    /// Successful execution.
    Success(ActionResult),
    /// Failed execution.
    Failure(ExecutionFailure),
}

/// Metadata about an in-progress execution, streamed to the client.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecuteOperationMetadata {
    /// The current stage of execution.
    pub stage: ExecutionStage,
    /// The digest of the Action being executed.
    pub action_digest: Digest,
    /// Stdout produced so far (partial, streamed).
    pub partial_stdout: Option<String>,
    /// Stderr produced so far (partial, streamed).
    pub partial_stderr: Option<String>,
    /// Worker ID executing this action.
    pub worker_id: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum ExecutionStage {
    /// The action is queued, waiting for a worker.
    Queued,
    /// Input files are being fetched from the CAS.
    FetchingInputs,
    /// The action is executing on a worker.
    Executing,
    /// Output files are being uploaded to the CAS.
    UploadingOutputs,
    /// The action has completed.
    Completed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionFailure {
    pub exit_code: i32,
    pub stderr: Option<String>,
    pub timeout_expired: bool,
    pub worker_error: Option<String>,
}
```

The streaming protocol allows the client (orchestrator or CLI) to display progress indicators for remote compilation, show partial error messages as they appear, and cancel long-running actions.

---

## 4. New Crate: `glyim-scheduler`

### 4.1 Crate Structure

```
crates/glyim-scheduler/
├── Cargo.toml
└── src/
    ├── lib.rs              — public API, re-exports
    ├── scheduler.rs        — Scheduler core (priority queue, dedup, dispatch)
    ├── queue.rs            — Priority action queue with dependency tracking
    ├── worker_pool.rs      — Worker pool management (registration, health, capacity)
    ├── dedup.rs            — Action deduplication (same digest → cached result)
    ├── speculative.rs      — Speculative pre-compilation engine
    ├── platform_match.rs   — Platform property matching for action→worker assignment
    ├── metrics.rs          — Scheduler metrics (queue depth, wait time, throughput)
    ├── config.rs           — Scheduler configuration
    └── tests/
        ├── mod.rs
        ├── scheduler_tests.rs
        ├── queue_tests.rs
        ├── worker_pool_tests.rs
        ├── dedup_tests.rs
        ├── speculative_tests.rs
        └── platform_match_tests.rs
```

### 4.2 Cargo.toml

```toml
[package]
name = "glyim-scheduler"
version.workspace = true
edition.workspace = true
license.workspace = true
rust-version.workspace = true
description = "Build scheduler for distributed Glyim compilation"

[dependencies]
glyim-macro-vfs = { path = "../glyim-macro-vfs" }
glyim-cas-server = { path = "../glyim-cas-server" }
glyim-compiler = { path = "../glyim-compiler" }
glyim-orchestrator = { path = "../glyim-orchestrator" }
tonic = "0.12"
prost = "0.13"
tokio = { version = "1", features = ["full"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
tracing = "0.1"
uuid = { version = "1", features = ["v4"] }
dashmap = "6"
petgraph = "0.7"

[dev-dependencies]
tempfile = "3"
```

### 4.3 Scheduler Core

The scheduler is a gRPC service that accepts execution requests from orchestrators and dispatches them to available workers:

```rust
// crates/glyim-scheduler/src/scheduler.rs

use crate::queue::{ActionQueue, ActionPriority};
use crate::worker_pool::WorkerPool;
use crate::dedup::ActionDeduplicator;
use crate::metrics::SchedulerMetrics;
use glyim_cas_server::execution::types::{Action, ActionResult, Digest, Operation};
use std::sync::Arc;
use tokio::sync::RwLock;

/// The build scheduler, responsible for dispatching compilation actions to workers.
pub struct Scheduler {
    /// Priority queue of pending actions.
    queue: Arc<ActionQueue>,
    /// Pool of available workers.
    worker_pool: Arc<WorkerPool>,
    /// Deduplicator for identical actions.
    dedup: Arc<ActionDeduplicator>,
    /// Scheduler metrics for monitoring.
    metrics: Arc<SchedulerMetrics>,
    /// Scheduler configuration.
    config: SchedulerConfig,
}

#[derive(Debug, Clone)]
pub struct SchedulerConfig {
    /// Maximum number of retries for a failed action.
    pub max_retries: usize,
    /// Timeout for worker health checks.
    pub health_check_timeout: std::time::Duration,
    /// Interval between worker health checks.
    pub health_check_interval: std::time::Duration,
    /// Maximum queue depth before rejecting new actions.
    pub max_queue_depth: usize,
    /// Whether to enable speculative pre-compilation.
    pub speculative_enabled: bool,
    /// Maximum number of concurrent speculative actions.
    pub max_speculative_actions: usize,
    /// Default action timeout.
    pub default_action_timeout: std::time::Duration,
}

impl Default for SchedulerConfig {
    fn default() -> Self {
        Self {
            max_retries: 3,
            health_check_timeout: std::time::Duration::from_secs(5),
            health_check_interval: std::time::Duration::from_secs(30),
            max_queue_depth: 10_000,
            speculative_enabled: true,
            max_speculative_actions: 50,
            default_action_timeout: std::time::Duration::from_secs(300),
        }
    }
}

impl Scheduler {
    pub fn new(config: SchedulerConfig) -> Self {
        Self {
            queue: Arc::new(ActionQueue::new(config.max_queue_depth)),
            worker_pool: Arc::new(WorkerPool::new(config.health_check_interval)),
            dedup: Arc::new(ActionDeduplicator::new()),
            metrics: Arc::new(SchedulerMetrics::new()),
            config,
        }
    }

    /// Submit an action for execution. Returns an Operation that can be polled
    /// for completion or streamed for progress updates.
    pub async fn execute(
        &self,
        action: Action,
        priority: ActionPriority,
        skip_cache: bool,
    ) -> Result<Operation, SchedulerError> {
        let action_digest = compute_action_digest(&action);

        // Check deduplication: if an identical action is already in progress
        // or has been completed, return the existing operation.
        if !skip_cache {
            if let Some(existing) = self.dedup.check(&action_digest).await {
                self.metrics.record_cache_hit();
                return Ok(existing);
            }
        }

        // Check if the action result is cached in the CAS.
        if !skip_cache && !action.do_not_cache {
            if let Some(cached_result) = self.check_action_cache(&action_digest).await {
                let op = Operation::completed(action_digest, cached_result);
                self.dedup.insert(&action_digest, &op).await;
                self.metrics.record_cache_hit();
                return Ok(op);
            }
        }

        // Enqueue the action.
        let operation = Operation::pending(action_digest.clone());
        self.dedup.insert(&action_digest, &operation).await;

        self.queue.enqueue(
            action_digest,
            action,
            priority,
            self.config.default_action_timeout,
        ).await?;

        self.metrics.record_action_queued(&priority);

        Ok(operation)
    }

    /// Dispatch pending actions to available workers.
    /// Called by the scheduler's main loop.
    pub async fn dispatch(&self) -> Result<(), SchedulerError> {
        while let Some(worker) = self.worker_pool.find_available_worker(None).await {
            if let Some(action_entry) = self.queue.dequeue_for_worker(&worker).await {
                // Assign the action to the worker.
                worker.assign_action(action_entry).await?;
                self.metrics.record_action_dispatched();
            } else {
                // No actions available for this worker.
                break;
            }
        }
        Ok(())
    }

    /// Handle a worker's completion of an action.
    pub async fn complete_action(
        &self,
        worker_id: &str,
        action_digest: &Digest,
        result: ActionResult,
    ) -> Result<(), SchedulerError> {
        // Update the operation.
        let operation = Operation::completed(action_digest.clone(), result.clone());
        self.dedup.update(action_digest, &operation).await;

        // Mark the worker as available again.
        self.worker_pool.mark_available(worker_id).await?;

        // Record metrics.
        self.metrics.record_action_completed(result.exit_code == 0);

        Ok(())
    }

    /// Handle a worker failure for an in-flight action.
    pub async fn fail_action(
        &self,
        worker_id: &str,
        action_digest: &Digest,
        error: WorkerError,
    ) -> Result<(), SchedulerError> {
        // Mark the worker as potentially unhealthy.
        self.worker_pool.record_failure(worker_id, &error).await?;

        // Re-queue the action if retries remain.
        let operation = self.dedup.get(action_digest).await;
        let retry_count = operation.map(|op| op.retry_count()).unwrap_or(0);

        if retry_count < self.config.max_retries {
            self.queue.requeue(action_digest, ActionPriority::High).await?;
            self.metrics.record_action_retried();
        } else {
            let failed_op = Operation::failed(
                action_digest.clone(),
                ExecutionFailure {
                    exit_code: -1,
                    stderr: Some(format!("Worker error after {} retries: {:?}", retry_count, error)),
                    timeout_expired: false,
                    worker_error: Some(format!("{:?}", error)),
                },
            );
            self.dedup.update(action_digest, &failed_op).await;
            self.metrics.record_action_failed();
        }

        Ok(())
    }

    async fn check_action_cache(&self, digest: &Digest) -> Option<ActionResult> {
        // Query the CAS for a cached ActionResult with this digest.
        // This is delegated to the CAS server's existing action result storage.
        None // TODO: implement CAS query
    }
}
```

### 4.4 Priority Action Queue

The action queue orders pending compilation actions by priority and respects dependency constraints:

```rust
// crates/glyim-scheduler/src/queue.rs

use glyim_cas_server::execution::types::{Action, Digest};
use std::collections::HashMap;
use tokio::sync::RwLock;
use std::sync::Arc;

/// Priority levels for compilation actions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ActionPriority {
    /// Lowest priority: speculative pre-compilation, background indexing.
    Speculative = 0,
    /// Low priority: CI batch builds, scheduled builds.
    Low = 1,
    /// Medium priority: CLI builds (`glyim build`), test runs.
    Medium = 2,
    /// High priority: incremental edits, `glyim check`.
    High = 3,
    /// Critical priority: LSP diagnostics, interactive features.
    Critical = 4,
}

/// An entry in the action queue.
pub struct ActionEntry {
    /// The action digest.
    pub digest: Digest,
    /// The action to execute.
    pub action: Action,
    /// The priority of this action.
    pub priority: ActionPriority,
    /// Timeout for this action.
    pub timeout: std::time::Duration,
    /// Number of times this action has been retried.
    pub retry_count: usize,
    /// When this action was enqueued.
    pub enqueued_at: std::time::Instant,
}

/// A priority queue for compilation actions with deduplication.
pub struct ActionQueue {
    /// Internal priority queue, segmented by priority level.
    queues: Arc<RwLock<HashMap<ActionPriority, Vec<ActionEntry>>>>,
    /// Maximum queue depth across all priority levels.
    max_depth: usize,
    /// Current total queue depth.
    current_depth: Arc<std::sync::atomic::AtomicUsize>,
}

impl ActionQueue {
    pub fn new(max_depth: usize) -> Self {
        let mut queues = HashMap::new();
        for priority in [
            ActionPriority::Critical,
            ActionPriority::High,
            ActionPriority::Medium,
            ActionPriority::Low,
            ActionPriority::Speculative,
        ] {
            queues.insert(priority, Vec::new());
        }
        Self {
            queues: Arc::new(RwLock::new(queues)),
            max_depth,
            current_depth: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
        }
    }

    /// Enqueue an action at the given priority.
    pub async fn enqueue(
        &self,
        digest: Digest,
        action: Action,
        priority: ActionPriority,
        timeout: std::time::Duration,
    ) -> Result<(), QueueFullError> {
        let current = self.current_depth.load(std::sync::atomic::Ordering::Relaxed);
        if current >= self.max_depth {
            return Err(QueueFullError { max_depth: self.max_depth });
        }

        let entry = ActionEntry {
            digest,
            action,
            priority,
            timeout,
            retry_count: 0,
            enqueued_at: std::time::Instant::now(),
        };

        let mut queues = self.queues.write().await;
        queues.get_mut(&priority).unwrap().push(entry);
        self.current_depth.fetch_add(1, std::sync::atomic::Ordering::Relaxed);

        Ok(())
    }

    /// Dequeue the highest-priority action that can be assigned to a worker.
    /// Considers the worker's platform capabilities when selecting actions.
    pub async fn dequeue_for_worker(
        &self,
        worker: &crate::worker_pool::WorkerInfo,
    ) -> Option<ActionEntry> {
        let mut queues = self.queues.write().await;

        // Try priorities from highest to lowest.
        for priority in [
            ActionPriority::Critical,
            ActionPriority::High,
            ActionPriority::Medium,
            ActionPriority::Low,
            ActionPriority::Speculative,
        ] {
            let queue = queues.get_mut(&priority).unwrap();
            // Find the first action whose platform requirements match the worker.
            let matching_idx = queue.iter().position(|entry| {
                crate::platform_match::matches(&entry.action.platform, &worker.platform)
            });
            if let Some(idx) = matching_idx {
                let entry = queue.remove(idx);
                self.current_depth.fetch_sub(1, std::sync::atomic::Ordering::Relaxed);
                return Some(entry);
            }
        }

        None
    }

    /// Re-queue a failed action at elevated priority.
    pub async fn requeue(
        &self,
        digest: Digest,
        new_priority: ActionPriority,
    ) -> Result<(), QueueFullError> {
        // Find the action in any queue and move it to the new priority.
        // Implementation omitted for brevity.
        Ok(())
    }
}
```

### 4.5 Worker Pool Management

```rust
// crates/glyim-scheduler/src/worker_pool.rs

use crate::platform_match::PlatformCapabilities;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use serde::{Serialize, Deserialize};

/// Information about a registered worker.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkerInfo {
    /// Unique worker identifier.
    pub id: String,
    /// The worker's platform capabilities.
    pub platform: PlatformCapabilities,
    /// Maximum number of concurrent actions this worker can handle.
    pub max_concurrency: usize,
    /// Current number of in-flight actions.
    pub current_load: usize,
    /// Worker status.
    pub status: WorkerStatus,
    /// Last health check time.
    pub last_health_check: std::time::Instant,
    /// Number of consecutive health check failures.
    pub health_failures: usize,
    /// Total actions completed by this worker.
    pub total_actions_completed: u64,
    /// Average action duration (EWMA).
    pub avg_action_duration: std::time::Duration,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WorkerStatus {
    /// Worker is available to accept actions.
    Available,
    /// Worker is busy executing actions but may accept more (if concurrency > 1).
    Busy,
    /// Worker is temporarily unavailable (health check failed).
    Unhealthy,
    /// Worker has been removed from the pool.
    Removed,
}

/// Manages the pool of registered workers.
pub struct WorkerPool {
    workers: Arc<RwLock<HashMap<String, WorkerInfo>>>,
    health_check_interval: std::time::Duration,
    max_health_failures: usize,
}

impl WorkerPool {
    pub fn new(health_check_interval: std::time::Duration) -> Self {
        Self {
            workers: Arc::new(RwLock::new(HashMap::new())),
            health_check_interval,
            max_health_failures: 3,
        }
    }

    /// Register a new worker.
    pub async fn register(&self, info: WorkerInfo) -> Result<(), WorkerError> {
        let mut workers = self.workers.write().await;
        workers.insert(info.id.clone(), info);
        Ok(())
    }

    /// Deregister a worker (graceful shutdown).
    pub async fn deregister(&self, worker_id: &str) -> Result<Option<WorkerInfo>, WorkerError> {
        let mut workers = self.workers.write().await;
        if let Some(mut info) = workers.remove(worker_id) {
            info.status = WorkerStatus::Removed;
            Ok(Some(info))
        } else {
            Ok(None)
        }
    }

    /// Find an available worker that matches the given platform requirements.
    pub async fn find_available_worker(
        &self,
        required_platform: Option<&crate::platform_match::Platform>,
    ) -> Option<WorkerInfo> {
        let workers = self.workers.read().await;
        workers.values()
            .filter(|w| w.status == WorkerStatus::Available || w.current_load < w.max_concurrency)
            .filter(|w| {
                required_platform.map_or(true, |rp| {
                    crate::platform_match::matches(rp, &w.platform)
                })
            })
            .min_by_key(|w| w.current_load) // least-loaded worker
            .cloned()
    }

    /// Mark a worker as available after completing an action.
    pub async fn mark_available(&self, worker_id: &str) -> Result<(), WorkerError> {
        let mut workers = self.workers.write().await;
        if let Some(info) = workers.get_mut(worker_id) {
            info.current_load = info.current_load.saturating_sub(1);
            if info.current_load == 0 {
                info.status = WorkerStatus::Available;
            }
        }
        Ok(())
    }

    /// Record a failure for a worker. After too many consecutive failures,
    /// mark the worker as unhealthy.
    pub async fn record_failure(
        &self,
        worker_id: &str,
        error: &WorkerError,
    ) -> Result<(), WorkerError> {
        let mut workers = self.workers.write().await;
        if let Some(info) = workers.get_mut(worker_id) {
            info.health_failures += 1;
            if info.health_failures >= self.max_health_failures {
                info.status = WorkerStatus::Unhealthy;
                tracing::warn!(
                    worker_id = %info.id,
                    failures = info.health_failures,
                    "Worker marked as unhealthy after consecutive failures"
                );
            }
        }
        Ok(())
    }

    /// Get the total available capacity across all healthy workers.
    pub async fn total_capacity(&self) -> usize {
        let workers = self.workers.read().await;
        workers.values()
            .filter(|w| w.status != WorkerStatus::Unhealthy && w.status != WorkerStatus::Removed)
            .map(|w| w.max_concurrency.saturating_sub(w.current_load))
            .sum()
    }
}

#[derive(Debug, Clone)]
pub enum WorkerError {
    WorkerNotFound(String),
    ActionAssignmentFailed(String),
    HealthCheckFailed(String),
    ConnectionLost(String),
}
```

### 4.6 Speculative Pre-Compilation

The speculative pre-compilation engine predicts which source files a developer is likely to edit next, based on the reference graph from Phase 7, and pre-compiles them on idle workers:

```rust
// crates/glyim-scheduler/src/speculative.rs

use glyim_cas_server::execution::types::{Action, Digest, Platform};
use crate::queue::ActionPriority;
use std::collections::{HashMap, HashSet};

/// Speculative pre-compilation engine.
pub struct SpeculativeEngine {
    /// The reference graph (symbol → set of files that reference it).
    reference_graph: HashMap<String, HashSet<String>>,
    /// Recently edited files (LRU, most recent first).
    recently_edited: Vec<String>,
    /// Files that have been speculatively compiled recently.
    recently_speculated: HashSet<String>,
    /// Maximum number of speculative actions to run concurrently.
    max_concurrent: usize,
    /// Current number of speculative actions running.
    current_speculative: usize,
}

impl SpeculativeEngine {
    pub fn new(
        reference_graph: HashMap<String, HashSet<String>>,
        max_concurrent: usize,
    ) -> Self {
        Self {
            reference_graph,
            recently_edited: Vec::new(),
            recently_speculated: HashSet::new(),
            max_concurrent,
            current_speculative: 0,
        }
    }

    /// Record a file edit event. Triggers speculative pre-compilation
    /// of files that share symbols with the edited file.
    pub fn on_file_edited(&mut self, file_path: &str) -> Vec<SpeculativeAction> {
        // Add to recently edited
        self.recently_edited.retain(|f| f != file_path);
        self.recently_edited.push(file_path.to_string());

        // Find files that reference symbols defined in the edited file
        let mut candidates = HashSet::new();

        // Heuristic 1: Files that import from the edited file
        if let Some(referencers) = self.reference_graph.get(file_path) {
            for ref_file in referencers {
                if !self.recently_speculated.contains(ref_file) {
                    candidates.insert(ref_file.clone());
                }
            }
        }

        // Heuristic 2: Files edited in the same "editing session"
        // (files edited within the last N minutes that share references)
        for recent_file in &self.recently_edited {
            if recent_file != file_path {
                if let Some(referencers) = self.reference_graph.get(recent_file) {
                    for ref_file in referencers {
                        if !self.recently_speculated.contains(ref_file) && ref_file != file_path {
                            candidates.insert(ref_file.clone());
                        }
                    }
                }
            }
        }

        // Generate speculative actions for the top candidates
        let mut actions = Vec::new();
        for candidate in candidates.iter().take(self.max_concurrent - self.current_speculative) {
            self.recently_speculated.insert(candidate.clone());
            actions.push(SpeculativeAction {
                file_path: candidate.clone(),
                trigger: file_path.to_string(),
                reason: SpeculativeReason::SharedReferences,
            });
        }

        actions
    }

    /// Called when a speculative action completes.
    pub fn on_speculative_complete(&mut self, file_path: &str) {
        self.current_speculative = self.current_speculative.saturating_sub(1);
        // Keep in recently_speculated for a while to avoid re-speculation
    }

    /// Prune the recently_speculated set periodically.
    pub fn prune(&mut self) {
        // Remove entries older than a configurable TTL
        self.recently_speculated.clear();
    }
}

#[derive(Debug, Clone)]
pub struct SpeculativeAction {
    pub file_path: String,
    pub trigger: String,
    pub reason: SpeculativeReason,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpeculativeReason {
    /// The file references symbols defined in the trigger file.
    SharedReferences,
    /// The file was edited recently in the same session.
    CoEditingPattern,
    /// The file is a known "hot path" in the project (frequently changed).
    HotPath,
}
```

---

## 5. Remote Worker Daemon

### 5.1 New Binary: `glyim-worker`

The remote worker is a standalone daemon that connects to the scheduler, requests work, executes compilation actions, and reports results. It is started with `glyim worker --scheduler <URL>`:

```rust
// crates/glyim-cli/src/commands/worker.rs

use glyim_compiler::pipeline::QueryPipeline;
use glyim_cas_server::execution::types::*;
use std::path::PathBuf;
use std::sync::Arc;

/// Configuration for the remote worker daemon.
pub struct WorkerConfig {
    /// URL of the scheduler service.
    pub scheduler_url: String,
    /// URL of the CAS server for input/output transfer.
    pub cas_url: String,
    /// Authentication token for the scheduler.
    pub auth_token: Option<String>,
    /// Maximum number of concurrent actions.
    pub max_concurrency: usize,
    /// Working directory for action execution.
    pub work_dir: PathBuf,
    /// Target triple this worker supports.
    pub target_triple: String,
    /// Available RAM in GB.
    pub available_ram_gb: usize,
    /// Number of CPU cores.
    pub cpu_cores: usize,
    /// Whether to accept speculative (low-priority) actions.
    pub accept_speculative: bool,
}

/// The remote worker daemon.
pub struct RemoteWorker {
    config: WorkerConfig,
    pipeline: QueryPipeline,
    worker_id: String,
}

impl RemoteWorker {
    pub fn new(config: WorkerConfig) -> Self {
        let worker_id = uuid::Uuid::new_v4().to_string();
        let pipeline = QueryPipeline::new(
            &config.work_dir,
            glyim_compiler::pipeline::PipelineConfig::default(),
        );
        Self {
            config,
            pipeline,
            worker_id,
        }
    }

    /// Run the worker daemon main loop.
    pub async fn run(&self) -> Result<(), WorkerDaemonError> {
        // 1. Register with the scheduler
        self.register_with_scheduler().await?;

        // 2. Enter the work loop
        loop {
            // Request an action from the scheduler
            let action = self.request_action().await?;

            match action {
                Some(action_entry) => {
                    // Execute the action in a sandboxed environment
                    let result = self.execute_action(&action_entry).await?;

                    // Report the result to the scheduler
                    self.report_result(&action_entry.digest, result).await?;
                }
                None => {
                    // No actions available; wait before polling again
                    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                }
            }

            // Periodic health check
            self.send_heartbeat().await?;
        }
    }

    /// Execute a compilation action locally.
    async fn execute_action(&self, entry: &ActionEntry) -> Result<ActionResult, WorkerDaemonError> {
        // 1. Fetch inputs from the CAS
        let inputs = self.fetch_inputs(&entry.action).await?;

        // 2. Set up sandboxed execution environment
        let sandbox_dir = self.config.work_dir.join(format!("action-{}", entry.digest.hash));
        std::fs::create_dir_all(&sandbox_dir)?;

        // 3. Write input files to sandbox
        for (path, content) in &inputs.files {
            std::fs::write(sandbox_dir.join(path), content)?;
        }

        // 4. Execute the compilation action
        let start = std::time::Instant::now();
        let result = match self.determine_action_type(&entry.action) {
            ActionType::ParseAndLower => {
                self.execute_parse_and_lower(&sandbox_dir, &entry.action).await
            }
            ActionType::CodegenFunction => {
                self.execute_codegen_function(&sandbox_dir, &entry.action).await
            }
            ActionType::CodegenPackage => {
                self.execute_codegen_package(&sandbox_dir, &entry.action).await
            }
            ActionType::EgraphOptimize => {
                self.execute_egraph_optimize(&sandbox_dir, &entry.action).await
            }
            ActionType::LinkBinary => {
                self.execute_link_binary(&sandbox_dir, &entry.action).await
            }
        };
        let duration = start.elapsed();

        // 5. Upload outputs to the CAS
        match result {
            Ok(output_files) => {
                let output_digests = self.upload_outputs(&output_files).await?;
                Ok(ActionResult {
                    output_files: output_digests,
                    exit_code: 0,
                    stdout_digest: None, // TODO: stream stdout
                    stderr_digest: None,
                    execution_metadata: ExecutionMetadata {
                        worker_id: self.worker_id.clone(),
                        execution_duration: duration,
                        queued_duration: std::time::Duration::ZERO,
                        input_fetch_duration: std::time::Duration::ZERO,
                        output_upload_duration: std::time::Duration::ZERO,
                    },
                })
            }
            Err(e) => {
                Ok(ActionResult {
                    output_files: vec![],
                    exit_code: 1,
                    stdout_digest: None,
                    stderr_digest: None,
                    execution_metadata: ExecutionMetadata {
                        worker_id: self.worker_id.clone(),
                        execution_duration: duration,
                        queued_duration: std::time::Duration::ZERO,
                        input_fetch_duration: std::time::Duration::ZERO,
                        output_upload_duration: std::time::Duration::ZERO,
                    },
                })
            }
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum ActionType {
    ParseAndLower,
    CodegenFunction,
    CodegenPackage,
    EgraphOptimize,
    LinkBinary,
}

#[derive(Debug)]
pub enum WorkerDaemonError {
    SchedulerConnectionFailed(String),
    CasConnectionFailed(String),
    ActionExecutionFailed(String),
    InputFetchFailed(String),
    OutputUploadFailed(String),
    IoError(std::io::Error),
}
```

### 5.2 Worker Sandbox

The worker executes each action in an isolated sandbox directory to prevent actions from interfering with each other. The sandbox is created per-action and cleaned up after the action completes:

```
work_dir/
├── action-<hash1>/
│   ├── inputs/
│   │   ├── main.g           — source file
│   │   ├── config.bin        — serialized PipelineConfig
│   │   └── prelude.g         — standard prelude
│   └── outputs/
│       ├── compiled_hir.bin  — serialized CompiledHir
│       └── object.o          — compiled object code
├── action-<hash2>/
│   ├── inputs/
│   └── outputs/
└── worker-state/
    └── heartbeat.json        — last heartbeat timestamp
```

### 5.3 Platform Capabilities Advertisement

Workers advertise their platform capabilities to the scheduler during registration:

```rust
// crates/glyim-scheduler/src/platform_match.rs

use serde::{Serialize, Deserialize};
use glyim_cas_server::execution::types::Platform;

/// The platform capabilities of a worker.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlatformCapabilities {
    /// Supported target triples (e.g., ["x86_64-unknown-linux-gnu"]).
    pub target_triples: Vec<String>,
    /// Number of CPU cores.
    pub cpu_cores: usize,
    /// Available RAM in GB.
    pub ram_gb: usize,
    /// Available disk space in GB.
    pub disk_gb: usize,
    /// Whether the worker supports GPU acceleration (for future e-graph GPU acceleration).
    pub gpu_available: bool,
    /// Compiler version.
    pub compiler_version: String,
    /// Operating system.
    pub os: String,
    /// Architecture.
    pub arch: String,
}

/// Check if a worker's capabilities match the action's platform requirements.
pub fn matches(
    required: &Platform,
    available: &PlatformCapabilities,
) -> bool {
    for property in &required.properties {
        match property.name.as_str() {
            "target_triple" => {
                if !available.target_triples.contains(&property.value) {
                    return false;
                }
            }
            "min_ram_gb" => {
                if let Ok(min_ram) = property.value.parse::<usize>() {
                    if available.ram_gb < min_ram {
                        return false;
                    }
                }
            }
            "min_cpu_cores" => {
                if let Ok(min_cores) = property.value.parse::<usize>() {
                    if available.cpu_cores < min_cores {
                        return false;
                    }
                }
            }
            "os" => {
                if available.os != property.value {
                    return false;
                }
            }
            "arch" => {
                if available.arch != property.value {
                    return false;
                }
            }
            "compiler_version" => {
                if available.compiler_version != property.value {
                    return false;
                }
            }
            _ => {} // Unknown properties are ignored
        }
    }
    true
}
```

---

## 6. CAS Server Enhancements

### 6.1 Bazel REv2 Execution API

The `glyim-cas-server` crate is extended with the Bazel REv2 `Execution` API:

```rust
// crates/glyim-cas-server/src/execution/grpc.rs

use tonic::{Request, Response, Status};
use prost::Message;

/// The Bazel REv2 Execution gRPC service implementation.
pub struct ExecutionService {
    scheduler: Arc<glyim_scheduler::Scheduler>,
    cas_store: Arc<dyn ContentStore>,
}

#[tonic::async_trait]
impl Execution for ExecutionService {
    /// Execute an action remotely.
    async fn execute(
        &self,
        request: Request<ExecuteRequest>,
    ) -> Result<Response<Self::ExecuteStream>, Status> {
        let req = request.into_inner();

        // 1. Resolve the Action digest to get the Action message
        let action_bytes = self.cas_store.retrieve(&req.action_digest.hash.into())
            .await
            .ok_or_else(|| Status::not_found("Action digest not found in CAS"))?;
        let action: Action = bincode::deserialize(&action_bytes)
            .map_err(|e| Status::invalid_argument(format!("Invalid Action: {}", e)))?;

        // 2. Determine priority from the request's priority field
        let priority = match req.priority {
            0 => ActionPriority::Speculative,
            1 => ActionPriority::Low,
            2 => ActionPriority::Medium,
            3 => ActionPriority::High,
            4 => ActionPriority::Critical,
            _ => ActionPriority::Medium,
        };

        // 3. Submit the action to the scheduler
        let operation = self.scheduler.execute(action, priority, req.skip_cache_lookup)
            .await
            .map_err(|e| Status::internal(format!("Scheduler error: {:?}", e)))?;

        // 4. Return a streaming response
        let (tx, rx) = tokio::sync::mpsc::channel(16);

        // Spawn a task that polls the operation and streams updates
        let scheduler = self.scheduler.clone();
        tokio::spawn(async move {
            loop {
                let op = scheduler.get_operation(&operation.name).await;
                if let Some(op) = op {
                    let _ = tx.send(op.clone().into()).await;
                    if op.done {
                        break;
                    }
                }
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            }
        });

        Ok(Response::new(ReceiverStream::new(rx)))
    }

    /// Wait for an execution operation to complete.
    async fn wait_execution(
        &self,
        request: Request<WaitExecutionRequest>,
    ) -> Result<Response<Self::WaitExecutionStream>, Status> {
        // Similar to execute but for an already-submitted operation
        todo!()
    }
}
```

### 6.2 Authentication and Authorization

```rust
// crates/glyim-cas-server/src/auth.rs

use serde::{Serialize, Deserialize};

/// Authentication configuration for the CAS server.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthConfig {
    /// Authentication mode.
    pub mode: AuthMode,
    /// HMAC key for API key validation.
    pub hmac_key: Option<String>,
    /// OAuth 2.0 JWKS URL for token validation.
    pub jwks_url: Option<String>,
    /// Token issuer (for validation).
    pub issuer: Option<String>,
    /// Token audience (for validation).
    pub audience: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AuthMode {
    /// No authentication (current behavior).
    None,
    /// API key authentication (simple bearer token).
    ApiKey,
    /// OAuth 2.0 JWT bearer token authentication.
    OAuth2,
}

/// A verified identity extracted from an authentication token.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Identity {
    /// The principal (user or service account) identifier.
    pub principal: String,
    /// The namespace(s) this identity belongs to.
    pub namespaces: Vec<String>,
    /// The roles assigned to this identity.
    pub roles: Vec<Role>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Role {
    /// Can read artifacts from the CAS.
    Reader,
    /// Can write artifacts to the CAS.
    Writer,
    /// Can submit actions for remote execution.
    Executor,
    /// Can manage workers and scheduler configuration.
    Admin,
}

/// Authorization check for CAS operations.
pub fn authorize(identity: &Identity, action: AuthAction, namespace: &str) -> bool {
    let in_namespace = identity.namespaces.contains(&namespace.to_string())
        || identity.namespaces.contains(&"*".to_string());

    match action {
        AuthAction::ReadArtifact => {
            in_namespace && identity.roles.contains(&Role::Reader)
        }
        AuthAction::WriteArtifact => {
            in_namespace && identity.roles.contains(&Role::Writer)
        }
        AuthAction::SubmitAction => {
            identity.roles.contains(&Role::Executor)
        }
        AuthAction::ManageWorkers => {
            identity.roles.contains(&Role::Admin)
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum AuthAction {
    ReadArtifact,
    WriteArtifact,
    SubmitAction,
    ManageWorkers,
}
```

### 6.3 Namespace Isolation

Artifacts in the CAS are now namespaced. The namespace is a prefix on all blob names and action results:

```
CAS storage layout (filesystem):
  <cas_root>/
  ├── namespaces/
  │   ├── team-alpha/
  │   │   ├── blobs/
  │   │   │   └── sha256/
  │   │   │       ├── ab/
  │   │   │       │   └── ab12cd34...
  │   │   │       └── ...
  │   │   └── actions/
  │   │       └── sha256/
  │   │           └── ...
  │   ├── team-beta/
  │   │   └── ...
  │   └── public/
  │       └── ...
  └── gc/
      └── last_run.json
```

Namespaces are transparent to the Bazel REv2 protocol: the namespace is extracted from the authentication token and used to scope all blob and action operations. Unauthenticated requests (when `AuthMode::None`) use the `public` namespace.

### 6.4 Garbage Collection Daemon

```rust
// crates/glyim-cas-server/src/gc.rs

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;
use serde::{Serialize, Deserialize};

/// Configuration for the garbage collection daemon.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GcConfig {
    /// Whether GC is enabled.
    pub enabled: bool,
    /// Interval between GC runs.
    pub interval: std::time::Duration,
    /// Retention policy.
    pub retention: RetentionPolicy,
    /// Minimum free disk space to maintain (in GB). GC runs when free space drops below this.
    pub min_free_gb: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RetentionPolicy {
    /// Keep the last N versions of each artifact.
    LastNVersions { n: usize },
    /// Keep artifacts for D days after last access.
    TimeBased { days: usize },
    /// Keep artifacts referenced by any lockfile in the workspace.
    ReferenceCounted,
    /// Combined policy: reference-counted primary, time-based fallback.
    Combined {
        /// Always keep referenced artifacts.
        keep_referenced: bool,
        /// Additionally keep unreferenced artifacts for this many days.
        unreferenced_grace_days: usize,
    },
}

impl Default for GcConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            interval: std::time::Duration::from_secs(3600), // 1 hour
            retention: RetentionPolicy::Combined {
                keep_referenced: true,
                unreferenced_grace_days: 7,
            },
            min_free_gb: 10,
        }
    }
}

/// The garbage collection daemon.
pub struct GcDaemon {
    config: GcConfig,
    cas_root: PathBuf,
}

impl GcDaemon {
    pub fn new(config: GcConfig, cas_root: PathBuf) -> Self {
        Self { config, cas_root }
    }

    /// Run a single GC pass.
    pub async fn run_gc_pass(&self) -> GcResult {
        let start = std::time::Instant::now();

        // 1. Scan all blobs in the CAS
        let all_blobs = self.scan_blobs().await;

        // 2. Find referenced blobs (referenced by any action result or name)
        let referenced_blobs = self.find_referenced_blobs().await;

        // 3. Compute the set of unreferenced blobs
        let unreferenced: HashSet<_> = all_blobs.difference(&referenced_blobs).cloned().collect();

        // 4. Apply retention policy
        let to_delete = self.apply_retention_policy(&unreferenced).await;

        // 5. Delete the blobs
        let mut deleted = 0;
        let mut freed_bytes = 0u64;
        for hash in &to_delete {
            if let Ok(size) = self.delete_blob(hash).await {
                deleted += 1;
                freed_bytes += size;
            }
        }

        GcResult {
            total_blobs: all_blobs.len(),
            referenced_blobs: referenced_blobs.len(),
            unreferenced_blobs: unreferenced.len(),
            deleted_blobs: deleted,
            freed_bytes,
            elapsed: start.elapsed(),
        }
    }

    /// Run the GC daemon in a background loop.
    pub async fn run(self: Arc<Self>) {
        let mut interval = tokio::time::interval(self.config.interval);
        loop {
            interval.tick().await;
            let result = self.run_gc_pass().await;
            tracing::info!(
                deleted = result.deleted_blobs,
                freed_mb = result.freed_bytes / 1_048_576,
                elapsed_ms = result.elapsed.as_millis(),
                "GC pass completed"
            );
        }
    }

    async fn scan_blobs(&self) -> HashSet<ContentHash> { /* ... */ }
    async fn find_referenced_blobs(&self) -> HashSet<ContentHash> { /* ... */ }
    async fn apply_retention_policy(&self, unreferenced: &HashSet<ContentHash>) -> Vec<ContentHash> { /* ... */ }
    async fn delete_blob(&self, hash: &ContentHash) -> Result<u64, std::io::Error> { /* ... */ }
}

#[derive(Debug, Clone)]
pub struct GcResult {
    pub total_blobs: usize,
    pub referenced_blobs: usize,
    pub unreferenced_blobs: usize,
    pub deleted_blobs: usize,
    pub freed_bytes: u64,
    pub elapsed: std::time::Duration,
}
```

---

## 7. Orchestrator Integration

### 7.1 Remote Execution Decision

The orchestrator must decide whether to compile locally or send actions for remote execution. The decision is based on:

1. **Action size**: Small actions (parse + lower for a single file) are faster to execute locally than the round-trip overhead of remote execution. Large actions (full-package codegen, e-graph optimization) benefit from remote execution on powerful workers.

2. **Worker availability**: If no workers are available, the orchestrator compiles locally to avoid blocking.

3. **Target platform**: If the target platform differs from the local platform, the action must be sent to a worker with the matching target.

4. **Queue depth**: If the scheduler queue is too deep, the orchestrator compiles locally to avoid excessive latency.

```rust
// crates/glyim-orchestrator/src/execution_strategy.rs

use glyim_compiler::pipeline::PipelineConfig;
use glyim_cas_server::execution::types::Action;

/// Strategy for deciding between local and remote execution.
pub enum ExecutionStrategy {
    /// Always compile locally (no remote execution).
    LocalOnly,
    /// Prefer remote execution; fall back to local if no workers available.
    RemotePreferred {
        scheduler_url: String,
        auth_token: Option<String>,
    },
    /// Use remote execution only for actions that exceed a local threshold.
    Hybrid {
        scheduler_url: String,
        auth_token: Option<String>,
        /// Maximum estimated local execution time before sending to remote.
        local_timeout: std::time::Duration,
    },
    /// Always compile remotely (for CI/CD environments).
    RemoteOnly {
        scheduler_url: String,
        auth_token: Option<String>,
    },
}

impl ExecutionStrategy {
    /// Decide whether to execute an action locally or remotely.
    pub fn should_execute_remotely(
        &self,
        action: &Action,
        estimated_duration: std::time::Duration,
        worker_available: bool,
    ) -> bool {
        match self {
            ExecutionStrategy::LocalOnly => false,
            ExecutionStrategy::RemotePreferred { .. } => worker_available,
            ExecutionStrategy::Hybrid { local_timeout, .. } => {
                estimated_duration > *local_timeout && worker_available
            }
            ExecutionStrategy::RemoteOnly { .. } => true,
        }
    }
}
```

### 7.2 Package-Level Parallelism

The orchestrator now compiles packages in parallel within topological layers. Packages at the same depth in the dependency graph have no dependencies on each other and can be compiled concurrently:

```rust
// crates/glyim-orchestrator/src/parallel.rs

use crate::graph::PackageGraph;
use crate::orchestrator::PackageNode;
use std::collections::HashMap;

/// Compute topological layers of the package graph.
/// Packages in the same layer can be compiled in parallel.
pub fn compute_layers(graph: &PackageGraph) -> Vec<Vec<String>> {
    let mut layers = Vec::new();
    let mut remaining: HashMap<String, usize> = graph.all_packages()
        .map(|name| {
            let in_degree = graph.dependency_count(name);
            (name.clone(), in_degree)
        })
        .collect();

    while !remaining.is_empty() {
        // Find all packages with zero in-degree (no unresolved dependencies)
        let layer: Vec<String> = remaining.iter()
            .filter(|(_, &in_degree)| in_degree == 0)
            .map(|(name, _)| name.clone())
            .collect();

        if layer.is_empty() {
            // This shouldn't happen if the graph is acyclic
            panic!("Circular dependency detected in package graph");
        }

        // Remove the layer's packages and update in-degrees
        for pkg_name in &layer {
            remaining.remove(pkg_name);
            for dependent in graph.direct_dependents(pkg_name) {
                if let Some(in_degree) = remaining.get_mut(dependent) {
                    *in_degree -= 1;
                }
            }
        }

        layers.push(layer);
    }

    layers
}
```

### 7.3 Orchestrator Build Flow with Remote Execution

```
1. Discover workspace / detect single package
2. Load manifests for all packages
3. Resolve dependencies → produce Lockfile
4. Compute topological layers
5. For each layer (in parallel where possible):
   a. For each package in the layer:
      i.   Compute source hash
      ii.  Check CAS for cached artifact
      iii. If cached: download and skip
      iv.  If not cached:
           - Decide: local or remote execution?
           - If local: compile using Phase 4 query pipeline
           - If remote: submit actions to scheduler
      v.   Store artifacts in CAS
6. Resolve cross-module symbols
7. Link all object code
8. Report diagnostics
```

---

## 8. CLI Integration

### 8.1 New Commands

| Command | Description | New Flags |
|---------|-------------|-----------|
| `glyim worker` | Start a remote worker daemon | `--scheduler <URL>`, `--cas <URL>`, `--max-concurrency <N>`, `--target <triple>`, `--token <key>` |
| `glyim scheduler` | Start the build scheduler service | `--port <PORT>`, `--cas <URL>`, `--token <key>`, `--speculative`, `--max-queue-depth <N>` |
| `glyim gc` | Run garbage collection on the CAS | `--dry-run`, `--namespace <ns>`, `--retention <policy>` |

### 8.2 Modified Commands

| Command | Changes | New Flags |
|---------|---------|-----------|
| `glyim build` | Supports remote execution and package-level parallelism | `--remote-execution <URL>`, `--execution-strategy <local\|hybrid\|remote>`, `--max-parallel <N>` |
| `glyim check` | Supports remote type checking | `--remote-execution <URL>` |
| `glyim test` | Distributes test execution across workers | `--remote-execution <URL>`, `--test-parallelism <N>` |
| `glyim cas` | New subcommand group for CAS management | `glyim cas gc`, `glyim cas stats`, `glyim cas verify` |

### 8.3 `glyim worker` Command

```rust
// crates/glyim-cli/src/commands/worker.rs

/// Start a remote worker daemon.
pub fn cmd_worker(
    scheduler_url: String,
    cas_url: String,
    max_concurrency: usize,
    target: Option<String>,
    token: Option<String>,
    work_dir: Option<PathBuf>,
) -> i32 {
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        let config = WorkerConfig {
            scheduler_url,
            cas_url,
            auth_token: token,
            max_concurrency,
            work_dir: work_dir.unwrap_or_else(|| std::env::temp_dir().join("glyim-worker")),
            target_triple: target.unwrap_or_else(|| std::env::consts::ARCH.to_string()),
            available_ram_gb: 8, // TODO: detect
            cpu_cores: num_cpus::get(),
            accept_speculative: true,
        };

        let worker = RemoteWorker::new(config);
        match worker.run().await {
            Ok(()) => 0,
            Err(e) => {
                eprintln!("Worker error: {:?}", e);
                1
            }
        }
    })
}
```

### 8.4 `glyim scheduler` Command

```rust
// crates/glyim-cli/src/commands/scheduler_cmd.rs

/// Start the build scheduler service.
pub fn cmd_scheduler(
    port: u16,
    cas_url: String,
    token: Option<String>,
    speculative: bool,
    max_queue_depth: usize,
) -> i32 {
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        let config = SchedulerConfig {
            max_retries: 3,
            health_check_timeout: std::time::Duration::from_secs(5),
            health_check_interval: std::time::Duration::from_secs(30),
            max_queue_depth,
            speculative_enabled: speculative,
            max_speculative_actions: 50,
            default_action_timeout: std::time::Duration::from_secs(300),
        };

        let scheduler = Arc::new(Scheduler::new(config));

        // Start the gRPC scheduler service
        let addr = format!("0.0.0.0:{}", port).parse().unwrap();
        let service = SchedulerService::new(scheduler.clone());

        tracing::info!("Scheduler listening on {}", addr);

        // Start background dispatch loop
        let scheduler_clone = scheduler.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_millis(100));
            loop {
                interval.tick().await;
                let _ = scheduler_clone.dispatch().await;
            }
        });

        // Start the gRPC server
        Server::builder()
            .add_service(SchedulerServer::new(service))
            .serve(addr)
            .await
            .unwrap();

        0
    })
}
```

---

## 9. Testing Strategy

### 9.1 Unit Tests

| Test | Location | Description |
|------|----------|-------------|
| `action_serialization_roundtrip` | `glyim-cas-server/tests/` | Action → serialize → deserialize → compare |
| `directory_tree_serialization` | `glyim-cas-server/tests/` | Input root directory tree roundtrips correctly |
| `priority_queue_ordering` | `glyim-scheduler/tests/` | Higher-priority actions are dequeued first |
| `priority_queue_platform_filter` | `glyim-scheduler/tests/` | Only matching-platform actions are dequeued for a worker |
| `dedup_cache_hit` | `glyim-scheduler/tests/` | Duplicate action returns cached result |
| `dedup_no_cache` | `glyim-scheduler/tests/` | `skip_cache` bypasses dedup cache |
| `platform_match_target` | `glyim-scheduler/tests/` | Target triple matching works correctly |
| `platform_match_ram` | `glyim-scheduler/tests/` | RAM requirement filtering works |
| `speculative_prediction` | `glyim-scheduler/tests/` | Speculative engine predicts correct candidates |
| `execution_strategy_hybrid` | `glyim-orchestrator/tests/` | Hybrid strategy sends large actions remotely |
| `execution_strategy_local_only` | `glyim-orchestrator/tests/` | Local-only strategy never sends actions remotely |
| `topological_layers_parallel` | `glyim-orchestrator/tests/` | Independent packages are in the same layer |
| `auth_authorization_reader` | `glyim-cas-server/tests/` | Reader role can read but not write |
| `auth_authorization_writer` | `glyim-cas-server/tests/` | Writer role can read and write |
| `auth_namespace_isolation` | `glyim-cas-server/tests/` | Namespace-scoped reads are isolated |
| `gc_retention_referenced` | `glyim-cas-server/tests/` | Referenced artifacts are not collected |
| `gc_retention_unreferenced_expired` | `glyim-cas-server/tests/` | Expired unreferenced artifacts are collected |
| `gc_retention_unreferenced_grace` | `glyim-cas-server/tests/` | Unreferenced artifacts within grace period are kept |

### 9.2 Integration Tests

| Test | Location | Description |
|------|----------|-------------|
| `worker_registration_lifecycle` | `glyim-scheduler/tests/` | Worker registers, receives actions, deregisters |
| `worker_health_check` | `glyim-scheduler/tests/` | Unhealthy workers are removed from pool |
| `action_retry_on_failure` | `glyim-scheduler/tests/` | Failed actions are retried on different workers |
| `action_timeout` | `glyim-scheduler/tests/` | Timed-out actions are killed and retried |
| `scheduler_dedup_identical_actions` | `glyim-scheduler/tests/` | Two identical actions produce one execution |
| `speculative_precompilation_hit` | `glyim-orchestrator/tests/` | Speculatively compiled artifact is cached when needed |
| `cross_compilation_dispatch` | `glyim-orchestrator/tests/` | ARM-targeted action is dispatched to ARM worker |
| `parallel_package_compilation` | `glyim-orchestrator/tests/` | Independent packages compile in parallel |
| `hybrid_execution_small_local` | `glyim-orchestrator/tests/` | Small actions execute locally |
| `hybrid_execution_large_remote` | `glyim-orchestrator/tests/` | Large actions execute remotely |
| `cas_auth_read_write` | `glyim-cas-server/tests/` | Authenticated read/write with valid token |
| `cas_auth_reject_invalid` | `glyim-cas-server/tests/` | Invalid token is rejected |
| `gc_frees_disk_space` | `glyim-cas-server/tests/` | GC pass deletes unreferenced blobs |

### 9.3 End-to-End Tests

| Test | Description |
|------|-------------|
| `single_worker_distributed_build` | Build a workspace with one worker; verify all artifacts in CAS |
| `multi_worker_distributed_build` | Build a workspace with three workers (different platforms); verify cross-compilation |
| `worker_failure_recovery` | Kill a worker mid-build; verify the scheduler retries on another worker |
| `scheduler_restart_recovery` | Restart the scheduler; verify workers reconnect and pending actions resume |
| `speculative_precompilation_e2e` | Edit a file, verify speculative compilation occurs, verify cached result used |
| `hybrid_build_ci` | CI-like build with remote execution; verify all actions execute remotely |
| `team_isolation` | Two teams share a CAS server; verify namespace isolation prevents cross-team access |
| `gc_integration` | Build a project, push artifacts, run GC, verify referenced artifacts survive |
| `full_pipeline_distributed` | Full incremental pipeline: edit → local parse/lower → remote codegen → local link |

---

## 10. Implementation Timeline

### Week 1–2: Remote Execution Protocol and CAS Enhancements

| Day | Task |
|-----|------|
| 1–2 | Define Action/Command/Directory/Operation types in `glyim-cas-server` |
| 3–4 | Implement Bazel REv2 `Execution` gRPC service in `glyim-cas-server` |
| 5–6 | Implement authentication middleware (API key + OAuth2) in `glyim-cas-server` |
| 7–8 | Implement namespace isolation in `LocalContentStore` |
| 9–10 | Implement garbage collection daemon |

### Week 3–4: Scheduler and Worker

| Day | Task |
|-----|------|
| 11–13 | Create `glyim-scheduler` crate with priority queue and deduplication |
| 14–16 | Implement worker pool management and platform matching |
| 17–19 | Implement speculative pre-compilation engine |
| 20–21 | Implement `glyim-worker` daemon with sandboxed action execution |
| 22–23 | Implement worker health monitoring and failure recovery |
| 24 | Implement streaming execution logs |

### Week 5–6: Orchestrator Integration

| Day | Task |
|-----|------|
| 25–26 | Implement `ExecutionStrategy` decision logic in `glyim-orchestrator` |
| 27–28 | Implement package-level parallel compilation with topological layers |
| 29–30 | Implement CAS artifact management for remote execution (input packing, output unpacking) |
| 31–32 | Implement `--remote-execution` and `--execution-strategy` CLI flags |
| 33 | Implement `glyim worker` and `glyim scheduler` CLI commands |

### Week 7–8: Testing, Metrics, and Polish

| Day | Task |
|-----|------|
| 34–36 | Write integration tests for distributed compilation with simulated worker pools |
| 37–38 | Add distributed build metrics to `glyim-bench` (scheduler latency, worker utilization, cache effectiveness) |
| 39–40 | End-to-end testing with multi-worker setup |
| 41 | Documentation: architecture docs, operational guide for worker/scheduler deployment |
| 42 | Performance tuning and final integration testing |

---

## 11. Crate Dependency Changes

### 11.1 New Crates

| Crate | Description | Dependencies |
|-------|-------------|--------------|
| `glyim-scheduler` | Build scheduler with priority queue, dedup, worker pool, speculative pre-compilation | `glyim-macro-vfs`, `glyim-cas-server`, `glyim-compiler`, `glyim-orchestrator`, `tonic`, `prost`, `tokio`, `dashmap`, `petgraph` |

### 11.2 Modified Crates

| Crate | Changes |
|-------|---------|
| `glyim-cas-server` | Add Bazel REv2 `Execution` gRPC service; add authentication middleware; add namespace isolation; add garbage collection daemon; add streaming execution logs |
| `glyim-orchestrator` | Add `ExecutionStrategy` for local/remote decision; add package-level parallelism with topological layers; add remote action submission and result retrieval |
| `glyim-cli` | Add `glyim worker` command; add `glyim scheduler` command; add `--remote-execution` flag to `build`, `check`, `test`; add `glyim gc` command |
| `glyim-bench` | Add distributed build metrics (scheduler latency, worker utilization, action deduplication rate, cache hit rate) |
| `glyim-macro-vfs` | Extend `ContentStore` trait with namespace-aware methods |
| `Cargo.toml` (workspace) | Add `glyim-scheduler` to workspace members |

---

## 12. Risk Register

| Risk | Probability | Impact | Mitigation |
|------|------------|--------|------------|
| Network latency dominates small actions | High | Medium | Hybrid execution strategy: actions under `local_timeout` execute locally. Default threshold: 200ms. Only codegen and e-graph actions are sent remotely by default. |
| Worker unavailability during build | Medium | High | Orchestrator falls back to local compilation if no workers available within 5 seconds. Build never blocks waiting for remote workers. |
| CAS blob corruption on transfer | Low | High | Content hash verification on every blob upload and download. Corrupted blobs are discarded and re-uploaded from the source. |
| Scheduler becomes a single point of failure | Medium | Critical | Scheduler is stateless (all state is in the CAS). If the scheduler crashes, a new instance can be started and workers will reconnect. In-flight actions may be lost and retried. |
| Speculative pre-compilation wastes resources | Medium | Low | Speculative actions have lowest priority and are cancelled when higher-priority actions arrive. Workers cap speculative concurrency. |
| Namespace isolation breach | Low | Critical | All CAS operations go through the authorization layer. Integration tests verify isolation. Security audit before release. |
| GC deletes recently referenced artifacts | Low | High | Reference counting is conservative: an artifact is only deleted if it has zero references AND has passed the grace period. GC logs all deletions for audit. |
| Cross-compilation worker mismatch | Medium | Medium | Platform matching is strict: if no worker matches the target triple, the action fails immediately with a clear error. Fallback to local cross-compilation is available. |
| gRPC streaming backpressure | Low | Medium | Client-side flow control limits the number of in-flight streaming operations. If the client falls behind, the server drops intermediate updates and sends only the latest. |
| Action hermeticity violation | Medium | Critical | Workers execute actions in sandboxed directories with no network access and no shared state. Filesystem writes are restricted to the sandbox directory. Integration tests verify hermeticity. |

---

## 13. Performance Targets

| Metric | Target | Measurement |
|--------|--------|-------------|
| Remote action dispatch latency (scheduler → worker) | < 50ms | `glyim-bench bench_scheduler_dispatch` |
| CAS blob upload throughput | > 100 MB/s (LAN) | `glyim-bench bench_cas_upload` |
| CAS blob download throughput | > 100 MB/s (LAN) | `glyim-bench bench_cas_download` |
| Action deduplication hit rate (team sharing) | > 30% | `glyim-bench bench_dedup_hit_rate` |
| Speculative pre-compilation hit rate | > 40% | `glyim-bench bench_speculative_hit_rate` |
| Worker health check latency | < 5s | Scheduler metrics |
| Scheduler throughput (actions/second) | > 100 actions/s | `glyim-bench bench_scheduler_throughput` |
| GC pass time (10K blobs) | < 30s | `glyim-bench bench_gc` |
| Hybrid build (50% local, 50% remote) vs. all-local | < 1.2x wall-clock (for overhead) | `glyim-bench bench_hybrid_build` |
| Full remote build vs. all-local (3 workers) | < 0.5x wall-clock | `glyim-bench bench_remote_build` |
| Cross-compilation dispatch overhead | < 100ms per action | `glyim-bench bench_cross_compile` |
| CAS disk space after GC (steady state) | < 2x active artifacts | Monitoring dashboard |

---

## 14. Security Model

### 14.1 Threat Model

| Threat | Mitigation |
|--------|------------|
| Unauthorized artifact access | Token-based authentication; namespace isolation; role-based access control |
| Artifact poisoning (malicious worker uploads tampered code) | Content hash verification on download; optional signing of artifacts with worker private key |
| Scheduler DoS (flooding with actions) | Rate limiting per identity; maximum queue depth; priority-based admission control |
| Worker impersonation | Worker registration requires valid auth token; scheduler validates worker identity on heartbeat |
| Input data exfiltration (worker reads proprietary source) | Workers execute in ephemeral sandboxes; no persistent storage; CAS access is logged and auditable |
| Supply chain attack (malicious CAS server) | Content hash verification ensures artifact integrity; clients can pin expected hashes |

### 14.2 Authentication Flow

```
1. Client obtains auth token (API key or OAuth2 JWT)
2. Client includes token in gRPC/HTTP metadata: "Authorization: Bearer <token>"
3. CAS server validates token against configured auth backend
4. CAS server extracts identity (principal, namespaces, roles)
5. CAS server checks authorization for the requested operation
6. If authorized: proceed; if not: return UNAUTHENTICATED or PERMISSION_DENIED
```

---

## 15. Distributed Metrics and Observability

### 15.1 Metrics Extensions to `glyim-bench`

```rust
// crates/glyim-bench/src/distributed_metrics.rs

use serde::{Serialize, Deserialize};
use std::collections::HashMap;

/// Metrics for a distributed compilation session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DistributedBuildMetrics {
    /// Total wall-clock time for the distributed build.
    pub total_duration: std::time::Duration,
    /// Time spent waiting for the scheduler to dispatch actions.
    pub scheduler_wait_time: std::time::Duration,
    /// Time spent transferring inputs to workers.
    pub input_transfer_time: std::time::Duration,
    /// Time spent executing actions on workers.
    pub remote_execution_time: std::time::Duration,
    /// Time spent transferring outputs from workers.
    pub output_transfer_time: std::time::Duration,
    /// Time spent on local compilation (hybrid mode).
    pub local_execution_time: std::time::Duration,
    /// Number of actions executed remotely.
    pub remote_actions: usize,
    /// Number of actions executed locally.
    pub local_actions: usize,
    /// Number of actions served from dedup cache.
    pub dedup_cache_hits: usize,
    /// Number of actions served from CAS cache.
    pub cas_cache_hits: usize,
    /// Number of speculative actions that were later used.
    pub speculative_hits: usize,
    /// Number of speculative actions that were wasted.
    pub speculative_misses: usize,
    /// Per-worker utilization (worker_id → (busy_time, total_time)).
    pub worker_utilization: HashMap<String, (std::time::Duration, std::time::Duration)>,
    /// Scheduler queue depth over time (sampled).
    pub queue_depth_samples: Vec<(std::time::Instant, usize)>,
}
```

### 15.2 Scheduler Metrics

```rust
// crates/glyim-scheduler/src/metrics.rs

use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;

/// Scheduler metrics, updated atomically.
pub struct SchedulerMetrics {
    /// Total actions submitted.
    pub actions_submitted: AtomicU64,
    /// Total actions completed successfully.
    pub actions_completed: AtomicU64,
    /// Total actions failed.
    pub actions_failed: AtomicU64,
    /// Total actions retried.
    pub actions_retried: AtomicU64,
    /// Total dedup cache hits.
    pub dedup_cache_hits: AtomicU64,
    /// Total CAS cache hits.
    pub cas_cache_hits: AtomicU64,
    /// Current queue depth.
    pub current_queue_depth: AtomicUsize,
    /// Peak queue depth.
    pub peak_queue_depth: AtomicUsize,
    /// Total workers registered.
    pub workers_registered: AtomicUsize,
    /// Total workers currently healthy.
    pub workers_healthy: AtomicUsize,
    /// Total speculative actions submitted.
    pub speculative_submitted: AtomicU64,
    /// Total speculative actions that were later used.
    pub speculative_hits: AtomicU64,
}

impl SchedulerMetrics {
    pub fn new() -> Self {
        Self {
            actions_submitted: AtomicU64::new(0),
            actions_completed: AtomicU64::new(0),
            actions_failed: AtomicU64::new(0),
            actions_retried: AtomicU64::new(0),
            dedup_cache_hits: AtomicU64::new(0),
            cas_cache_hits: AtomicU64::new(0),
            current_queue_depth: AtomicUsize::new(0),
            peak_queue_depth: AtomicUsize::new(0),
            workers_registered: AtomicUsize::new(0),
            workers_healthy: AtomicUsize::new(0),
            speculative_submitted: AtomicU64::new(0),
            speculative_hits: AtomicU64::new(0),
        }
    }

    pub fn record_action_queued(&self, priority: &ActionPriority) {
        self.actions_submitted.fetch_add(1, Ordering::Relaxed);
        let current = self.current_queue_depth.fetch_add(1, Ordering::Relaxed) + 1;
        self.peak_queue_depth.fetch_max(current, Ordering::Relaxed);
    }

    pub fn record_action_dispatched(&self) {
        self.current_queue_depth.fetch_sub(1, Ordering::Relaxed);
    }

    pub fn record_action_completed(&self, success: bool) {
        if success {
            self.actions_completed.fetch_add(1, Ordering::Relaxed);
        } else {
            self.actions_failed.fetch_add(1, Ordering::Relaxed);
        }
    }

    pub fn record_cache_hit(&self) {
        self.dedup_cache_hits.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_action_retried(&self) {
        self.actions_retried.fetch_add(1, Ordering::Relaxed);
    }

    /// Generate a snapshot of the current metrics.
    pub fn snapshot(&self) -> SchedulerMetricsSnapshot {
        SchedulerMetricsSnapshot {
            actions_submitted: self.actions_submitted.load(Ordering::Relaxed),
            actions_completed: self.actions_completed.load(Ordering::Relaxed),
            actions_failed: self.actions_failed.load(Ordering::Relaxed),
            actions_retried: self.actions_retried.load(Ordering::Relaxed),
            dedup_cache_hits: self.dedup_cache_hits.load(Ordering::Relaxed),
            cas_cache_hits: self.cas_cache_hits.load(Ordering::Relaxed),
            current_queue_depth: self.current_queue_depth.load(Ordering::Relaxed),
            peak_queue_depth: self.peak_queue_depth.load(Ordering::Relaxed),
            workers_registered: self.workers_registered.load(Ordering::Relaxed),
            workers_healthy: self.workers_healthy.load(Ordering::Relaxed),
            speculative_submitted: self.speculative_submitted.load(Ordering::Relaxed),
            speculative_hits: self.speculative_hits.load(Ordering::Relaxed),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SchedulerMetricsSnapshot {
    pub actions_submitted: u64,
    pub actions_completed: u64,
    pub actions_failed: u64,
    pub actions_retried: u64,
    pub dedup_cache_hits: u64,
    pub cas_cache_hits: u64,
    pub current_queue_depth: usize,
    pub peak_queue_depth: usize,
    pub workers_registered: usize,
    pub workers_healthy: usize,
    pub speculative_submitted: u64,
    pub speculative_hits: u64,
}
```

---

## 16. Migration Strategy

### 16.1 Remote Execution Opt-In

Remote execution is completely opt-in. The default `ExecutionStrategy` is `LocalOnly`, which means the existing build workflow is unchanged. Users who want distributed compilation must explicitly enable it:

1. **v1.0.0 (Phase 8 release)**: No remote execution. All compilation is local.
2. **v1.1.0 (Phase 9 initial release)**: `--remote-execution` flag available but off by default. `glyim worker` and `glyim scheduler` commands available for manual setup.
3. **v1.2.0 (future)**: Configuration file support for `execution_strategy` in `glyim.toml`. Automatic worker discovery via mDNS.
4. **v2.0.0 (future)**: Remote execution as a first-class feature with auto-configuration.

### 16.2 CAS Server Upgrade

The CAS server upgrade is backward-compatible:

1. **v1.0.0 CAS server**: No authentication, no namespaces, no GC. Existing REST and gRPC APIs unchanged.
2. **v1.1.0 CAS server**: New `Execution` gRPC service. Authentication is optional (`AuthMode::None` by default). Namespace isolation is optional (default namespace: `public`). GC daemon is disabled by default.
3. **Migration**: Existing CAS data is automatically assigned to the `public` namespace. No data migration required.

### 16.3 Orchestrator Compatibility

The orchestrator's `build()` method is extended with an optional `ExecutionStrategy` parameter. The default is `LocalOnly`, which preserves the existing behavior. Users who set `--remote-execution` get the hybrid or remote strategy.

```rust
// crates/glyim-orchestrator/src/orchestrator.rs (extended)

pub struct OrchestratorConfig {
    pub mode: BuildMode,
    pub target: Option<String>,
    pub remote_cache_url: Option<String>,
    pub remote_cache_token: Option<String>,
    pub force_rebuild: bool,
    // NEW: execution strategy for distributed compilation
    pub execution_strategy: ExecutionStrategy,
    // NEW: maximum number of packages to compile in parallel
    pub max_parallel_packages: usize,
}

impl Default for OrchestratorConfig {
    fn default() -> Self {
        Self {
            mode: BuildMode::Debug,
            target: None,
            remote_cache_url: None,
            remote_cache_token: None,
            force_rebuild: false,
            execution_strategy: ExecutionStrategy::LocalOnly,
            max_parallel_packages: 1, // backward compatible: sequential
        }
    }
}
```

---

## 17. Success Criteria

### 17.1 Functionality Criteria

- [ ] Bazel REv2 `Execution` gRPC API is implemented and passes the Bazel remote execution compatibility test suite
- [ ] Remote worker daemon (`glyim worker`) can connect to the scheduler, execute actions, and report results
- [ ] Build scheduler (`glyim scheduler`) dispatches actions to workers, handles failures with retries, and deduplicates identical actions
- [ ] Speculative pre-compilation produces cache hits for at least 40% of IDE-triggered incremental builds
- [ ] Orchestrator compiles packages in parallel within topological layers
- [ ] Hybrid execution strategy sends large actions remotely while executing small actions locally
- [ ] Cross-compilation actions are dispatched to workers with matching target triples

### 17.2 Reliability Criteria

- [ ] Worker failure during action execution triggers automatic retry on a different worker
- [ ] Scheduler restart does not lose in-flight actions (they are re-queued from the CAS)
- [ ] CAS garbage collection does not delete artifacts referenced by any lockfile
- [ ] Namespace isolation prevents cross-team artifact access
- [ ] Authentication rejects invalid tokens and unauthorized operations

### 17.3 Performance Criteria

- [ ] Remote action dispatch latency (scheduler → worker) is under 50ms
- [ ] CAS blob transfer throughput exceeds 100 MB/s on LAN
- [ ] Full remote build with 3 workers is at least 2x faster than all-local build for a workspace with 5+ packages
- [ ] Hybrid build overhead (vs. all-local) is under 20% for small projects
- [ ] GC pass on 10K blobs completes in under 30 seconds
- [ ] Scheduler handles 100 actions per second

### 17.4 Security Criteria

- [ ] All CAS operations require authentication when `AuthMode` is not `None`
- [ ] Namespace isolation prevents cross-namespace reads
- [ ] Content hash verification detects corrupted blobs
- [ ] Worker sandboxing prevents access to files outside the action directory
- [ ] Audit trail: all CAS read/write operations are logged with identity and timestamp

---

## 18. Operational Guide

### 18.1 Minimal Setup (Single Worker)

For a single developer wanting to offload compilation to a home server:

```bash
# On the build server (e.g., 32-core Linux machine):
glyim worker --scheduler http://localhost:9092 --cas http://localhost:9090 --max-concurrency 4

# Start the scheduler (can run on the same machine):
glyim scheduler --port 9092 --cas http://localhost:9090

# Start the CAS server:
glyim cas-server --port 9090 --grpc-port 9091

# On the developer laptop:
glyim build --remote-execution http://build-server:9092 --execution-strategy hybrid
```

### 18.2 Team Setup (Multiple Workers, Shared CAS)

For a team with multiple build servers and shared caching:

```bash
# CAS server (shared, on dedicated storage node):
glyim cas-server --port 9090 --grpc-port 9091 \
  --auth-mode oauth2 --jwks-url https://auth.company.com/.well-known/jwks.json \
  --gc-enabled --gc-interval 3600

# Scheduler (on dedicated scheduling node):
glyim scheduler --port 9092 --cas http://cas:9090 \
  --token $SCHEDULER_TOKEN --speculative --max-queue-depth 5000

# Workers (on build farm nodes):
glyim worker --scheduler http://scheduler:9092 --cas http://cas:9090 \
  --max-concurrency 8 --target x86_64-unknown-linux-gnu --token $WORKER_TOKEN

# ARM worker for cross-compilation:
glyim worker --scheduler http://scheduler:9092 --cas http://cas:9090 \
  --max-concurrency 4 --target aarch64-unknown-linux-gnu --token $WORKER_TOKEN

# Developer machines:
glyim build --remote-execution http://scheduler:9092 \
  --execution-strategy hybrid --token $DEV_TOKEN
```

### 18.3 CI/CD Setup

For CI pipelines that want to leverage the build farm:

```bash
# CI build (all remote, no local compilation):
glyim build --remote-execution http://scheduler:9092 \
  --execution-strategy remote --token $CI_TOKEN

# Cache artifacts from this build for future CI runs:
glyim build --remote-execution http://scheduler:9092 \
  --execution-strategy remote --remote-cache http://cas:9090 --token $CI_TOKEN
```

---

## 19. Future Extensions (Beyond Phase 9)

Phase 9 establishes the distributed compilation infrastructure. Future extensions that build on this foundation include:

1. **Auto-scaling worker pools**: Automatically spin up cloud VMs as workers when the scheduler queue depth exceeds a threshold, and shut them down when idle.

2. **GPU-accelerated e-graph optimization**: Use GPU workers for parallel e-graph equality saturation, potentially achieving 10x+ speedup for large functions.

3. **Build artifact signing**: Sign compilation artifacts with worker private keys to establish a verifiable supply chain from source to binary.

4. **Remote JIT execution**: Execute JIT-compiled functions on remote workers with low-latency IPC, enabling distributed testing and debugging.

5. **Intelligent scheduling with ML**: Use machine learning to predict compilation times, optimize action-to-worker assignment, and improve speculative pre-compilation accuracy.

6. **Multi-tenant scheduler**: Support multiple organizations sharing a single scheduler with resource quotas and priority preemption.

7. **Build event protocol**: Stream build events (similar to Bazel's BEP) to monitoring dashboards for real-time visibility into distributed builds.
