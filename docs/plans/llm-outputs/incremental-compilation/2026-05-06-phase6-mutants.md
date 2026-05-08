# Glyim Incremental Compiler — Phase 6 Implementation Plan

## Test-Aware Compilation & Mutation Testing Integration

**Codebase:** elcoosp-glyim v0.5.0  
**Rust Workspace | 20 Crates | LLVM 22.1 / Inkwell 0.9**  
**Date:** 2026-05-07

---

## 1. Executive Summary

Phase 6 transforms Glyim from a compiler that treats test functions as ordinary code into a test-aware compilation platform with integrated mutation testing. Phases 0 through 5 built a complete incremental compilation pipeline with per-function caching, cross-module linking, and CAS-backed artifact sharing, but the compiler has no concept of test semantics: it does not propagate `#[test]` metadata through the HIR, it cannot instrument code for coverage, it cannot apply systematic mutations to source programs, and it cannot measure test quality through mutation scores. The test runner (`glyim-testr`) is functional — it collects `#[test]` functions from the AST, compiles the source with an injected harness, and executes each test in a subprocess with timeout handling — but it operates entirely outside the compiler's incremental pipeline. Each test run is a full compile-link-execute cycle with no reuse of artifacts from previous runs, no understanding of which tests are affected by a change, and no mechanism to evaluate whether the test suite actually exercises the program's logic.

Phase 6 closes these gaps through three interconnected capabilities. First, **test-aware compilation** extends the HIR with test metadata so the compiler can make test-specific decisions during code generation: coverage-instrumented builds inject LLVM-level counters into branch points and function entry, test functions can be JIT-called individually without the full compile-link-execute cycle, and the incremental pipeline from Phase 4 can skip re-running tests whose dependencies are unchanged. Second, **mutation testing** introduces a new `glyim-mutant` crate that applies semantic mutations at the HIR level — swapping arithmetic operators, negating boolean conditions, replacing constants with boundary values, deleting statements — and compiles each mutant incrementally by reusing the per-function artifact cache for all unchanged functions. Third, **effect analysis** classifies functions as pure or impure, enabling the mutation engine to prune equivalent mutants (mutations that cannot change observable behavior because they affect dead code or pure expressions whose results are discarded) and to prioritize mutations in functions with side effects.

The phase also implements the `FlakeTracker` (currently a placeholder that always returns 0.0), extends the `TestDependencyGraph` (currently a placeholder that always returns all tests), and adds a `--mutate` flag to `glyim test` that runs the full mutation testing workflow. The mutation score — the ratio of killed mutants to total non-equivalent mutants — is persisted in the incremental state directory and can be compared against a threshold via `--mutation-score <N>` to fail CI builds when test quality drops below an acceptable level.

**Estimated effort:** 30–42 working days.

**Key deliverables:**
- HIR test metadata propagation (`HirFn::is_test`, `HirFn::test_config`)
- Coverage instrumentation in `glyim-codegen-llvm` (branch + function-entry counters)
- `glyim-mutant` crate with semantic mutation operators applied at HIR level
- Effect analysis (`glyim-hir/src/effects.rs`) for purity classification
- Mutation test runner integrating with incremental compilation pipeline
- Equivalent mutant detection via semantic hash comparison and dead-code analysis
- JIT-based test function execution (`run_jit_test()`)
- Functional `FlakeTracker` and `TestDependencyGraph` replacing placeholders
- `--mutate`, `--coverage`, and `--mutation-score` CLI flags
- Mutation score persistence in incremental state

---

## 2. Current Codebase State Assessment

### 2.1 Test Runner (As-Is)

The `glyim-testr` crate provides a complete async test execution framework with these components:

| Component | File | Status | Gap |
|-----------|------|--------|-----|
| `Compiler` | `compiler.rs` | Compiles test source to binary via `glyim_compiler::pipeline::build()` | No incremental compilation; every test run is a full build |
| `collect_tests` | `collector.rs` | Finds `#[test]`, `#[test(should_panic)]`, `#[ignore]` functions in AST | Does not propagate test metadata to HIR; information lost after parsing |
| `inject_harness` | `harness.rs` | Rewrites source to add `main()` that reads `GLYIM_TEST` env var | Source-level injection is fragile; cannot inject at HIR/LLVM level |
| `Executor` | `executor.rs` | Spawns binary per-test with timeout; parses PASS/FAIL output | Works correctly; can be reused for mutation test execution |
| `TestRunner` | `runner.rs` | Orchestrates full test suite with parallel execution | No mutation mode; always runs all collected tests |
| `TestConfig` | `config.rs` | Filter, timeout, jobs, priority mode, optimize_check | No `mutation_mode`, `coverage_mode`, or `mutation_score_threshold` fields |
| `TestOutcome` | `types.rs` | Passed, Failed, TimedOut, Crash, FlakyPass, CompilationError, InternalError | Sufficient; `MutationKilled` / `MutationSurvived` outcomes to be added |
| `TestDependencyGraph` | `incremental.rs` | **Placeholder**: empty struct, `affected_tests()` returns all tests | Must be replaced with real dependency-aware test selection |
| `FlakeTracker` | `flaky.rs` | **Placeholder**: score always 0.0 | Must be implemented with real flake detection logic |
| `FileWatcher` | `watcher.rs` | **Stub**: rx channel never sends | To be replaced by `glyim-watch` from Phase 4 |
| `SnapshotStore` | `snapshot.rs` | Functional snapshot testing | Can be leveraged for mutation result comparison |
| `prioritizer` | `prioritizer.rs` | DeclarationOrder / FastFirst / RecentFailuresFirst | Can be extended with mutation-aware ordering |
| `optimize_check` | `optimize.rs` | FileCheck integration for optimization verification | Orthogonal to mutation testing |

### 2.2 Test Execution Model (Dual Path)

The codebase has two test execution paths that are not unified:

| Path | Level | Harness Injection | Execution | Used By |
|------|-------|-------------------|-----------|---------|
| **Subprocess** | Source | `inject_harness()` rewrites `.g` source to add `main()` | Each test = separate OS process with `GLYIM_TEST=<name>` env var | `glyim test` (production) |
| **In-process** | LLVM IR | `emit_test_harness()` generates LLVM `@main` that iterates test names | All tests in single process | `compile_to_ir_tests()` (debug/internals) |

Neither path supports per-test coverage instrumentation or per-test JIT execution. The subprocess path is crash-safe (a crashing test does not bring down the runner) but slow (full process spawn per test). The in-process path is fast but unsafe (a crashing test aborts the entire process). Phase 6 introduces a third path — **JIT per-test execution** — that combines the safety of process isolation with the speed of JIT compilation.

### 2.3 HIR Test Metadata (Missing)

The HIR has no concept of test functions. The `HirFn` struct contains:

```rust
pub struct HirFn {
    pub doc: Option<String>,
    pub name: Symbol,
    pub type_params: Vec<Symbol>,
    pub params: Vec<(Symbol, HirType)>,
    pub param_mutability: Vec<bool>,
    pub ret: Option<HirType>,
    pub body: HirExpr,
    pub span: Span,
    pub is_pub: bool,
    pub is_macro_generated: bool,
    pub is_extern_backed: bool,
    // NO is_test, test_config, or should_panic fields!
}
```

Test detection happens only at the AST level in `glyim-testr/src/collector.rs`, which checks `attrs` on `Item::FnDef`. This information is lost when the AST is lowered to HIR, preventing the compiler from making test-aware decisions during type checking, optimization, and code generation.

### 2.4 Coverage Instrumentation (Missing)

There is zero coverage instrumentation in the codebase. No source maps, no branch tracking, no line coverage, and no sanitizer integration. The closest existing feature is DWARF debug info generation (`DebugMode::Full` / `LineTablesOnly`) in `glyim-codegen-llvm/src/debug.rs`, which generates LLVM debug metadata for breakpoints and stack traces but does not track execution counts.

LLVM provides built-in coverage instrumentation via the `-fprofile-instr-generate` flag and the `llvm.instrprof.increment` intrinsic, which can be accessed through Inkwell's `module.add_global()` and `builder.build_load()`/`build_store()` APIs. Phase 6 leverages these to add coverage counters without reinventing instrumentation from scratch.

### 2.5 Mutation Testing (Missing)

There is no Glyim-level mutation testing. The only mutation testing is Rust-level via `cargo mutants` in CI, which mutates the compiler's own Rust source code. This is completely separate from Glyim-level mutation testing, which would mutate `.g` source programs and check whether their test suites catch the mutations. No mutation operators, no mutant generation, no mutation scoring, and no integration with the incremental compilation pipeline exist.

### 2.6 Effect Analysis (Missing)

There is no effect analysis module. The `glyim-hir/src/dependency_names.rs` tracks which symbols a function references (for dependency graph construction) but does not classify effects (read, write, IO, pure). The `glyim-hir/src/normalize.rs` normalizes HIR for semantic hashing but does not analyze purity. Effect analysis is essential for mutation testing because pure functions can be mutated more aggressively (mutations to pure expressions in dead positions are trivially equivalent) and impure functions should be prioritized for mutation (they are more likely to produce observable behavior changes).

### 2.7 JIT Execution (Limited)

The JIT in `glyim-compiler/src/pipeline.rs` can only call `main()`:

```rust
fn execute_jit(compiled: &CompiledHir, mode: BuildMode, target: Option<&str>) -> Result<i32, PipelineError>
```

There is no way to JIT-call a specific test function. This is a critical gap for mutation testing, where the bottleneck is running the test suite against each mutant. Full compile-link-execute cycles per mutant are prohibitively expensive; JIT-based test execution can reduce the per-mutant cost by an order of magnitude.

### 2.8 Critical Gaps That Phase 6 Addresses

| Gap | Impact | Affected Crate | Phase 6 Solution |
|-----|--------|---------------|-------------------|
| No test metadata in HIR | Compiler cannot make test-aware decisions | `glyim-hir` | Add `is_test`, `test_config` to `HirFn` |
| No coverage instrumentation | Cannot measure test effectiveness | `glyim-codegen-llvm` | LLVM `instrprof` counters at branch points and function entries |
| No mutation testing | Cannot evaluate test suite quality | (missing) | New `glyim-mutant` crate with HIR-level mutation operators |
| No effect analysis | Cannot prune equivalent mutants | `glyim-hir` | New `effects.rs` module with purity classification |
| Test dependency graph is placeholder | Incremental test selection is non-functional | `glyim-testr` | Real `TestDependencyGraph` with HIR-level dependency edges |
| Flake tracker is placeholder | Flaky tests are not detected | `glyim-testr` | Real `FlakeTracker` with statistical flake detection |
| JIT cannot call individual tests | Mutation testing requires full compile-link-execute per mutant | `glyim-compiler` | `run_jit_test()` function for per-test JIT execution |
| No mutation-aware CLI | Users cannot run mutation tests | `glyim-cli` | `--mutate`, `--coverage`, `--mutation-score` flags |

---

## 3. Architecture Design

### 3.1 Test-Aware Compilation

The core architectural change is extending the HIR with test metadata so every compilation stage can reason about test semantics. Currently, test functions are indistinguishable from regular functions after AST lowering. Phase 6 adds an `is_test: bool` field and a `test_config: Option<TestConfig>` field to `HirFn`, propagated during the HIR lowering pass.

```rust
// crates/glyim-hir/src/lib.rs (extended)

/// Configuration for test functions, propagated from AST attributes.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct HirTestConfig {
    /// Whether the test is expected to panic.
    pub should_panic: bool,
    /// Whether the test is ignored.
    pub ignored: bool,
    /// User-defined tags for filtering.
    pub tags: Vec<String>,
    /// Source file where the test is defined.
    pub source_file: String,
}

// Extended HirFn
pub struct HirFn {
    pub doc: Option<String>,
    pub name: Symbol,
    pub type_params: Vec<Symbol>,
    pub params: Vec<(Symbol, HirType)>,
    pub param_mutability: Vec<bool>,
    pub ret: Option<HirType>,
    pub body: HirExpr,
    pub span: Span,
    pub is_pub: bool,
    pub is_macro_generated: bool,
    pub is_extern_backed: bool,
    // NEW FIELDS:
    pub is_test: bool,
    pub test_config: Option<HirTestConfig>,
}
```

The `is_test` and `test_config` fields are populated during `lower_with_declarations()` by reading the `attrs` field of each `Item::FnDef` in the AST, which already contains `#[test]`, `#[test(should_panic)]`, and `#[ignore]` attributes. The lowering pass currently discards these attributes; Phase 6 preserves them.

### 3.2 Coverage Instrumentation

Coverage instrumentation is implemented as an optional pass in `glyim-codegen-llvm` that injects LLVM profiler counters. The instrumentation is activated by a new `CoverageMode` enum in the `Codegen` configuration:

```rust
// crates/glyim-codegen-llvm/src/codegen/mod.rs (extended)

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CoverageMode {
    /// No instrumentation (default).
    Off,
    /// Instrument function entries only.
    Function,
    /// Instrument function entries and branch conditions.
    Branch,
    /// Instrument function entries, branch conditions, and expression evaluations.
    Full,
}
```

When `CoverageMode` is not `Off`, the code generator injects a global counter array into the LLVM module:

```rust
// The counter array: one 64-bit counter per instrumentation point.
// __glyim_cov_counts[point_id] is incremented each time the point is reached.
let counter_array = module.add_global(
    i64_type.array_type(num_instrumentation_points as u32),
    Some(3),  // alignment
    "__glyim_cov_counts",
);
counter_array.set_initializer(&i64_type.const_zero_array(num_instrumentation_points));
counter_array.set_linkage(Linkage::Internal);
```

At each instrumentation point (function entry, `if` condition, `match` arm), the code generator emits an increment:

```rust
// At function entry:
let counter_ptr = unsafe {
    builder.build_in_bounds_gep(
        i64_type.array_type(num_points),
        counter_array.as_pointer_value(),
        &[i32_type.const_int(0, false), i32_type.const_int(point_id, false)],
        "cov_inc",
    )
};
let current = builder.build_load(i64_type, counter_ptr, "cov_load");
let incremented = builder.build_int_add(current.into_int_value(), i64_type.const_int(1, false), "cov_add");
builder.build_store(counter_ptr, incremented);
```

After test execution, the counter array is read via a runtime API (`__glyim_cov_dump()`) that writes the counts to a file. The coverage data is then consumed by the mutation engine to determine which mutants are "reached" by the test suite (a mutant that is never executed cannot be killed, and reaching it is a prerequisite for meaningful mutation testing).

### 3.3 Mutation Engine Architecture

The mutation engine operates at the HIR level, applying semantic mutations to `HirExpr` nodes within function bodies. This is fundamentally different from source-level mutation (which operates on text) and from AST-level mutation (which operates on syntax trees). HIR-level mutation has three key advantages:

1. **Type-awareness**: The mutation engine knows the types of all expressions, so it can only apply type-compatible mutations. Swapping `+` for `-` on integers is valid; swapping `+` for `-` on strings is not.
2. **Post-type-checking**: Mutations are applied after type checking, so the mutated HIR is guaranteed to be well-typed. There is no need for a separate type-checking pass on each mutant.
3. **Incremental reuse**: The mutation engine produces a new `Hir` that differs from the original in exactly one function. The query-driven pipeline from Phase 4 can reuse all cached artifacts for the unchanged functions, recompiling only the mutated function.

```rust
// crates/glyim-mutant/src/lib.rs

use glyim_hir::{Hir, HirFn, HirExpr, HirType, HirItem};
use glyim_interner::Symbol;
use std::collections::HashMap;

/// A single mutation applied to a specific location in a function.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Mutation {
    /// Unique identifier for this mutation.
    pub id: MutationId,
    /// The function being mutated.
    pub function_name: Symbol,
    /// The mutation operator that was applied.
    pub operator: MutationOperator,
    /// Description of the original expression.
    pub original: String,
    /// Description of the mutated expression.
    pub replacement: String,
    /// The expression ID being mutated (for precise identification).
    pub expr_id: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct MutationId(pub u64);

/// The set of mutation operators supported by the engine.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum MutationOperator {
    // Arithmetic operator mutations
    ArithmeticPlusToMinus,       // a + b → a - b
    ArithmeticMinusToPlus,       // a - b → a + b
    ArithmeticMulToDiv,          // a * b → a / b
    ArithmeticDivToMul,          // a / b → a * b
    ArithmeticModToDiv,          // a % b → a / b

    // Comparison operator mutations
    CompareLessToLessEqual,      // a < b → a <= b
    CompareGreaterToGreaterEqual, // a > b → a >= b
    CompareEqualToNotEqual,      // a == b → a != b
    CompareNotEqualToEqual,      // a != b → a == b
    CompareLessEqualToLess,      // a <= b → a < b
    CompareGreaterEqualToGreater, // a >= b → a > b

    // Boolean operator mutations
    BooleanAndToOr,              // a && b → a || b
    BooleanOrToAnd,              // a || b → a && b
    BooleanNotElimination,       // !a → a

    // Constant mutations
    ConstantZero,                // n → 0
    ConstantOne,                 // n → 1
    ConstantBoundary,            // n → n - 1 (if n > 0) or n + 1

    // Statement mutations
    StatementDeletion,           // Remove a statement (replace with unit)

    // Conditional mutations
    ConditionalFlip,             // if c → if !c

    // Return value mutations
    ReturnValueZero,             // return n → return 0
    ReturnValueNegate,           // return n → return -n (for numeric types)
}

/// The mutation engine generates all possible mutations for a given HIR.
pub struct MutationEngine {
    /// Configuration controlling which operators are active.
    config: MutationConfig,
    /// The next mutation ID to assign.
    next_id: MutationId,
    /// Collected mutations.
    mutations: Vec<Mutation>,
    /// Per-function mutation count (for reporting).
    fn_mutation_counts: HashMap<Symbol, usize>,
}

#[derive(Debug, Clone)]
pub struct MutationConfig {
    /// Which mutation operators to enable.
    pub operators: Vec<MutationOperator>,
    /// Whether to skip pure functions (they rarely produce killable mutants).
    pub skip_pure: bool,
    /// Whether to skip test functions (mutating tests themselves is usually not useful).
    pub skip_tests: bool,
    /// Maximum number of mutations per function (prevents explosion in large functions).
    pub max_mutations_per_fn: usize,
    /// Whether to include equivalent mutant detection.
    pub detect_equivalents: bool,
}

/// Result of applying a mutation to a HIR.
pub struct MutatedHir {
    /// The mutated HIR (differs from original in exactly one function).
    pub hir: Hir,
    /// The mutation that was applied.
    pub mutation: Mutation,
    /// The semantic hash of the mutated function (for cache keying).
    pub mutated_fn_hash: glyim_macro_vfs::ContentHash,
}
```

### 3.4 Effect Analysis

The effect analysis module classifies each function's side effects into categories, enabling the mutation engine to make informed pruning decisions:

```rust
// crates/glyim-hir/src/effects.rs

use crate::{Hir, HirFn, HirExpr, HirType, HirItem};
use glyim_interner::Symbol;
use std::collections::{HashMap, HashSet};

/// The set of effects a function may have.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct EffectSet {
    /// The function reads from mutable state.
    pub reads_mutable: bool,
    /// The function writes to mutable state.
    pub writes_mutable: bool,
    /// The function performs I/O (print, read, etc.).
    pub performs_io: bool,
    /// The function may panic or abort.
    pub may_panic: bool,
    /// The function calls other functions with these effects.
    pub transitive_effects: HashSet<Symbol>,
}

impl EffectSet {
    pub fn pure() -> Self {
        Self {
            reads_mutable: false,
            writes_mutable: false,
            performs_io: false,
            may_panic: false,
            transitive_effects: HashSet::new(),
        }
    }

    pub fn is_pure(&self) -> bool {
        !self.reads_mutable && !self.writes_mutable && !self.performs_io
    }

    pub fn is_impure(&self) -> bool {
        !self.is_pure()
    }
}

/// Analyzes the effects of all functions in a HIR.
pub struct EffectAnalyzer {
    /// Per-function effect sets, populated during analysis.
    effects: HashMap<Symbol, EffectSet>,
    /// Functions that are known to be impure (builtins, runtime functions).
    known_impure: HashSet<Symbol>,
}

impl EffectAnalyzer {
    pub fn new() -> Self { ... }

    /// Analyze all functions in the HIR, computing their effect sets.
    pub fn analyze(&mut self, hir: &Hir) -> &HashMap<Symbol, EffectSet> { ... }

    /// Get the effect set for a specific function.
    pub fn get_effects(&self, fn_name: Symbol) -> Option<&EffectSet> { ... }

    /// Determine whether a mutation at the given expression can change
    /// the function's observable behavior.
    pub fn mutation_is_observable(&self, fn_name: Symbol, expr: &HirExpr) -> bool { ... }
}
```

The `EffectAnalyzer` performs a bottom-up traversal of the call graph (using the dependency information from `glyim-hir/src/dependency_names.rs`), starting with leaf functions and propagating effects transitively. Built-in functions (`print`, `read_line`, `Vec.push`, etc.) are pre-registered as impure. A function is classified as pure if it does not read or write mutable state, does not perform I/O, and all functions it calls (transitively) are also pure.

### 3.5 JIT Test Execution

The JIT execution model is extended to support calling individual test functions:

```rust
// crates/glyim-compiler/src/pipeline.rs (extended)

/// Execute a single test function via JIT, returning the test result.
pub fn run_jit_test(
    source: &str,
    test_name: &str,
    timeout: Option<std::time::Duration>,
) -> Result<TestExecutionResult, PipelineError> { ... }

pub struct TestExecutionResult {
    pub test_name: String,
    pub outcome: TestJitOutcome,
    pub duration: std::time::Duration,
    pub coverage: Option<Vec<(usize, u64)>>,  // (point_id, count) if coverage enabled
}

pub enum TestJitOutcome {
    Passed,
    Failed(String),       // panic message
    TimedOut,
}
```

The `run_jit_test()` function follows this flow:

1. Parse and compile the source to a `CompiledHir` using the query-driven pipeline.
2. Create a `Codegen` with `with_jit_mode()` and `with_coverage_mode()`.
3. Generate LLVM IR for the entire module.
4. Create a JIT execution engine.
5. Map runtime shims via `runtime_shims::map_runtime_shims_for_jit()`.
6. Look up the test function by name: `engine.get_function::<unsafe extern "C" fn() -> i32>(test_name)`.
7. Call the function with a timeout thread (spawn a thread that calls the function, join with timeout).
8. Return the result.

This avoids the full compile-link-execute cycle, reducing per-test execution time from hundreds of milliseconds (linking + process spawn) to tens of milliseconds (JIT call). For mutation testing with hundreds of mutants, this is the difference between minutes and hours.

---

## 4. New Crate: `glyim-mutant`

### 4.1 Crate Structure

```
crates/glyim-mutant/
├── Cargo.toml
└── src/
    ├── lib.rs           — public API, re-exports
    ├── engine.rs        — MutationEngine
    ├── operators.rs     — MutationOperator definitions and application logic
    ├── apply.rs         — Apply mutations to HIR, producing MutatedHir
    ├── equivalent.rs    — Equivalent mutant detection
    ├── score.rs         — Mutation score computation and persistence
    ├── config.rs        — MutationConfig
    └── tests/
        ├── mod.rs
        ├── operators_tests.rs
        ├── apply_tests.rs
        ├── equivalent_tests.rs
        └── score_tests.rs
```

### 4.2 Cargo.toml

```toml
[package]
name = "glyim-mutant"
version.workspace = true
edition.workspace = true
license.workspace = true
rust-version.workspace = true
description = "Mutation testing engine for Glyim"

[dependencies]
glyim-hir = { path = "../glyim-hir" }
glyim-interner = { path = "../glyim-interner" }
glyim-macro-vfs = { path = "../glyim-macro-vfs" }
glyim-compiler = { path = "../glyim-compiler" }
glyim-query = { path = "../glyim-query" }
glyim-merkle = { path = "../glyim-merkle" }
serde = { version = "1", features = ["derive"] }
tracing = "0.1"
sha2 = "0.11"

[dev-dependencies]
glyim-parse = { path = "../glyim-parse" }
glyim-typeck = { path = "../glyim-typeck" }
```

### 4.3 Mutation Operators (`operators.rs`)

Each mutation operator is implemented as a function that takes an `HirExpr` and returns `Option<HirExpr>` — the mutated expression, or `None` if the operator does not apply (e.g., `ArithmeticPlusToMinus` does not apply to a boolean expression). The operators are organized into categories:

```rust
// crates/glyim-mutant/src/operators.rs

use glyim_hir::{HirExpr, HirType, HirBinaryOp, HirUnaryOp};

/// Trait for mutation operators.
pub trait MutateOp: std::fmt::Debug + Clone + Send + Sync {
    /// The name of this operator (for reporting).
    fn name(&self) -> &'static str;

    /// Apply this mutation to the given expression.
    /// Returns None if the operator does not apply to this expression type.
    fn apply(&self, expr: &HirExpr, expr_type: Option<&HirType>) -> Option<HirExpr>;

    /// Whether this operator is applicable to the given expression type.
    fn is_applicable(&self, expr: &HirExpr, expr_type: Option<&HirType>) -> bool;
}

/// Built-in arithmetic mutation: swap + for -.
#[derive(Debug, Clone)]
pub struct ArithmeticPlusToMinus;

impl MutateOp for ArithmeticPlusToMinus {
    fn name(&self) -> &'static str { "ArithmeticPlusToMinus" }

    fn apply(&self, expr: &HirExpr, expr_type: Option<&HirType>) -> Option<HirExpr> {
        match expr {
            HirExpr::Binary(HirBinaryOp::Add, lhs, rhs, span) => {
                // Only apply to numeric types, not string concatenation
                if expr_type.map_or(true, |t| t.is_numeric()) {
                    Some(HirExpr::Binary(HirBinaryOp::Sub, lhs.clone(), rhs.clone(), *span))
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    fn is_applicable(&self, expr: &HirExpr, expr_type: Option<&HirType>) -> bool {
        matches!(expr, HirExpr::Binary(HirBinaryOp::Add, _, _, _))
            && expr_type.map_or(true, |t| t.is_numeric())
    }
}
```

The complete set of built-in operators is registered in a `BUILTIN_OPERATORS` constant and selected by `MutationConfig::operators`. The engine iterates over all expressions in a function body, checks which operators apply, and generates a `Mutation` for each applicable operator.

### 4.4 Mutation Application (`apply.rs`)

The `apply_mutation()` function takes an `Hir` and a `Mutation`, and produces a `MutatedHir` where exactly one expression in one function has been replaced. The function clones the HIR, navigates to the target expression by `expr_id`, replaces it with the mutated expression, and recomputes the semantic hash of the mutated function:

```rust
// crates/glyim-mutant/src/apply.rs

use crate::{Mutation, MutatedHir, MutationId};
use glyim_hir::{Hir, HirItem, HirExpr, HirFn};
use glyim_macro_vfs::ContentHash;

/// Apply a mutation to a HIR, producing a new HIR that differs in exactly one function.
pub fn apply_mutation(
    original_hir: &Hir,
    mutation: &Mutation,
    mutated_expr: HirExpr,
) -> MutatedHir {
    let mut hir = original_hir.clone();

    // Find the target function and replace the target expression
    for item in &mut hir.items {
        if let HirItem::Fn(fn_def) = item {
            if fn_def.name == mutation.function_name {
                let original_fn = fn_def.clone();
                replace_expr_by_id(&mut fn_def.body, mutation.expr_id, mutated_expr.clone());

                // Recompute semantic hash of the mutated function
                let mutated_fn_hash = glyim_hir::semantic_hash::semantic_hash_item(
                    &HirItem::Fn(fn_def.clone()),
                );

                return MutatedHir {
                    hir,
                    mutation: mutation.clone(),
                    mutated_fn_hash,
                };
            }
        }
    }

    panic!("Mutation references function {:?} not found in HIR", mutation.function_name)
}

/// Recursively find and replace an expression by its ID.
fn replace_expr_by_id(expr: &mut HirExpr, target_id: usize, replacement: HirExpr) {
    if expr.id() == target_id {
        *expr = replacement;
        return;
    }
    // Recurse into child expressions
    match expr {
        HirExpr::Binary(_, lhs, rhs, _) => {
            replace_expr_by_id(lhs, target_id, replacement.clone());
            replace_expr_by_id(rhs, target_id, replacement);
        }
        HirExpr::Call(_, args, _) => {
            for arg in args {
                replace_expr_by_id(arg, target_id, replacement.clone());
            }
        }
        HirExpr::If(cond, then_block, else_block, _) => {
            replace_expr_by_id(cond, target_id, replacement.clone());
            for stmt in then_block {
                replace_expr_by_id(stmt, target_id, replacement.clone());
            }
            if let Some(else_block) = else_block {
                for stmt in else_block {
                    replace_expr_by_id(stmt, target_id, replacement.clone());
                }
            }
        }
        // ... other expression variants ...
        _ => {}
    }
}
```

### 4.5 Equivalent Mutant Detection (`equivalent.rs`)

Equivalent mutants are mutations that do not change the program's observable behavior. Detecting them is undecidable in general, but several heuristic approaches can eliminate the most common classes:

1. **Semantic hash identity**: If the mutated function's semantic hash is identical to the original function's semantic hash, the mutation is trivially equivalent. This catches cases where the mutation is in dead code or where the mutated expression is not used (e.g., `let _ = x + 1` mutated to `let _ = x - 1` where the result is discarded).

2. **Dead code analysis**: If the mutated expression is in a function that is never called (determined by the dependency graph from Phase 4), the mutation is equivalent.

3. **Pure expression in void position**: If the mutated expression is a pure expression whose result is discarded (assigned to `_` or the last expression in a function returning unit), and the expression has no side effects (determined by `EffectAnalyzer`), the mutation is equivalent.

4. **Coverage unreachable**: If coverage data shows that the mutation point was never reached during the original test run, the mutation is not equivalent but is "unreachable" — it should be excluded from the mutation score denominator.

```rust
// crates/glyim-mutant/src/equivalent.rs

use crate::{Mutation, MutationId};
use glyim_hir::{Hir, EffectSet};
use glyim_macro_vfs::ContentHash;
use std::collections::{HashMap, HashSet};

/// The result of equivalent mutant analysis.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EquivalentStatus {
    /// The mutation is definitely equivalent (proven by a heuristic).
    Equivalent(EquivalentReason),
    /// The mutation is possibly non-equivalent (could not prove equivalence).
    PossiblyNonEquivalent,
    /// The mutation is unreachable (the mutation point was never covered).
    Unreachable,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EquivalentReason {
    /// The semantic hash of the mutated function matches the original.
    SemanticHashIdentical,
    /// The mutated function is never called.
    DeadFunction,
    /// The mutated expression is pure and its result is discarded.
    PureVoidExpression,
    /// The mutation point was not covered by any test.
    NotCovered,
}

/// Analyzes mutations for equivalence.
pub struct EquivalentDetector {
    /// Per-function semantic hashes from the original HIR.
    original_hashes: HashMap<Symbol, ContentHash>,
    /// Per-function effect sets from the EffectAnalyzer.
    effects: HashMap<Symbol, EffectSet>,
    /// Coverage data from the initial test run (point_id → execution count).
    coverage: HashMap<usize, u64>,
    /// Functions that are called by any test or by main.
    reachable_functions: HashSet<Symbol>,
}

impl EquivalentDetector {
    pub fn new(/* ... */) -> Self { ... }

    /// Check whether a mutation is equivalent.
    pub fn check(&self, mutation: &Mutation, mutated_hash: ContentHash) -> EquivalentStatus {
        // Check 1: Semantic hash identity
        if let Some(original_hash) = self.original_hashes.get(&mutation.function_name) {
            if *original_hash == mutated_hash {
                return EquivalentStatus::Equivalent(
                    EquivalentReason::SemanticHashIdentical
                );
            }
        }

        // Check 2: Dead function
        if !self.reachable_functions.contains(&mutation.function_name) {
            return EquivalentStatus::Equivalent(EquivalentReason::DeadFunction);
        }

        // Check 3: Coverage unreachable
        if let Some(count) = self.coverage.get(&mutation.expr_id) {
            if *count == 0 {
                return EquivalentStatus::Unreachable;
            }
        }

        // Check 4: Pure expression in void position
        if let Some(effects) = self.effects.get(&mutation.function_name) {
            if effects.is_pure() && mutation.operator == MutationOperator::StatementDeletion {
                return EquivalentStatus::Equivalent(EquivalentReason::PureVoidExpression);
            }
        }

        EquivalentStatus::PossiblyNonEquivalent
    }
}
```

### 4.6 Mutation Score (`score.rs`)

The mutation score is the ratio of killed mutants to total non-equivalent mutants, expressed as a percentage:

```
mutation_score = killed / (total - equivalent - unreachable) * 100
```

The score is persisted in the incremental state directory alongside the test dependency graph and query results:

```rust
// crates/glyim-mutant/src/score.rs

use crate::{Mutation, MutationId, EquivalentStatus};
use serde::{Serialize, Deserialize};
use std::collections::HashMap;
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct MutationScoreReport {
    /// The source file that was mutation-tested.
    pub source_file: String,
    /// The semantic hash of the source at the time of testing.
    pub source_hash: String,
    /// Total number of mutations generated.
    pub total_mutations: usize,
    /// Mutations that were killed (test suite detected the change).
    pub killed: usize,
    /// Mutations that survived (test suite did not detect the change).
    pub survived: Vec<Mutation>,
    /// Mutations that were determined to be equivalent.
    pub equivalent: usize,
    /// Mutations that were unreachable (not covered by any test).
    pub unreachable: usize,
    /// Mutations that caused compilation errors (invalid mutation).
    pub compile_errors: usize,
    /// Mutations that caused test timeouts.
    pub timeouts: usize,
    /// The computed mutation score (0-100).
    pub score: f64,
    /// Per-function mutation scores.
    pub per_function: HashMap<String, FunctionMutationScore>,
    /// Timestamp of this report.
    pub timestamp: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionMutationScore {
    pub function_name: String,
    pub total: usize,
    pub killed: usize,
    pub equivalent: usize,
    pub score: f64,
}

impl MutationScoreReport {
    /// Load the report from the incremental state directory.
    pub fn load(dir: &Path) -> Option<Self> { ... }

    /// Save the report to the incremental state directory.
    pub fn save(&self, dir: &Path) -> Result<(), String> { ... }
}
```

---

## 5. Test Dependency Graph (Replacing Placeholder)

### 5.1 Real Implementation

The `TestDependencyGraph` in `glyim-testr/src/incremental.rs` is replaced with a real implementation that tracks which HIR items each test function depends on, enabling fine-grained test selection when source files change:

```rust
// crates/glyim-testr/src/incremental.rs (rewritten)

use crate::types::TestDef;
use glyim_query::{Fingerprint, DependencyGraph};
use glyim_interner::Symbol;
use std::collections::{HashMap, HashSet};
use serde::{Serialize, Deserialize};

/// Tracks which HIR items each test function depends on,
/// enabling incremental test selection.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestDependencyGraph {
    /// Maps each test name to the set of HIR item fingerprints it depends on.
    test_deps: HashMap<String, HashSet<Fingerprint>>,

    /// Maps each HIR item fingerprint to the source file it came from.
    item_sources: HashMap<Fingerprint, String>,

    /// Maps each HIR item fingerprint to the function name.
    item_names: HashMap<Fingerprint, Symbol>,

    /// Maps each source file to the set of HIR item fingerprints it defines.
    file_items: HashMap<String, HashSet<Fingerprint>>,
}

impl TestDependencyGraph {
    pub fn new() -> Self { ... }

    /// Record that a test depends on specific HIR items.
    /// Called during type checking when the test's call graph is resolved.
    pub fn add_test_dependency(
        &mut self,
        test_name: &str,
        item_fp: Fingerprint,
        source_file: &str,
        item_name: Symbol,
    ) { ... }

    /// Given a set of changed source files, compute which tests are affected.
    pub fn affected_tests(
        &self,
        changed_files: &HashSet<&str>,
        all_tests: &[TestDef],
    ) -> Vec<TestDef> {
        // Collect fingerprints of all items in changed files
        let affected_fps: HashSet<Fingerprint> = changed_files.iter()
            .flat_map(|file| self.file_items.get(*file).into_iter().flatten())
            .copied()
            .collect();

        // Propagate through the dependency graph (transitive)
        let transitive_fps = self.propagate_deps(&affected_fps);

        // Return tests whose dependencies intersect with affected items
        all_tests.iter()
            .filter(|test| {
                self.test_deps.get(&test.name)
                    .map(|deps| deps.iter().any(|d| transitive_fps.contains(d)))
                    .unwrap_or(true) // conservative: run test if deps unknown
            })
            .cloned()
            .collect()
    }

    /// Propagate affected fingerprints through the dependency graph.
    fn propagate_deps(&self, seeds: &HashSet<Fingerprint>) -> HashSet<Fingerprint> { ... }

    /// Load from the incremental state directory.
    pub fn load(dir: &std::path::Path) -> Option<Self> { ... }

    /// Save to the incremental state directory.
    pub fn save(&self, dir: &std::path::Path) -> Result<(), String> { ... }
}
```

The test dependency graph is populated during the `query_typeck` phase of the Phase 4 pipeline. When the type checker processes a test function (identified by the new `HirFn::is_test` field), it records the fingerprints of all HIR items that the test function depends on (all called functions, referenced types, etc.). This information flows from `glyim-hir/src/dependency_names.rs` through the query pipeline into the `TestDependencyGraph`.

---

## 6. Flake Tracker (Replacing Placeholder)

### 6.1 Real Implementation

The `FlakeTracker` in `glyim-testr/src/flaky.rs` is replaced with a statistical flake detector that uses the existing SQLite-backed test history from `glyim-testr/src/history.rs`:

```rust
// crates/glyim-testr/src/flaky.rs (rewritten)

use crate::history::test_runs;
use crate::types::{TestDef, TestOutcome};
use serde::{Serialize, Deserialize};
use std::collections::HashMap;

/// Statistical flake detector for test results.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlakeTracker {
    /// Minimum number of runs before flake detection is meaningful.
    min_runs: usize,
    /// A test is considered flaky if its pass rate is between these thresholds.
    flaky_low: f64,   // e.g., 0.8
    flaky_high: f64,  // e.g., 1.0

    /// Per-test statistics.
    stats: HashMap<String, FlakeStats>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlakeStats {
    pub total_runs: usize,
    pub passes: usize,
    pub fails: usize,
    pub pass_rate: f64,
    pub is_flaky: bool,
}

impl FlakeTracker {
    pub fn new() -> Self {
        Self {
            min_runs: 5,
            flaky_low: 0.8,
            flaky_high: 1.0,
            stats: HashMap::new(),
        }
    }

    /// Record a test result.
    pub fn record(&mut self, test_name: &str, outcome: &TestOutcome) {
        let stats = self.stats.entry(test_name.to_string()).or_default();
        stats.total_runs += 1;
        match outcome {
            TestOutcome::Passed | TestOutcome::FlakyPass { .. } => stats.passes += 1,
            _ => stats.fails += 1,
        }
        stats.pass_rate = stats.passes as f64 / stats.total_runs as f64;
        stats.is_flaky = stats.total_runs >= self.min_runs
            && stats.pass_rate >= self.flaky_low
            && stats.pass_rate < self.flaky_high;
    }

    /// Get the flake score for a test (0.0 = never flaky, 1.0 = always flaky).
    pub fn flake_score(&self, test_name: &str) -> f64 {
        self.stats.get(test_name)
            .map(|s| if s.is_flaky { 1.0 - s.pass_rate } else { 0.0 })
            .unwrap_or(0.0)
    }

    /// Get all tests that are currently considered flaky.
    pub fn flaky_tests(&self) -> Vec<&str> {
        self.stats.iter()
            .filter(|(_, s)| s.is_flaky)
            .map(|(name, _)| name.as_str())
            .collect()
    }
}
```

The flake tracker is used in mutation testing to handle test non-determinism. If a test is flaky, a mutant may appear to survive (the test passes even though the code changed) when in reality the test is non-deterministic. The mutation runner re-runs flaky tests multiple times and uses the majority outcome to determine killed/survived status.

---

## 7. Mutation Test Runner

### 7.1 Runner Architecture

The mutation test runner orchestrates the full mutation testing workflow:

```
1. Compile original source → CompiledHir
2. Run original test suite → collect coverage data
3. Analyze effects → EffectSet per function
4. Generate mutations → Vec<Mutation>
5. Filter equivalent mutants → EquivalentDetector
6. For each non-equivalent mutation:
   a. Apply mutation → MutatedHir
   b. Compile mutated function (incremental: reuse cached artifacts)
   c. Run test suite against mutant (JIT or subprocess)
   d. Record killed / survived
7. Compute mutation score
8. Persist mutation score report
```

```rust
// crates/glyim-mutant/src/runner.rs

use crate::{Mutation, MutationConfig, MutationEngine, MutationId, MutationScoreReport};
use crate::equivalent::EquivalentDetector;
use crate::score::MutationScoreReport;
use glyim_compiler::pipeline::{PipelineConfig, BuildMode};
use glyim_hir::Hir;
use glyim_merkle::MerkleStore;
use glyim_query::IncrementalState;
use std::sync::Arc;
use tokio::sync::Semaphore;

/// The mutation test runner.
pub struct MutationRunner {
    /// The original (unmutated) HIR.
    original_hir: Hir,
    /// The incremental state from Phase 4.
    incremental: IncrementalState,
    /// The Merkle store for artifact caching.
    merkle: Arc<MerkleStore>,
    /// The mutation engine.
    engine: MutationEngine,
    /// The equivalent mutant detector.
    equiv_detector: EquivalentDetector,
    /// The pipeline configuration.
    config: PipelineConfig,
    /// Mutation-specific configuration.
    mutation_config: MutationConfig,
    /// The mutation score report (accumulated during the run).
    report: MutationScoreReport,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MutantOutcome {
    /// The test suite detected the mutation (test failed).
    Killed,
    /// The test suite did not detect the mutation (all tests passed).
    Survived,
    /// The mutation caused a compilation error (invalid mutant).
    CompileError,
    /// The mutation caused a test timeout.
    Timeout,
    /// The mutation was determined to be equivalent.
    Equivalent,
    /// The mutation point was not covered by any test.
    Unreachable,
}

impl MutationRunner {
    pub fn new(/* ... */) -> Self { ... }

    /// Run the full mutation testing workflow.
    pub async fn run(&mut self) -> Result<MutationScoreReport, MutationError> {
        // Step 1: Run original tests with coverage to establish baseline
        let baseline = self.run_baseline_tests()?;

        // Step 2: Generate mutations from the original HIR
        let mutations = self.engine.generate_mutations(&self.original_hir)?;

        // Step 3: Filter equivalent mutants
        let mut non_equiv_mutations = Vec::new();
        for mutation in &mutations {
            let mutated_hir = self.apply_mutation(mutation)?;
            let status = self.equiv_detector.check(mutation, mutated_hir.mutated_fn_hash);
            match status {
                EquivalentStatus::Equivalent(_) => {
                    self.report.equivalent += 1;
                }
                EquivalentStatus::Unreachable => {
                    self.report.unreachable += 1;
                }
                EquivalentStatus::PossiblyNonEquivalent => {
                    non_equiv_mutations.push(mutation.clone());
                }
            }
        }

        // Step 4: Run tests against each mutant (with concurrency control)
        let semaphore = Arc::new(Semaphore::new(self.mutation_config.max_concurrent));
        let mut tasks = Vec::new();

        for mutation in non_equiv_mutations {
            let permit = semaphore.clone().acquire_owned().await.unwrap();
            let runner = self.clone_for_task();
            let handle = tokio::spawn(async move {
                let result = runner.run_mutant(&mutation).await;
                drop(permit);
                (mutation, result)
            });
            tasks.push(handle);
        }

        // Step 5: Collect results
        for handle in tasks {
            let (mutation, result) = handle.await.unwrap();
            match result {
                Ok(MutantOutcome::Killed) => self.report.killed += 1,
                Ok(MutantOutcome::Survived) => self.report.survived.push(mutation),
                Ok(MutantOutcome::CompileError) => self.report.compile_errors += 1,
                Ok(MutantOutcome::Timeout) => self.report.timeouts += 1,
                Err(e) => tracing::warn!("Mutation {:?} failed: {}", mutation.id, e),
            }
        }

        // Step 6: Compute score
        let denominator = self.report.total_mutations
            - self.report.equivalent
            - self.report.unreachable
            - self.report.compile_errors;
        self.report.score = if denominator > 0 {
            (self.report.killed as f64 / denominator as f64) * 100.0
        } else {
            100.0
        };

        Ok(self.report.clone())
    }

    /// Run tests against a single mutant.
    async fn run_mutant(&self, mutation: &Mutation) -> Result<MutantOutcome, MutationError> {
        // Apply mutation to produce MutatedHir
        let mutated = self.apply_mutation(mutation)?;

        // Compile only the mutated function (incremental)
        let compile_result = self.compile_mutant(&mutated);

        match compile_result {
            Err(_) => return Ok(MutantOutcome::CompileError),
            Ok(artifact) => {
                // Run test suite against the mutant
                let test_result = self.run_tests_against_mutant(&artifact).await?;
                match test_result {
                    TestSuiteResult::AllPassed => Ok(MutantOutcome::Survived),
                    TestSuiteResult::TestFailed(_) => Ok(MutantOutcome::Killed),
                    TestSuiteResult::Timeout => Ok(MutantOutcome::Timeout),
                }
            }
        }
    }
}
```

### 7.2 Incremental Mutant Compilation

The key performance optimization for mutation testing is reusing the per-function artifact cache from Phase 4. When a mutation changes one expression in one function, only that function needs to be recompiled. All other functions reuse their cached LLVM IR and object code from the `MerkleStore`.

The incremental compilation path for a mutant:

1. The `MutatedHir` contains a `mutated_fn_hash` that differs from the original function's hash.
2. The query pipeline checks the `MerkleStore` for cached artifacts keyed by `mutated_fn_hash`.
3. If this exact mutation has been compiled before (same semantic hash), the cached object code is reused — no recompilation needed.
4. If not cached, the pipeline recompiles only the mutated function (using `codegen_fn()` from Phase 4's per-function code generation).
5. The final binary is assembled from the mutated function's object code plus all unchanged functions' cached object code.

This reduces per-mutant compilation time from O(module_size) to O(mutated_function_size), a dramatic improvement for large modules with many small functions.

### 7.3 Test Execution Strategy

The mutation runner uses a two-tier test execution strategy:

**Tier 1: JIT execution (fast path)**. If the mutant's test functions can be resolved in the JIT engine (they are not `extern` functions, they do not require process isolation, and the coverage mode is `Off` or `Function`), the runner uses `run_jit_test()` to execute each test directly. This is approximately 10x faster than subprocess execution because it avoids the compile-link-execute cycle.

**Tier 2: Subprocess execution (safe path)**. If JIT execution is not possible (e.g., the mutant calls `extern` functions, requires coverage instrumentation at the `Full` level, or may crash), the runner falls back to the existing subprocess model: compile the mutant to a binary, inject the test harness, and spawn each test as a separate process with timeout handling.

The runner automatically selects the tier based on the mutation's characteristics:

```rust
fn select_execution_tier(&self, mutation: &Mutation) -> ExecutionTier {
    if self.mutation_config.prefer_jit
        && !self.mutated_fn_has_extern(mutation)
        && self.mutation_config.coverage_mode <= CoverageMode::Function
    {
        ExecutionTier::Jit
    } else {
        ExecutionTier::Subprocess
    }
}
```

---

## 8. Coverage Instrumentation Details

### 8.1 Instrumentation Points

Coverage instrumentation is applied at three levels of granularity, controlled by `CoverageMode`:

| Mode | Instrumentation Points | Overhead | Use Case |
|------|----------------------|----------|----------|
| `Off` | None | 0% | Production builds |
| `Function` | Function entry | ~2-5% | Quick coverage check; mutation reachability |
| `Branch` | Function entry + `if`/`match` conditions | ~5-15% | Branch coverage; mutation targeting |
| `Full` | Function entry + branch conditions + expression evaluations | ~15-30% | Detailed coverage analysis; line-level mapping |

### 8.2 Counter Implementation

Each instrumentation point gets a unique `point_id` (a monotonically increasing integer). The counter array `__glyim_cov_counts` is a global `i64` array indexed by `point_id`. At each instrumentation point, the code generator emits an atomic increment:

```llvm
; LLVM IR for incrementing counter at point_id 42:
%ptr = getelementptr inbounds [N x i64], [N x i64]* @__glyim_cov_counts, i64 0, i64 42
%old = load i64, i64* %ptr
%new = add i64 %old, 1
store i64 %new, i64* %ptr
```

The counter is non-atomic (no `cmpxchg`) for performance. This is safe because test execution is single-threaded (each test runs in its own process or JIT context).

### 8.3 Coverage Dump

After test execution, the counter array is dumped to a file via a runtime function registered with `atexit()` or called explicitly by the test harness:

```rust
// Runtime function to dump coverage data
#[no_mangle]
pub extern "C" fn __glyim_cov_dump(path: *const i8) {
    let path = unsafe { std::ffi::CStr::from_ptr(path) };
    let counts = unsafe { &__glyim_cov_counts };
    let mut output = std::fs::File::create(path.to_str().unwrap()).unwrap();
    for (i, &count) in counts.iter().enumerate() {
        writeln!(output, "{},{}", i, count).unwrap();
    }
}
```

### 8.4 Coverage Map

A coverage map (`coverage_map.json`) is generated during instrumentation that maps each `point_id` to its source location:

```json
{
  "0": {"file": "src/main.g", "line": 12, "col": 5, "kind": "fn_entry", "fn": "add"},
  "1": {"file": "src/main.g", "line": 14, "col": 8, "kind": "branch", "fn": "add"},
  "2": {"file": "src/main.g", "line": 17, "col": 3, "kind": "fn_entry", "fn": "main"}
}
```

This map is used by the mutation engine to correlate coverage data with mutation points, and by the `--coverage` CLI flag to produce human-readable coverage reports.

---

## 9. CLI Integration

### 9.1 New Flags

| Command | New Flag | Description |
|---------|----------|-------------|
| `glyim test` | `--mutate` | Run mutation testing instead of regular tests |
| `glyim test` | `--coverage` | Run tests with coverage instrumentation |
| `glyim test` | `--mutation-score <N>` | Fail if mutation score is below N% |
| `glyim test` | `--mutation-operators <list>` | Specify which mutation operators to use |
| `glyim test` | `--max-mutants <N>` | Limit the number of mutants to generate |
| `glyim test` | `--concurrent-mutants <N>` | Number of mutants to compile/test in parallel |
| `glyim test` | `--mutation-report <path>` | Output path for the mutation score report |
| `glyim test` | `--coverage-mode <mode>` | Set coverage granularity (function/branch/full) |

### 9.2 Command Implementation

```rust
// crates/glyim-cli/src/commands/cmd_test.rs (extended)

pub fn cmd_test(
    input: PathBuf,
    ignore: bool,
    filter: Option<String>,
    nocapture: bool,
    watch: bool,
    optimize_check: bool,
    mutate: bool,                     // NEW
    coverage: bool,                   // NEW
    mutation_score: Option<f64>,      // NEW
    mutation_operators: Option<String>, // NEW
    max_mutants: Option<usize>,       // NEW
    concurrent_mutants: Option<usize>, // NEW
    mutation_report: Option<PathBuf>, // NEW
    coverage_mode: Option<String>,    // NEW
) -> i32 {
    let mut config = TestConfig::default();
    config.filter = filter;
    config.include_ignored = ignore;
    config.nocapture = nocapture;
    config.watch = watch;
    config.optimize_check = optimize_check;

    if mutate {
        // Mutation testing mode
        let rt = tokio::runtime::Runtime::new().unwrap();
        let source = std::fs::read_to_string(&input).unwrap();
        let mutation_config = MutationConfig {
            operators: parse_operators(mutation_operators),
            skip_pure: true,
            skip_tests: true,
            max_mutations_per_fn: max_mutants.unwrap_or(50),
            detect_equivalents: true,
        };
        let runner_config = MutationRunnerConfig {
            max_concurrent: concurrent_mutants.unwrap_or(num_cpus::get()),
            prefer_jit: true,
            coverage_mode: if coverage {
                parse_coverage_mode(coverage_mode.as_deref())
            } else {
                CoverageMode::Function  // Default for mutation testing
            },
        };
        let mut runner = MutationRunner::new(source, mutation_config, runner_config);
        let report = rt.block_on(runner.run()).unwrap();

        // Print mutation score report
        eprintln!("{}", format_mutation_report(&report));

        // Save report to file if requested
        if let Some(path) = mutation_report {
            let json = serde_json::to_string_pretty(&report).unwrap();
            std::fs::write(&path, json).unwrap();
        }

        // Check mutation score threshold
        if let Some(threshold) = mutation_score {
            if report.score < threshold {
                eprintln!(
                    "Mutation score {:.1}% is below threshold {:.1}%",
                    report.score, threshold
                );
                return 1;
            }
        }

        if report.survived.is_empty() { 0 } else { 1 }
    } else if coverage {
        // Coverage mode (no mutation testing)
        config.coverage_mode = parse_coverage_mode(coverage_mode.as_deref());
        let source = std::fs::read_to_string(&input).unwrap();
        let rt = tokio::runtime::Runtime::new().unwrap();
        let results = rt.block_on(glyim_testr::run_tests_with_coverage(&source, &config));
        print_coverage_report(&results);
        let failed = results.iter()
            .filter(|r| matches!(r.outcome, TestOutcome::Failed { .. }))
            .count();
        if failed > 0 { 1 } else { 0 }
    } else {
        // Regular test mode (unchanged)
        // ... existing code ...
    }
}
```

### 9.3 CLI Subcommand Update

The CLI `main.rs` is updated to include the new flags:

```rust
Test {
    input: PathBuf,
    #[arg(long)] ignore: bool,
    #[arg(long)] filter: Option<String>,
    #[arg(long)] nocapture: bool,
    #[arg(long)] watch: bool,
    #[arg(long)] optimize: bool,
    // NEW FLAGS:
    #[arg(long)] mutate: bool,
    #[arg(long)] coverage: bool,
    #[arg(long)] mutation_score: Option<f64>,
    #[arg(long)] mutation_operators: Option<String>,
    #[arg(long)] max_mutants: Option<usize>,
    #[arg(long)] concurrent_mutants: Option<usize>,
    #[arg(long)] mutation_report: Option<PathBuf>,
    #[arg(long)] coverage_mode: Option<String>,
}
```

---

## 10. Mutation Report Format

### 10.1 Human-Readable Output

```
Mutation Testing Report for src/main.g
═══════════════════════════════════════
Total mutations:    47
  Killed:           38 (80.9%)
  Survived:          5 (10.6%)
  Equivalent:        2 (4.3%)
  Unreachable:       1 (2.1%)
  Compile errors:    1 (2.1%)
  Timeouts:          0 (0.0%)

Mutation Score: 88.4% (killed / non-equivalent)

Survived Mutants:
  1. add::[line 12] ArithmeticPlusToMinus: a + b → a - b
     Tests: test_add, test_add_negative
  2. factorial::[line 24] CompareLessToLessEqual: n < 2 → n <= 2
     Tests: test_factorial
  3. is_even::[line 31] BooleanNotElimination: !is_odd → is_odd
     Tests: test_is_even
  4. sort::[line 45] ConditionalFlip: if a > b → if !(a > b)
     Tests: test_sort
  5. main::[line 8] ConstantZero: count → 0
     Tests: (no covering tests)

Per-Function Mutation Scores:
  add:        75.0% (3/4 killed)
  factorial:  83.3% (5/6 killed)
  is_even:    66.7% (2/3 killed)
  sort:       90.0% (9/10 killed)
  main:       100.0% (4/4 killed)
```

### 10.2 JSON Report Format

The JSON report (saved by `--mutation-report`) contains the full mutation data for programmatic consumption:

```json
{
  "source_file": "src/main.g",
  "source_hash": "a3f2b8c1...",
  "total_mutations": 47,
  "killed": 38,
  "survived": [
    {
      "id": 12,
      "function_name": "add",
      "operator": "ArithmeticPlusToMinus",
      "original": "a + b",
      "replacement": "a - b",
      "line": 12,
      "covering_tests": ["test_add", "test_add_negative"]
    }
  ],
  "equivalent": 2,
  "unreachable": 1,
  "compile_errors": 1,
  "timeouts": 0,
  "score": 88.4,
  "per_function": {
    "add": {"total": 4, "killed": 3, "equivalent": 0, "score": 75.0},
    "factorial": {"total": 6, "killed": 5, "equivalent": 1, "score": 83.3}
  },
  "timestamp": "2026-05-07T14:30:00Z"
}
```

---

## 11. Effect Analysis Implementation

### 11.1 Bottom-Up Analysis

The `EffectAnalyzer` performs a bottom-up traversal of the call graph:

1. **Seed impure functions**: Register built-in impure functions (`print`, `read_line`, `Vec.push`, `Vec.pop`, `io.write`, etc.) as having `writes_mutable: true` and/or `performs_io: true`.
2. **Leaf functions**: Analyze leaf functions (functions that do not call any other user-defined functions) by walking their `HirExpr` trees and checking for mutable state access, I/O calls, and panic expressions.
3. **Non-leaf functions**: For each non-leaf function, compute the union of its direct effects and the transitive effects of all functions it calls. This is iterative because the call graph may have cycles (mutual recursion); the analysis converges when no effect set changes in an iteration.

### 11.2 Expression-Level Effects

The analyzer walks each function's `HirExpr` tree and classifies effects at the expression level:

| Expression | Effect |
|-----------|--------|
| `HirExpr::Call(name, args, _)` | Transitively impure if `name` refers to an impure function |
| `HirExpr::Assign(target, value, _)` | `writes_mutable: true` if `target` is a mutable variable |
| `HirExpr::MethodCall(receiver, method, args, _)` | Depends on method: `push`/`pop` → `writes_mutable`, `len` → pure |
| `HirExpr::Binary(op, _, _, _)` | Pure for all operators |
| `HirExpr::If(cond, then, else, _)` | Union of effects from all branches |
| `HirExpr::Match(scrutinee, arms, _)` | Union of effects from all arms |
| `HirExpr::Lit(_)` | Pure |
| `HirExpr::Var(name, _)` | `reads_mutable: true` if `name` is a mutable variable |

### 11.3 Integration with Mutation Engine

The effect analysis is used by the mutation engine in two ways:

1. **Equivalent mutant pruning**: Mutations to pure expressions in void positions are marked as equivalent by the `EquivalentDetector`.
2. **Mutation prioritization**: When the number of mutants exceeds `max_mutations_per_fn`, the engine prioritizes mutations in impure functions (which are more likely to produce observable behavior changes) over mutations in pure functions.

---

## 12. Error Handling & Recovery

### 12.1 Invalid Mutants

Some mutations produce invalid HIR (e.g., a type mismatch introduced by a mutation operator that was not type-aware). The mutation runner handles this gracefully:

1. If compilation of a mutant fails, record it as `MutantOutcome::CompileError`.
2. Skip all subsequent mutations that use the same operator at the same expression ID (they are likely also invalid).
3. Do not count compile errors against the mutation score denominator.

### 12.2 Test Timeouts During Mutation

If a mutation causes an infinite loop (e.g., `ConditionalFlip` on a loop condition), the test execution may hang. The runner uses the existing timeout mechanism from `glyim-testr/src/executor.rs`:

1. If JIT execution: spawn a thread with a timeout; if the thread does not return within the timeout, detach it and record `MutantOutcome::Timeout`.
2. If subprocess execution: use the existing `Executor::run_test()` with `timeout_secs`.

### 12.3 Out-of-Memory Mutants

Some mutations may cause excessive memory allocation (e.g., `ConstantBoundary` on a loop counter). The runner monitors memory usage via `/proc/self/status` (on Linux) or `task_info` (on macOS) and aborts mutants that exceed a configurable memory limit.

### 12.4 Crashed Mutants

If a mutant crashes (segfault, assertion failure), the subprocess executor detects the non-zero exit code and records the mutant as `MutantOutcome::Killed` — a crash is even stronger evidence that the test suite detects the mutation than a test failure.

---

## 13. Testing Strategy

### 13.1 Unit Tests

| Test | Location | Description |
|------|----------|-------------|
| `mutation_operator_arithmetic` | `glyim-mutant/tests/` | Arithmetic operators swap correctly |
| `mutation_operator_comparison` | `glyim-mutant/tests/` | Comparison operators mutate correctly |
| `mutation_operator_boolean` | `glyim-mutant/tests/` | Boolean operators flip correctly |
| `mutation_operator_constant` | `glyim-mutant/tests/` | Constant replacement works for all types |
| `mutation_apply_single_fn` | `glyim-mutant/tests/` | Applying a mutation changes exactly one function |
| `mutation_apply_preserves_others` | `glyim-mutant/tests/` | Non-mutated functions are identical to original |
| `equivalent_semantic_hash` | `glyim-mutant/tests/` | Equivalent mutants detected by hash identity |
| `equivalent_dead_code` | `glyim-mutant/tests/` | Mutations in dead code are flagged as equivalent |
| `equivalent_pure_void` | `glyim-mutant/tests/` | Pure expressions in void position are flagged |
| `effect_analyzer_pure` | `glyim-hir/tests/` | Pure functions are correctly identified |
| `effect_analyzer_impure` | `glyim-hir/tests/` | Impure functions have correct effect sets |
| `effect_analyzer_transitive` | `glyim-hir/tests/` | Transitive effects propagate through call graph |
| `test_dep_graph_affected` | `glyim-testr/tests/` | Changed files select correct subset of tests |
| `test_dep_graph_unchanged` | `glyim-testr/tests/` | No changes → no tests selected |
| `flake_tracker_score` | `glyim-testr/tests/` | Flake scores are computed correctly |
| `coverage_instrumentation_fn_entry` | `glyim-codegen-llvm/tests/` | Function entry counters are emitted |
| `coverage_instrumentation_branch` | `glyim-codegen-llvm/tests/` | Branch condition counters are emitted |

### 13.2 Integration Tests

| Test | Location | Description |
|------|----------|-------------|
| `mutation_full_workflow` | `glyim-cli-tests-full/` | End-to-end: mutate, run, score, report |
| `mutation_incremental_reuse` | `glyim-cli-tests-full/` | Mutant compilation reuses cached artifacts |
| `coverage_collection` | `glyim-cli-tests-full/` | Coverage data is collected during test runs |
| `jit_test_execution` | `glyim-cli-tests-full/` | JIT-based test execution works for simple tests |
| `mutation_score_threshold` | `glyim-cli-tests-full/` | `--mutation-score 80` fails when score is 75% |
| `mutation_report_json` | `glyim-cli-tests-full/` | JSON report is valid and contains all fields |
| `test_aware_hir_propagation` | `glyim-cli-tests-full/` | `#[test]` metadata survives HIR lowering |

### 13.3 Property Tests

| Test | Location | Description |
|------|----------|-------------|
| `mutation_preserves_type_safety` | `glyim-mutant/tests/` | Every mutant passes type checking |
| `mutation_reversibility` | `glyim-mutant/tests/` | Applying and then reversing a mutation recovers the original HIR |
| `score_monotonicity` | `glyim-mutant/tests/` | Adding a test never decreases the mutation score |
| `equivalent_subset_consistency` | `glyim-mutant/tests/` | Equivalent mutants are a subset of total mutants |

---

## 14. Implementation Timeline

### Phase 6A: HIR Test Metadata & Effect Analysis (4–6 days)

| Day | Task | Files |
|-----|------|-------|
| 1–2 | Add `is_test`, `test_config` to `HirFn`; propagate during lowering | `glyim-hir/src/lib.rs`, `glyim-hir/src/lower.rs` |
| 3–4 | Implement `EffectAnalyzer` with bottom-up call graph traversal | `glyim-hir/src/effects.rs` (new) |
| 5 | Implement known-impure function registration | `glyim-hir/src/effects.rs` |
| 6 | Unit tests for test metadata propagation and effect analysis | `glyim-hir/tests/` |

### Phase 6B: Coverage Instrumentation (5–7 days)

| Day | Task | Files |
|-----|------|-------|
| 1–2 | Add `CoverageMode` to `Codegen`; implement function-entry counters | `glyim-codegen-llvm/src/codegen/mod.rs` |
| 3–4 | Implement branch-condition counters | `glyim-codegen-llvm/src/codegen/mod.rs` |
| 5 | Implement coverage dump runtime and coverage map generation | `glyim-codegen-llvm/src/coverage.rs` (new) |
| 6–7 | Unit tests and integration tests for coverage | `glyim-codegen-llvm/tests/`, `glyim-cli-tests-full/` |

### Phase 6C: Mutation Engine Core (6–8 days)

| Day | Task | Files |
|-----|------|-------|
| 1–2 | Create `glyim-mutant` crate with `MutationEngine` and `MutationConfig` | `crates/glyim-mutant/` |
| 3–4 | Implement all mutation operators (`operators.rs`) | `glyim-mutant/src/operators.rs` |
| 5–6 | Implement mutation application (`apply.rs`) with HIR cloning and replacement | `glyim-mutant/src/apply.rs` |
| 7–8 | Implement equivalent mutant detection (`equivalent.rs`) | `glyim-mutant/src/equivalent.rs` |

### Phase 6D: Test Dependency Graph & Flake Tracker (3–4 days)

| Day | Task | Files |
|-----|------|-------|
| 1–2 | Rewrite `TestDependencyGraph` with real dependency tracking | `glyim-testr/src/incremental.rs` |
| 3 | Rewrite `FlakeTracker` with statistical flake detection | `glyim-testr/src/flaky.rs` |
| 4 | Unit tests for both | `glyim-testr/tests/` |

### Phase 6E: Mutation Test Runner & JIT Test Execution (6–8 days)

| Day | Task | Files |
|-----|------|-------|
| 1–2 | Implement `MutationRunner` with incremental mutant compilation | `glyim-mutant/src/runner.rs` |
| 3–4 | Implement `run_jit_test()` for per-test JIT execution | `glyim-compiler/src/pipeline.rs` |
| 5–6 | Implement two-tier test execution strategy (JIT + subprocess) | `glyim-mutant/src/runner.rs` |
| 7–8 | Implement mutation score computation and persistence | `glyim-mutant/src/score.rs` |

### Phase 6F: CLI Integration & Reporting (4–5 days)

| Day | Task | Files |
|-----|------|-------|
| 1–2 | Add `--mutate`, `--coverage`, `--mutation-score` flags | `glyim-cli/src/commands/cmd_test.rs`, `glyim-cli/src/main.rs` |
| 3 | Implement human-readable mutation report formatting | `glyim-mutant/src/report.rs` (new) |
| 4 | Implement JSON report output | `glyim-mutant/src/report.rs` |
| 5 | Final integration tests and documentation | All crates |

### Phase 6G: Testing & Hardening (2–4 days)

| Day | Task | Files |
|-----|------|-------|
| 1–2 | Property tests for mutation engine | `glyim-mutant/tests/` |
| 3 | End-to-end integration tests | `glyim-cli-tests-full/` |
| 4 | Performance benchmarks for mutation testing | `glyim-mutant/benches/` |

### Total: 30–42 working days

---

## 15. Crate Dependency Changes

### 15.1 New Crate

| Crate | Tier | Dependencies | Description |
|-------|------|-------------|-------------|
| `glyim-mutant` | 5 | `glyim-hir`, `glyim-interner`, `glyim-macro-vfs`, `glyim-compiler`, `glyim-query`, `glyim-merkle` | Mutation testing engine |

### 15.2 Modified Crates

| Crate | Changes |
|-------|---------|
| `glyim-hir` | `HirFn` gains `is_test: bool` and `test_config: Option<HirTestConfig>`; new `effects.rs` module with `EffectAnalyzer`, `EffectSet` |
| `glyim-codegen-llvm` | New `CoverageMode` enum; coverage instrumentation pass with counter injection; new `coverage.rs` module |
| `glyim-compiler` | New `run_jit_test()` function for per-test JIT execution; `CoverageMode` passed through pipeline config |
| `glyim-testr` | `TestDependencyGraph` rewritten with real dependency tracking; `FlakeTracker` rewritten with statistical detection; `TestConfig` gains `coverage_mode`, `mutation_mode` fields; new `run_tests_with_coverage()` API |
| `glyim-cli` | New `--mutate`, `--coverage`, `--mutation-score`, `--mutation-operators`, `--max-mutants`, `--concurrent-mutants`, `--mutation-report`, `--coverage-mode` flags |

### 15.3 Workspace Cargo.toml Update

```toml
[workspace]
members = [
    # ... existing members ...
    "crates/glyim-mutant",
]
```

### 15.4 Tier Assignment

```
Tier 1: glyim-interner, glyim-diag, glyim-syntax
Tier 2: glyim-lex, glyim-parse
Tier 3: glyim-hir, glyim-typeck, glyim-macro-core, glyim-macro-vfs, glyim-egraph
Tier 4: glyim-codegen-llvm
Tier 5: glyim-cli, glyim-cas-server, glyim-watch, glyim-orchestrator, glyim-mutant
```

`glyim-mutant` is tier 5 because it depends on `glyim-compiler` (tier 5). No tier violations are introduced.

---

## 16. Risk Register

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| Mutation operators produce invalid HIR | Medium | Medium | Type-aware operators; compile-error detection; skip invalid mutants |
| Equivalent mutant detection is too aggressive | Medium | High | Conservative heuristics only; report equivalent mutants separately; allow override |
| Mutation testing is too slow for large projects | High | Critical | JIT execution; incremental compilation; concurrent mutant testing; max_mutants limit |
| Coverage instrumentation slows down test execution | Medium | Low | Configurable granularity (Function/Branch/Full); Off mode for production |
| HIR test metadata breaks existing code | Low | Medium | Optional fields with defaults; backward-compatible serialization |
| Effect analysis is inaccurate | Medium | Medium | Conservative (over-approximate impurity); mark uncertain functions as impure |
| JIT test execution crashes corrupt runner state | Medium | High | Process isolation as fallback; memory monitoring; timeout enforcement |
| Mutation score is misinterpreted as test quality metric | Low | Low | Documentation explaining that mutation score is one metric among many |
| Large number of mutants overwhelms incremental cache | Medium | Medium | GC of MerkleStore after mutation run; limit cache size |
| Cross-platform coverage dump (Linux vs macOS) | Low | Medium | Platform-specific coverage dump implementations; CI testing on both |

---

## 17. Performance Targets

| Metric | Target | Measurement |
|--------|--------|-------------|
| Single mutation compilation (incremental) | < 50ms | Time to compile one mutated function, reusing cached artifacts |
| JIT test execution (one test) | < 10ms | Time from `run_jit_test()` call to result |
| Subprocess test execution (one test) | < 500ms | Time from process spawn to result |
| Mutation testing (50 mutants, JIT) | < 30s | End-to-end mutation testing with 50 mutants and JIT execution |
| Mutation testing (50 mutants, subprocess) | < 5min | End-to-end mutation testing with 50 mutants and subprocess execution |
| Coverage-instrumented build | < 2x overhead | Compilation time with Function coverage vs. no coverage |
| Coverage-instrumented test execution | < 1.5x overhead | Test execution time with Function coverage vs. no coverage |
| Effect analysis (100 functions) | < 100ms | Time to analyze all effects in a 100-function HIR |
| Test dependency graph query | < 1ms | Time to compute affected tests given a set of changed files |
| Equivalent mutant detection (50 mutants) | < 50ms | Time to classify 50 mutants as equivalent or not |

---

## 18. Migration Strategy

### 18.1 Backward Compatibility

All new fields on `HirFn` (`is_test`, `test_config`) use defaults that preserve existing behavior: `is_test: false`, `test_config: None`. Serialized HIR from previous versions (without these fields) is loaded correctly because the fields are marked with `#[serde(default)]`.

The `CoverageMode` defaults to `Off`, so existing builds produce no instrumentation overhead. The mutation testing flags (`--mutate`, `--coverage`) are opt-in and have no effect on regular builds or test runs.

### 18.2 Gradual Feature Rollout

1. **Phase 6A–6B:** HIR test metadata and coverage instrumentation are available internally but not exposed in the CLI. Testing happens via unit tests.
2. **Phase 6C–6D:** Mutation engine and test infrastructure are available in the `glyim-mutant` crate. Internal testing via integration tests.
3. **Phase 6E:** JIT test execution is available via `run_jit_test()`. Not yet wired to the CLI.
4. **Phase 6F:** `--mutate`, `--coverage`, and `--mutation-score` flags are activated. Mutation testing is now user-facing.
5. **Phase 6G:** Full testing, performance optimization, and documentation.

### 18.3 Feature Flags

A `mutation-testing` feature flag gates the `glyim-mutant` crate dependency in `glyim-cli`. When the feature is off (the default for initial rollout), the `--mutate` flag is not available. This allows the mutation testing code to be merged without affecting users who do not need it. The feature flag is removed in a subsequent release once mutation testing is stable.

---

## 19. Success Criteria

Phase 6 is complete when all of the following are true:

1. `HirFn::is_test` is populated during HIR lowering for all `#[test]` functions, and `test_config` correctly records `should_panic`, `ignored`, and `tags` attributes
2. `--coverage` produces a coverage report showing which functions and branches were executed during test runs
3. `--mutate` generates mutations, compiles each mutant incrementally, runs the test suite against each, and computes a mutation score
4. The mutation score is the ratio of killed mutants to total non-equivalent non-unreachable mutants, and `--mutation-score 80` fails when the score is below 80%
5. Incremental mutant compilation reuses cached artifacts for unchanged functions (verified by `IncrementalReport` showing green items for non-mutated functions)
6. JIT test execution works for simple test functions (no externs, no crashes)
7. The `TestDependencyGraph` correctly identifies affected tests when source files change (placeholder is fully replaced)
8. The `FlakeTracker` correctly identifies flaky tests from test history (placeholder is fully replaced)
9. Effect analysis classifies built-in functions (`print`, `Vec.push`, etc.) as impure and simple arithmetic functions as pure
10. Equivalent mutant detection identifies at least dead-code mutations and semantic-hash-identical mutations as equivalent
11. All property tests (`mutation_preserves_type_safety`, `mutation_reversibility`, `score_monotonicity`, `equivalent_subset_consistency`) pass
12. Coverage instrumentation overhead is less than 2x for Function mode and less than 1.5x for test execution
