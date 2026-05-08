# Glyim Incremental Compiler

## Phase 3 Implementation Plan

### E-Graph Middle-End & Algebraic Optimization

**Codebase:** elcoosp-glyim v0.5.0  
**Rust Workspace** | 20 Crates | LLVM 22.1 / Inkwell 0.9  
**Date:** 2026-05-07

---

## 1. Executive Summary

This document provides a fully-fledged implementation plan for Phase 3 of the Glyim Incremental Compiler project: the E-Graph Middle-End and Algebraic Optimization layer. Phase 3 sits at the critical juncture between the compiler's typed, monomorphized High-Level Intermediate Representation (HIR) and the LLVM code generation backend. By introducing an e-graph-based optimization pass, we unlock the ability to perform algebraic simplifications, prove semantic equivalences between expressions, cache optimization invariants, and provide the foundational machinery for equivalent mutant pruning in Phase 7.

The plan is grounded in a thorough analysis of the existing codebase, including the 20-crate Rust workspace, the existing query engine (`glyim-query`), the Merkle IR tree (`glyim-merkle`), the semantic normalization pass (`glyim-hir/src/normalize.rs`), and the type checker (`glyim-typeck`). Every design decision references concrete existing code, types, and integration points, ensuring that the plan can be executed without architectural surprises.

Phase 3 introduces two new crates (`glyim-egraph` and `glyim-effects`), extends the HIR crate with an effect analysis module, modifies the compiler pipeline to insert the e-graph optimization pass between monomorphization and LLVM lowering, and wires invariant certificates into the existing Merkle and query infrastructure. The total estimated effort is 28-37 working days, producing a compiler that can prove algebraic identities, skip redundant LLVM optimization passes, annotate pure functions with LLVM `readonly` attributes, and serve as the semantic equivalence engine for mutation testing.

## 2. Current Codebase State Assessment

### 2.1 Compilation Pipeline (As-Is)

The current Glyim compiler (v0.5.0) follows a linear pipeline architecture. Source code enters the pipeline, passes through macro expansion (via wasmtime-driven Wasm procedural macros), is parsed into a rowan-based Concrete Syntax Tree (CST), and then lowered into a High-Level Intermediate Representation (HIR). The HIR undergoes type checking, method desugaring, and monomorphization before being handed directly to the LLVM code generator. The code generator produces an inkwell Module, which is then compiled to object code and linked into a final binary. There is no optimization middle-end between the HIR and LLVM lowering; the compiler relies entirely on LLVM's built-in optimization passes.

The pipeline flow is orchestrated in `glyim-compiler/src/pipeline.rs`, specifically in the `compile_source_to_hir()` function. This function sequentially calls macro expansion, parsing, declaration table construction, HIR lowering, type checking, method desugaring, and monomorphization. The resulting `MonoResult` (containing `mono_hir` and `merged_types`) is passed directly to `Codegen::generate()` in `glyim-codegen-llvm/src/codegen/mod.rs`. The absence of a middle-end optimization layer means that every algebraic identity (such as `x + 0 = x` or `x * 2 = x << 1`) must be caught by LLVM's optimization passes, which operate at a much lower level and cannot leverage Glyim's type and effect information.

### 2.2 Existing Infrastructure Relevant to Phase 3

#### 2.2.1 HIR Types and Expression Tree

The HIR expression tree (defined in `glyim-hir/src/node/mod.rs`) is the primary data structure that the e-graph optimization pass will consume. Every `HirExpr` variant carries an `ExprId`, which uniquely identifies the expression node. This is critical for the e-graph because `ExprId` provides a stable identity that can be used to map e-class representatives back to specific HIR nodes during extraction. The `HirBinOp` enum defines the binary operators (`Add, Sub, Mul, Div, Mod, Eq, Neq, Lt, Gt, Lte, Gte, And, Or`) that are the primary targets for algebraic rewrites. The `HirUnOp` enum (`Neg, Not`) supports unary simplifications like double-negation elimination.

The `HirFn` struct represents a top-level function definition, containing the function name (`Symbol`), type parameters, parameter types and mutability, return type, body expression, and metadata flags (`is_pub, is_macro_generated, is_extern_backed`). The `Hir` struct contains a `Vec<HirItem>`, where `HirItem` is an enum over `Fn, Struct, Enum, Impl, and Extern` variants. This is the top-level container that the e-graph pass will iterate over.

#### 2.2.2 Semantic Normalization and Hashing

The normalize module (`glyim-hir/src/normalize.rs`) already implements a `SemanticNormalizer` that converts `HirExpr` into `NormalizedExpr`, a structurally equivalent but semantically canonical form. The normalizer performs variable renaming (symbolic names to numeric local IDs), commutative operand sorting (for `Add, Mul, Eq, Neq, And, Or`), and canonical string resolution. The `NormalizedExpr` enum derives `Hash, Eq, PartialOrd, and Ord`, making it suitable for use as a hash-consable representation in the e-graph. The `is_commutative()` method on `HirBinOp` already identifies which operators support commutative reordering, which directly informs which e-graph rewrite rules can apply.

The semantic_hash module (`glyim-hir/src/semantic_hash.rs`) provides `SemanticHash`, a SHA-256-based content hash computed over the normalized form. This hash is stable across variable renames and comment changes, and is already integrated into the query engine for cache invalidation. The `SemanticHash::combine()` function allows hierarchical hashing, which will be used to compute e-class canonical form hashes for invariant certificates.

#### 2.2.3 Type Checker Output

The `TypeChecker` (`glyim-typeck/src/typeck/mod.rs`) produces two critical outputs that the e-graph needs: `expr_types` (a `Vec<HirType>` indexed by `ExprId`) and `call_type_args` (a `HashMap<ExprId, Vec<HirType>>` mapping call-site expressions to their concrete type arguments). The `expr_types` vector is essential for the e-graph because rewrite rules must be type-constrained: integer-only optimizations (such as strength reduction for multiplication by powers of 2) must not be applied to floating-point expressions, and boolean simplifications (like and-true elimination) must only apply to boolean-typed expressions. The `call_type_args` map enables the e-graph to resolve generic function calls to their monomorphized concrete types, which is necessary for interprocedural optimization decisions.

#### 2.2.4 Query Engine and Merkle Store

The `glyim-query` crate provides a demand-driven, memoized query engine with fingerprint-based cache keys, dependency tracking (via petgraph), and red/green invalidation. The `QueryContext` stores results in a `DashMap<Fingerprint, QueryResult>` and supports atomic invalidation when input dependencies change. The `IncrementalState` persists query results between builds, enabling incremental compilation. The e-graph pass will be integrated as a query (`optimize_egraph`) in this system, so that unchanged functions skip the e-graph optimization entirely.

The `glyim-merkle` crate provides a content-addressed Merkle DAG backed by the existing CAS infrastructure (`LocalContentStore` and `RemoteContentStore` from `glyim-macro-vfs`). The `MerkleStore` caches `MerkleNode` objects in a DashMap and falls back to CAS retrieval for cache misses. This is where invariant certificates will be stored: each certificate is a `MerkleNode` whose hash depends on the function's canonical form hash, and whose children are the hashes of the optimization invariants that produced the optimized output.

#### 2.2.5 Monomorphized HIR as the E-Graph Input

The monomorphized HIR (`mono_hir`) produced by `glyim-hir/src/monomorphize/` is the ideal input for the e-graph optimization pass. By the time monomorphization is complete, all generic type parameters have been substituted with concrete types, all method calls have been resolved to specific function implementations, and all trait-like dispatch has been eliminated. This means the e-graph operates on a fully concrete, unambiguous representation where every expression has a known type and every function call has a known target. This eliminates the need for the e-graph to reason about type variables or generic constraints, dramatically simplifying the rewrite rule set.

### 2.3 Critical Gaps That Phase 3 Addresses

| Gap | Impact | Affected Crate | Phase 3 Solution |
|-----|--------|----------------|------------------|
| No algebraic optimization | Missed simplifications (x+0, x*1, x*2<<1) that LLVM may not catch at Glyim's type level | glyim-compiler | glyim-egraph: egg-based equality saturation engine |
| No semantic equivalence proofs | Cannot prune equivalent mutants in Phase 7 | (missing) | glyim-egraph: `are_equivalent()` via e-class membership |
| No effect system | Cannot prove purity for CSE/LICM/parallel safety | glyim-hir, glyim-typeck | glyim-hir/src/effects.rs: bottom-up purity analysis |
| No optimization invariant caching | Every build re-optimizes everything even when invariants are unchanged | glyim-compiler | InvariantCertificate in glyim-egraph, wired to MerkleStore |
| No LLVM attribute hints from effects | Pure functions lack readonly/noalias, missing optimization opportunities | glyim-codegen-llvm | Effect info drives LLVM function attribute annotations |

## 3. Architecture Design

### 3.1 Phase 3 Pipeline Insertion Point

The e-graph optimization pass inserts into the compilation pipeline between monomorphization and LLVM code generation. This is the natural insertion point because the input must be fully typed and monomorphized (so rewrite rules can be type-constrained), and the output must be a valid HIR that the existing `Codegen` struct can consume without modification. The insertion point is in `glyim-compiler/src/pipeline.rs`, specifically in the `compile_source_to_hir()` function (and its incremental counterpart `compile_source_to_hir_incremental()`).

The current pipeline flow after monomorphization is: `merge_mono_types()` produces `(merged_types, mono_hir)`, and then `Codegen::generate(&mono_hir)` is called. With Phase 3, this becomes: `merge_mono_types()` produces `(merged_types, mono_hir)`, then the e-graph optimization pass transforms `mono_hir` into `optimized_hir`, and finally `Codegen::generate(&optimized_hir)` is called. The `Codegen` struct does not need any changes to accept the optimized HIR because the e-graph pass produces structurally identical HIR (just with algebraically simplified expressions).

#### 3.1.1 Pipeline Modification (Pseudocode)

```rust
// In glyim-compiler/src/pipeline.rs

// BEFORE (current):
let (merged_types, mono_hir) = merge_mono_types(...);
codegen.generate(&mono_hir)?;

// AFTER (Phase 3):
let (merged_types, mono_hir) = merge_mono_types(...);
let optimized_hir = if cfg.feature("egraph-opt") {
    let effects = glyim_effects::analyze(&mono_hir, &interner);
    glyim_egraph::optimize(&mono_hir, &merged_types, &effects, &interner)
} else { mono_hir.clone() };
codegen.generate(&optimized_hir)?;
```

### 3.2 Crate Dependency Graph

Phase 3 introduces two new crates and extends one existing crate. The dependency relationships must respect the workspace tier structure: `glyim-effects` (tier 3, depends on `glyim-hir` and `glyim-interner`) and `glyim-egraph` (tier 3, depends on `glyim-hir`, `glyim-interner`, and `egg`). Neither new crate depends on `glyim-codegen-llvm` (tier 4) or `glyim-compiler` (tier 5), ensuring no tier violations. The effect analysis module could also live as a new file in `glyim-hir` (`glyim-hir/src/effects.rs`) since it only depends on HIR types and the interner, matching the pattern of `normalize.rs` and `semantic_hash.rs` which are already in `glyim-hir`.

### 3.3 Data Flow Through the E-Graph Pass

The e-graph optimization pass follows a five-stage data flow. First, the `MonoResult` (`mono_hir` + `merged_types`) and effect analysis results are received from the pipeline. Second, each `HirFn` in the `mono_hir` is converted into an e-graph representation by walking the `HirExpr` tree and adding each node as an e-node in the e-graph. Third, equality saturation is run by applying a set of algebraic rewrite rules for a configurable number of iterations or until a time budget is exhausted. Fourth, the best expression is extracted from each function's e-graph using a cost function that prefers fewer operations, prefers shifts over multiplications, and prefers constants over variable references. Fifth, the extracted expression is converted back into a `HirExpr` tree, producing an optimized `HirFn` that replaces the original in the output HIR.

Each stage is designed to be independently cacheable via the query engine. The conversion from `HirExpr` to e-graph can be cached by the function's semantic hash. The equality saturation result can be cached by the function's semantic hash plus the rule set version. The extraction result can be cached by the e-graph state plus the cost function parameters. This multi-level caching ensures that functions whose semantics have not changed are never re-optimized, even when other functions in the same file are modified.

## 4. glyim-egraph Crate Specification

### 4.1 Crate Structure

```
crates/glyim-egraph/
├── Cargo.toml
└── src/
    ├── lib.rs           — public API, re-exports
    ├── lang.rs          — GlyimExpr e-graph language definition
    ├── convert.rs       — HirExpr ↔ GlyimExpr conversion
    ├── rules.rs         — algebraic rewrite rules
    ├── analysis.rs      — GlyimAnalysis (constant folding, purity, cost)
    ├── extract.rs       — cost function and best-expression extraction
    ├── optimize.rs      — top-level optimize() entry point
    ├── invariant.rs     — InvariantCertificate computation and caching
    ├── equivalence.rs   — are_equivalent() for mutant pruning
    └── tests/
        ├── mod.rs
        ├── lang_tests.rs
        ├── rules_tests.rs
        ├── convert_tests.rs
        ├── optimize_tests.rs
        ├── invariant_tests.rs
        └── equivalence_tests.rs
```

### 4.2 Cargo.toml

```toml
[package]
name = "glyim-egraph"
version.workspace = true
edition.workspace = true
license.workspace = true
rust-version.workspace = true
description = "E-graph based algebraic optimization for Glyim"

[dependencies]
egg = "0.6"
glyim-hir = { path = "../glyim-hir" }
glyim-interner = { path = "../glyim-interner" }
glyim-effects = { path = "../glyim-effects" }
serde = { version = "1", features = ["derive"] }
sha2 = "0.11"
tracing = "0.1"

[dev-dependencies]
glyim-typeck = { path = "../glyim-typeck" }
glyim-parse = { path = "../glyim-parse" }
```

### 4.3 GlyimExpr Language Definition (`lang.rs`)

The `GlyimExpr` language defines how Glyim HIR expressions are represented as e-nodes in the egg e-graph. The language must implement egg's `Language` trait, which requires defining the children (sub-expressions) of each node and a display representation. The design uses a flat, tagged-union representation where each variant corresponds to a `HirExpr` variant, but with `Symbol` references replaced by string names and `ExprId` annotations stripped (since e-graphs assign their own IDs via e-class membership).

The key insight is that the `GlyimExpr` language must be rich enough to represent all `HirExpr` variants that are optimization-relevant, but does not need to represent statements or patterns (since the e-graph optimizes expressions within a single function body). The `convert.rs` module handles the bidirectional mapping between `HirExpr` and `GlyimExpr`, preserving type information through an external type map that associates each e-class ID with its `HirType`.

| GlyimExpr Variant | HirExpr Source | Optimization Relevance |
|---|---|---|
| `Num(i64)` | `IntLit` | Constant folding, identity elimination |
| `FNum(u64)` | `FloatLit` | Constant folding (ordered bits for hashing) |
| `BoolLit(bool)` | `BoolLit` | Boolean simplification (and-true, or-false) |
| `StrLit(Symbol)` | `StrLit` | Not optimized (opaque) |
| `Var(Symbol)` | `Ident` | Variable reference, renaming canonicalization |
| `BinOp(HirBinOp, Id, Id)` | `Binary` | Core algebraic rewrites |
| `UnOp(HirUnOp, Id)` | `Unary` | Double-negation, not-true/false |
| `Call(Symbol, Vec<Id>)` | `Call` | Inline candidate identification, pure call CSE |
| `If(Id, Id, Id)` | `If` | Branch simplification, constant condition elimination |
| `MethodCall(Symbol, Id, Vec<Id>)` | `MethodCall` | Resolved callee inlining hints |
| `FieldAccess(Id, Symbol)` | `FieldAccess` | Not optimized directly |
| `StructLit(Symbol, Vec<(Symbol, Id)>)` | `StructLit` | Not optimized directly |
| `EnumVariant(Symbol, Symbol, Vec<Id>)` | `EnumVariant` | Not optimized directly |

### 4.4 HIR to E-Graph Conversion (`convert.rs`)

The conversion module implements bidirectional translation between `HirExpr` and the egg e-graph representation. The `hir_to_egraph()` function walks the `HirExpr` tree recursively, creating e-nodes for each expression and returning the e-class ID of the root. The `egraph_to_hir()` function performs the reverse mapping, taking an e-class ID and extracting the best expression back into a `HirExpr` tree. The conversion must preserve type information through an external type map (`HashMap<Id, HirType>`) because the `GlyimExpr` language does not embed types directly into the e-node representation.

A critical design decision is how to handle statements. The egg e-graph operates on pure expressions, but `HirExpr::Block` contains `HirStmt` nodes that include let-bindings, assignments, and expression statements. The conversion strategy is to treat each `HirFn`'s body as a single expression rooted at the Block, and to represent let-bindings as let-expressions within the e-graph. The e-graph does not reorder statements or eliminate let-bindings (that is left to LLVM's mem2reg and DCE passes), but it does optimize the right-hand-side expressions of let-bindings and the conditions of if-expressions.

### 4.5 Core Rewrite Rules (`rules.rs`)

The rewrite rules are the heart of the e-graph optimizer. Each rule is an `egg::Rewrite` that transforms a pattern-matched expression into an equivalent but cheaper form. The rules are organized into five categories: identity elimination, strength reduction, commutativity/associativity, boolean simplification, and constant folding. Each rule includes a condition function that checks type constraints (e.g., integer-only rules must verify that the expression type is `HirType::Int`).

#### 4.5.1 Identity Elimination Rules

| Rule Name | Pattern | Replacement | Type Constraint |
|---|---|---|---|
| add-zero | `(+ a 0)` | `a` | Int or Float |
| zero-add | `(+ 0 a)` | `a` | Int or Float |
| sub-zero | `(- a 0)` | `a` | Int or Float |
| mul-one | `(* a 1)` | `a` | Int or Float |
| one-mul | `(* 1 a)` | `a` | Int or Float |
| div-one | `(/ a 1)` | `a` | Int or Float |
| and-true | `(&& a true)` | `a` | Bool |
| true-and | `(&& true a)` | `a` | Bool |
| or-false | `(\|\| a false)` | `a` | Bool |
| false-or | `(\|\| false a)` | `a` | Bool |

#### 4.5.2 Strength Reduction Rules

| Rule Name | Pattern | Replacement | Type Constraint |
|---|---|---|---|
| mul-by-2 | `(* a 2)` | `(<< a 1)` | Int only |
| mul-by-4 | `(* a 4)` | `(<< a 2)` | Int only |
| mul-by-8 | `(* a 8)` | `(<< a 3)` | Int only |
| div-by-2 | `(/ a 2)` | `(>> a 1)` | Int only (unsigned) |
| mod-power-2 | `(% a ?n)` | `(& a (- ?n 1))` | Int only, n is power of 2 |

#### 4.5.3 Commutativity and Associativity Rules

Commutativity rules enable the e-graph to discover that `(a + b)` and `(b + a)` are equivalent, which is necessary for proving that reordering operands does not change semantics. Associativity rules allow the e-graph to regroup operations, enabling optimizations like `((a + b) + c)` becoming `(a + (b + c))` when `b + c` is a constant that can be folded. These rules are critical for constant propagation through chains of operations.

#### 4.5.4 Boolean and Logical Simplification Rules

| Rule Name | Pattern | Replacement | Notes |
|---|---|---|---|
| double-neg | `(- (- a))` | `a` | Integer double negation |
| not-not | `(! (! a))` | `a` | Boolean double negation |
| not-eq | `(! (== a b))` | `(!= a b)` | Negated equality |
| not-neq | `(! (!= a b))` | `(== a b)` | Negated inequality |
| and-false | `(&& a false)` | `false` | Short circuit |
| or-true | `(\|\| a true)` | `true` | Short circuit |
| implies-to-or | `(!a \|\| b)` | Logical implication | If a then b |

#### 4.5.5 Constant Folding (Analysis-Driven)

Constant folding is implemented through the egg analysis framework rather than as explicit rewrite rules. The `GlyimAnalysis` struct (defined in `analysis.rs`) implements `egg::Analysis` and maintains a `constant_value` field for each e-class. When both operands of a binary operation have known constant values, the analysis computes the result at saturation time and stores it in the e-class data. The extraction phase then prefers the constant value over the original expression. This approach is more efficient than writing separate rules for every possible constant pair (e.g., `(+ 3 5) => 8`), because it handles arbitrary constants without rule explosion.

### 4.6 Analysis Data (`analysis.rs`)

The `GlyimAnalysis` struct implements `egg::Analysis` and stores per-e-class metadata that guides both rewrite rule application and expression extraction. The analysis data includes the constant value (if determinable), the purity flag (whether the expression has no side effects), the estimated computational cost, and the type of the expression. The analysis is recomputed during each iteration of equality saturation, ensuring that constant propagation and purity inference are always up-to-date.

```rust
pub struct GlyimAnalysis {
    pub constant: Option<ConstValue>,
    pub is_pure: bool,
    pub cost: f64,
    pub ty: Option<HirType>,
}

pub enum ConstValue {
    Int(i64),
    Float(u64), // bits, for ordered comparison
    Bool(bool),
}
```

### 4.7 Cost Function and Extraction (`extract.rs`)

The cost function determines which expression is selected from each e-class after equality saturation completes. The default cost function (`AstSizeCostFn`) assigns a cost of 1.0 to each node, preferring expressions with fewer total operations. Enhanced cost functions can assign different costs to different operations: for example, multiplication costs 3.0 while a left-shift costs 1.0, ensuring that strength-reduced forms are preferred. The extraction process walks the e-graph from the root e-class, selecting the cheapest e-node at each step, and produces a `GlyimExpr` tree that is then converted back to `HirExpr`.

### 4.8 Optimization Entry Point (`optimize.rs`)

The top-level `optimize()` function orchestrates the entire e-graph optimization pass for a complete HIR module. It iterates over each `HirItem::Fn` in the input HIR, applies the e-graph optimization to the function body, and collects the optimized functions into a new HIR. The function respects the query engine's caching: if a function's `InvariantCertificate` matches a cached version, the optimization is skipped entirely. The function also respects configurable limits: a maximum node count (to prevent e-graph explosion on very large functions), a time budget per function (defaulting to 50ms), and a maximum number of saturation iterations (defaulting to 10).

```rust
pub fn optimize(
    hir: &Hir,
    types: &[HirType],
    effects: &EffectAnalysis,
    interner: &Interner,
) -> Hir {
    let config = OptimizeConfig::default();
    optimize_with_config(hir, types, effects, interner, &config)
}

pub struct OptimizeConfig {
    pub node_limit: usize,           // default: 50_000
    pub time_limit: Duration,        // default: 50ms per function
    pub iter_limit: usize,           // default: 10
    pub rules: Vec<Rewrite<GlyimExpr, GlyimAnalysis>>,
}
```

### 4.9 Invariant Certificates (`invariant.rs`)

The `InvariantCertificate` is the key mechanism for skipping redundant optimization passes. After the e-graph optimization produces an optimized function, the certificate summarizes the optimization-relevant properties of that function. If a subsequent build produces the same certificate, the optimizer's output is guaranteed to be identical, so the optimization can be skipped entirely. The certificate is stored in the `MerkleStore`, keyed by the function's semantic hash, and its children are the hashes of the input invariants that contributed to the certificate.

```rust
#[derive(Serialize, Deserialize, Hash, PartialEq, Eq, Clone)]
pub struct InvariantCertificate {
    pub signature_hash: ContentHash,
    pub is_pure: bool,
    pub may_panic: bool,
    pub may_allocate: bool,
    pub complexity: u32,
    pub ir_size: u32,
    pub callees: Vec<Symbol>,
    pub canonical_form_hash: ContentHash,
    pub rule_set_version: u32,
}
```

The certificate matching logic is straightforward: compute the certificate for the new HIR function, look up the cached certificate in the `MerkleStore` by the function's semantic hash, and if they match, return the cached optimized HIR directly. If they differ (because the function changed, its callees changed, or the rule set was updated), re-run the e-graph optimization and store the new certificate. This mechanism ensures that the e-graph pass is only executed for functions that actually need optimization, making incremental builds with the e-graph pass nearly free for unchanged code.

### 4.10 Equivalence Checking (`equivalence.rs`)

The `are_equivalent()` function is the foundation for equivalent mutant pruning in Phase 7. It takes two `HirExpr` expressions, converts both into a shared e-graph, runs equality saturation with the core rewrite rules, and checks whether the two expressions end up in the same e-class. If they do, the expressions are provably equivalent under the set of algebraic identities encoded in the rewrite rules. This is not a complete equivalence proof (the e-graph can only prove equivalences discoverable by equality saturation within the node and time limits), but it is sound: if the e-graph says two expressions are equivalent, they are guaranteed to be equivalent.

```rust
pub fn are_equivalent(
    expr_a: &HirExpr,
    expr_b: &HirExpr,
    interner: &Interner,
) -> EquivalenceResult {
    let mut egraph = EGraph::new(GlyimAnalysis::default());
    let id_a = hir_expr_to_egraph(expr_a, &mut egraph, interner);
    let id_b = hir_expr_to_egraph(expr_b, &mut egraph, interner);
    let runner = Runner::default()
        .with_egraph(egraph)
        .run(&core_rewrites())
        .with_node_limit(10_000)
        .with_time_limit(Duration::from_millis(50));
    let equiv = runner.egraph.find(id_a) == runner.egraph.find(id_b);
    EquivalenceResult {
        equivalent: equiv,
        iterations: runner.iterations.len(),
        egraph_size: runner.egraph.total_number_of_nodes(),
        elapsed: runner.elapsed(),
    }
}
```

## 5. Effect System Specification

### 5.1 Placement Decision: `glyim-hir/src/effects.rs` vs. New Crate

After analyzing the dependency structure, the effect analysis module should be placed in `glyim-hir` as a new file (`glyim-hir/src/effects.rs`) rather than as a separate crate. The rationale is threefold: first, the effect analyzer only depends on HIR types and the interner, both of which are already dependencies of `glyim-hir`; second, the pattern is already established by `normalize.rs` and `semantic_hash.rs`, which are HIR analysis modules living inside `glyim-hir`; and third, creating a separate `glyim-effects` crate would add workspace complexity without any dependency isolation benefit. The public API will be exported through `glyim-hir`'s `lib.rs`, following the same pattern as the existing analysis modules.

### 5.2 EffectSet Definition

```rust
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct EffectSet {
    pub may_read: bool,       // Reads external mutable state
    pub may_write: bool,      // Writes external mutable state
    pub may_allocate: bool,   // Allocates heap memory
    pub may_panic: bool,      // May panic / throw
    pub may_diverge: bool,    // May not terminate (infinite loop)
    pub is_pure: bool,        // No effects at all
}
```

### 5.3 EffectAnalyzer Algorithm

The `EffectAnalyzer` performs a bottom-up, fixpoint-based analysis over the HIR call graph. The analysis proceeds in three phases. In the first phase, seed effects are assigned to known built-in functions: `print` and `println` are marked as `may_write`, `__glyim_alloc` and `__glyim_free` are marked as `may_allocate`, and division and modulo operations are marked as `may_panic` (due to division by zero). In the second phase, each user-defined function is analyzed by walking its body expression and collecting the effects of every function call, every allocation site, and every potentially diverging loop. The effects of a function are the union of the effects of all operations it performs and all functions it calls. In the third phase, the analysis iterates until a fixpoint is reached, because function A may call function B which calls function A, creating a cycle that requires multiple passes to stabilize.

### 5.4 Integration with the E-Graph

The effect analysis feeds into the e-graph optimizer in three ways. First, pure expressions can be freely reordered, duplicated, or eliminated by rewrite rules, because they have no observable side effects. Second, the `is_pure` flag is stored in the `GlyimAnalysis` data for each e-class, enabling the cost function to prefer pure expressions over impure ones (since pure expressions are candidates for common sub-expression elimination). Third, the effect information is propagated to the LLVM code generator, where pure functions are annotated with the `readonly` attribute and `noalias` parameter attributes, enabling LLVM to perform more aggressive optimizations on the generated code.

### 5.5 Integration with the LLVM Code Generator

After the e-graph optimization pass produces the optimized HIR, the effect analysis results are passed to the `Codegen` struct along with the optimized HIR. The `Codegen` is modified to accept an optional `EffectAnalysis` parameter. When present, the code generator annotates each LLVM `FunctionValue` with the appropriate attributes: pure functions get the `readonly` attribute (indicating that the function does not write to memory), functions that do not allocate get the `noalias` return attribute (indicating that the return pointer does not alias any other pointer), and functions that may panic get the `willreturn` attribute only if they are guaranteed to terminate (i.e., `may_diverge` is false). These annotations are emitted via inkwell's function attribute API, which maps directly to LLVM IR function attributes.

## 6. Pipeline Integration

### 6.1 Modifications to `glyim-compiler/src/pipeline.rs`

The pipeline modification is minimal and backwards-compatible. The `compile_source_to_hir()` function gains an optional e-graph optimization step that is gated behind the `egraph-opt` feature flag. When the feature is disabled, the pipeline behaves exactly as before. When the feature is enabled, the e-graph pass runs after monomorphization and before code generation. The same pattern applies to the incremental compilation path in `compile_source_to_hir_incremental()`, where the e-graph pass is integrated as a query in the `QueryContext`.

### 6.2 Query Integration

The e-graph optimization is registered as a query in the query engine, with the function's semantic hash as the query key and the optimized HIR as the query value. The query dependencies include the function's source code (via the `parse_file` query), the type check result (via the `type_check` query), the monomorphization result (via the `monomorphize` query), and the e-graph rule set version. When any of these dependencies change, the query is invalidated and re-executed. When none have changed, the cached optimized HIR is returned instantly.

The query fingerprint for the e-graph optimization includes the function's semantic hash, the rule set version number, and the effect analysis version. This ensures that when new rewrite rules are added (incrementing the rule set version), all e-graph queries are automatically invalidated, and the optimizer re-runs on the next build. The `InvariantCertificate` serves as an additional fast-path check: even if the query fingerprint has changed (because the function was recompiled), if the certificate matches, the optimization result can be reused.

### 6.3 Merkle Store Integration for Invariant Certificates

Invariant certificates are stored in the `MerkleStore` as `MerkleNode` objects. The node's hash is computed from the certificate's content (signature hash + purity + complexity + canonical form hash + rule set version). The node's children are the semantic hashes of the function's callees, creating a dependency chain that ensures certificates are invalidated when a callee's semantics change. The `MerkleStore`'s CAS backing ensures that certificates are shared across branches (since they are content-addressed), and the DashMap cache ensures fast lookups during incremental builds.

## 7. Step-by-Step Implementation Plan

### 7.1 Step 3.1: Create glyim-egraph Crate Skeleton (1 day)

- Create `crates/glyim-egraph/` directory with `Cargo.toml`, `src/lib.rs`, and empty module files
- Add `glyim-egraph` to the workspace members in the root `Cargo.toml`
- Add `egg = "0.6"` dependency and verify it compiles against the workspace's Rust edition (2024)
- Define the public API surface in `lib.rs`: `optimize()`, `are_equivalent()`, `InvariantCertificate`, `OptimizeConfig`
- Run `cargo check -p glyim-egraph` to verify the skeleton compiles

### 7.2 Step 3.2: Define GlyimExpr Language and HirExpr Conversion (4-5 days)

- Implement `GlyimExpr` enum in `lang.rs` with all variants matching the `HirExpr` optimization-relevant subset
- Implement the `egg::Language` trait for `GlyimExpr`, including the `children()` method and `Display` formatting
- Implement `hir_fn_to_egraph()` in `convert.rs`: walk `HirFn` body, create e-nodes, return root e-class ID
- Implement `egraph_to_hir()` in `convert.rs`: extract best expression from e-class, reconstruct `HirExpr` tree
- Handle the Block/Statement challenge: represent let-bindings as let-expressions, preserve statement order
- Build a type map (`HashMap<Id, HirType>`) alongside the e-graph to track expression types
- Write comprehensive conversion round-trip tests: convert `HirExpr` to e-graph and back, verify structural equivalence for simple functions
- Test with the existing pipeline: feed a simple Glyim program through parsing, type checking, monomorphization, then through the conversion layer, and verify the e-graph representation is correct

### 7.3 Step 3.3: Implement Core Algebraic Rewrite Rules (3-4 days)

- Implement identity elimination rules (`add-zero, mul-one, sub-zero, div-one, and-true, or-false`)
- Implement strength reduction rules (`mul-by-2/4/8, div-by-2, mod-power-2`) with integer type guards
- Implement commutativity rules (`add-comm, mul-comm, eq-comm, and-comm, or-comm`)
- Implement associativity rules (`add-assoc, mul-assoc`) for constant propagation through operation chains
- Implement double-negation and double-not elimination rules
- Implement boolean simplification rules (`not-eq, not-neq, and-false, or-true, implies-to-or`)
- Add type-conditional rule application: integer-only rules check the type map, float-only rules check for `Float` type, boolean rules check for `Bool` type
- Write exhaustive rule tests: for each rule, verify that it applies to matching patterns and does not apply to non-matching patterns
- Test rule interaction: verify that multiple rules can fire in sequence (e.g., `mul-by-2` then `add-zero`)

### 7.4 Step 3.4: Implement Cost Function and Extraction (2-3 days)

- Implement `AstSizeCostFn` (default): cost = 1.0 per node, preferring fewer total operations
- Implement `StrengthReductionCostFn`: multiplication costs 3.0, shift costs 1.0, division costs 10.0
- Implement the extraction algorithm: walk the e-graph from the root, select the cheapest e-node at each step
- Handle extraction of let-expressions and blocks: preserve statement ordering while optimizing sub-expressions
- Write tests verifying that extraction prefers strength-reduced forms (e.g., `(<< a 1)` over `(* a 2)`)
- Write tests verifying that extraction prefers identity-eliminated forms (e.g., `a` over `(+ a 0)`)

### 7.5 Step 3.5: Implement InvariantCertificate Computation and Caching (3-4 days)

- Define `InvariantCertificate` struct with `signature_hash, is_pure, may_panic, may_allocate, complexity, ir_size, callees, canonical_form_hash, rule_set_version`
- Implement `compute_certificate()` that analyzes a `HirFn` and produces its `InvariantCertificate`
- Implement certificate matching: compare a newly computed certificate with a cached one from the `MerkleStore`
- Implement certificate storage: serialize the certificate and store it as a `MerkleNode` in the `MerkleStore`, with the function's semantic hash as the key
- Integrate with the query engine: register the e-graph optimization as a query whose fingerprint includes the certificate
- Write tests: verify that two semantically identical functions produce matching certificates, and that a modified function produces a different certificate
- Write tests: verify that certificate caching correctly skips optimization when the certificate matches, and re-optimizes when it does not

### 7.6 Step 3.6: Implement EffectAnalyzer in glyim-hir (4-5 days)

- Create `glyim-hir/src/effects.rs` with `EffectSet` struct and `EffectAnalyzer`
- Implement seed effect assignment for built-in functions (`print, println, alloc, free, abort, division`)
- Implement body-walking analysis: for each `HirFn`, walk the body `HirExpr` and collect effects from calls, allocations, and potentially diverging loops
- Implement fixpoint iteration: handle recursive and mutually recursive call graphs by iterating until effects stabilize
- Export `EffectSet` and `EffectAnalyzer` from `glyim-hir/src/lib.rs`
- Implement `can_parallelize()` method that checks whether two functions can safely execute concurrently
- Write tests: verify that pure functions are correctly identified, that effects propagate through call chains, and that recursive functions reach fixpoint
- Write tests: verify that `can_parallelize()` correctly identifies safe and unsafe parallelism scenarios

### 7.7 Step 3.7: Integrate E-Graph Pass into Query Pipeline (3-4 days)

- Modify `glyim-compiler/src/pipeline.rs` to add the e-graph optimization step between monomorphization and code generation
- Gate the e-graph step behind the `egraph-opt` feature flag in `glyim-compiler/Cargo.toml`
- Register the e-graph optimization as a query in the incremental compilation path
- Pass the `EffectAnalysis` results from the effect analyzer to the e-graph optimizer
- Pass the optimized HIR to `Codegen::generate()` instead of the raw `mono_hir`
- Write integration tests: compile a full Glyim program with `egraph-opt` enabled and verify it produces the correct output
- Write integration tests: verify that incremental builds with the e-graph pass are faster than full rebuilds

### 7.8 Step 3.8: Add LLVM Metadata Hints from Effect Analysis (2-3 days)

- Modify `Codegen` struct to accept an optional `EffectAnalysis` parameter
- Implement LLVM function attribute annotation: pure functions get `readonly` attribute, non-allocating functions get `noalias`
- Implement LLVM parameter attribute annotation: `readonly` function parameters get `noalias` and `nocapture`
- Use inkwell's `function.add_attribute()` and `function.add_call_site_attribute()` APIs
- Write codegen tests: verify that pure functions have `readonly` in the generated LLVM IR
- Write codegen tests: verify that the optimized program still produces correct results (the attributes are hints, not semantic changes)

### 7.9 Step 3.9: Wire Invariant Certificates into Merkle Store (2-3 days)

- Implement `MerkleNode` serialization for `InvariantCertificate` using `postcard` (already a dependency of `glyim-merkle`)
- Store certificates with the function's semantic hash as the Merkle node hash
- Set the certificate's Merkle children to the semantic hashes of the function's callees, creating a dependency chain
- Implement certificate lookup in the `optimize()` entry point: check `MerkleStore` before running the e-graph
- Write tests: verify that certificates survive process restart (stored in CAS, not just in-memory cache)
- Write tests: verify that changing a callee invalidates the caller's certificate (via the Merkle children chain)

### 7.10 Step 3.10: Comprehensive Testing and Benchmarks (4-5 days)

- Write unit tests for every rewrite rule (approximately 30 rules, each tested with matching and non-matching inputs)
- Write round-trip tests: `HirExpr` to e-graph to optimized `HirExpr`, verify semantic preservation
- Write equivalence tests: verify that `are_equivalent()` correctly identifies equivalent and non-equivalent expressions
- Write performance benchmarks: measure the time to optimize functions of varying sizes (10, 100, 1000 expressions)
- Write incremental benchmarks: measure the time to re-optimize after a small change (single function modified in a 100-function file)
- Write end-to-end tests: compile and run Glyim programs with `egraph-opt` enabled, verify correctness of output
- Add benchmark results to the CI pipeline to track performance regressions
- Verify that the e-graph pass respects the `node_limit`, `time_limit`, and `iter_limit` configuration parameters

## 8. Success Criteria

| Criterion | Verification Method | Target |
|---|---|---|
| E-graph proves `x+0 = x` | Unit test with `are_equivalent(x+0, x)` | Must return equivalent = true |
| E-graph proves `x*1 = x` | Unit test with `are_equivalent(x*1, x)` | Must return equivalent = true |
| E-graph proves `x*2 = x<<1` | Unit test with `are_equivalent(x*2, x<<1)` | Must return equivalent = true |
| Auto-format triggers zero recompilation | Integration test: format file, rebuild with `--incremental` | All queries green, <50ms |
| Pure functions get `readonly` attribute | Codegen test: compile pure fn, inspect LLVM IR | Function has `readonly` attribute |
| InvariantCertificate skip works | Benchmark: compile twice, second time skips optimization | Certificate cache hit >95% |
| Equivalent mutant pruning | Unit test: `are_equivalent(x+x, 2*x)` | Must return equivalent = true |
| Effect analysis identifies pure functions | Unit test: analyze fn that only computes, verify `is_pure` | `is_pure = true` for computation-only functions |
| E-graph respects time budget | Benchmark: optimize 1000-expression function | Completes within 50ms time limit |
| Feature flag works | Compile with and without `egraph-opt` feature | Without: same as before; With: optimized HIR |

## 9. Risk Register

| Risk | Likelihood | Impact | Mitigation |
|---|---|---|---|
| E-graph scalability for large functions (>1000 expressions) | Medium | High | Set `node_limit=50000` and `time_limit=50ms`; fall back to no optimization for functions exceeding limits; log warning when limit hit |
| egg 0.6 API instability or bugs | Low | High | Pin exact egg version; write comprehensive wrapper tests; maintain a fallback path that skips e-graph optimization on internal errors |
| HIR-to-e-graph conversion loses information | Medium | Medium | Write round-trip property tests; verify that `convert(convert(hir))` produces semantically equivalent HIR; use the existing `semantic_hash` to verify round-trips |
| Effect analysis fixpoint does not converge | Low | Medium | Set maximum iteration count (e.g., 100); if fixpoint not reached, conservatively mark remaining functions as impure; log a warning |
| Rewrite rules produce incorrect equivalences | Low | Critical | Write formal verification tests for each rule; run differential testing (compile with and without e-graph, compare execution results); use the existing test suite as a regression check |
| Feature flag combinatorics (`egraph-opt` with/without incremental) | Low | Low | Test matrix: (no features, incremental only, egraph-opt only, both); CI covers all four combos |
| Performance regression from e-graph overhead | Medium | Medium | Benchmark before/after; make e-graph pass opt-in via feature flag; use `InvariantCertificate` caching to minimize repeated work |

## 10. Estimated Timeline

| Step | Task | Effort (days) | Dependencies |
|---|---|---|---|
| 3.1 | Create glyim-egraph crate skeleton | 1 | None |
| 3.2 | Define `GlyimExpr` language and HIR conversion | 4-5 | Step 3.1 |
| 3.3 | Implement core algebraic rewrite rules | 3-4 | Step 3.2 |
| 3.4 | Implement cost function and extraction | 2-3 | Step 3.2 |
| 3.5 | Implement `InvariantCertificate` computation and caching | 3-4 | Steps 3.2, 3.3, 3.4 |
| 3.6 | Implement `EffectAnalyzer` in `glyim-hir` | 4-5 | Step 3.1 (parallel with 3.2-3.5) |
| 3.7 | Integrate e-graph pass into query pipeline | 3-4 | Steps 3.2, 3.3, 3.4, 3.5, 3.6 |
| 3.8 | Add LLVM metadata hints from effect analysis | 2-3 | Steps 3.6, 3.7 |
| 3.9 | Wire invariant certificates into Merkle store | 2-3 | Steps 3.5, 3.7 |
| 3.10 | Comprehensive testing and benchmarks | 4-5 | All previous steps |
| **Total** | | **28-37** | |

The critical path runs through Steps 3.1, 3.2, 3.3, 3.4, 3.5, 3.7, and 3.10, totaling approximately 20-26 days. Step 3.6 (`EffectAnalyzer`) can be developed in parallel with Steps 3.2-3.5, since it only depends on `glyim-hir` types. Steps 3.8 and 3.9 are integration tasks that can be completed in parallel after Step 3.7. The recommended approach is to assign Steps 3.2-3.5 and Step 3.6 to two developers working in parallel, reducing the wall-clock timeline to approximately 22-30 days.

## 11. Feature Flag Configuration

Phase 3 introduces the `egraph-opt` feature flag, which gates the e-graph optimization pass. The flag is added to `glyim-compiler/Cargo.toml` with appropriate dependencies. When the flag is disabled, the compiler behaves exactly as before (no e-graph pass, no effect analysis). When the flag is enabled, the e-graph pass runs after monomorphization, the effect analyzer runs before the e-graph pass, and the LLVM code generator receives effect metadata for function attribute annotation.

```toml
# In glyim-compiler/Cargo.toml
[features]
default = []
incremental = ["glyim-query"]
semantic-cache = ["incremental"]
egraph-opt = ["glyim-egraph", "glyim-effects"]
live-jit = ["semantic-cache"]
speculative = ["live-jit", "egraph-opt"]
full = ["speculative"]
```

The feature flag ensures backward compatibility: existing users who do not need e-graph optimization can continue using the compiler without the egg dependency (which adds approximately 2MB to the compile time). The feature flag also enables safe rollout: the e-graph pass can be enabled in CI for testing while remaining opt-in for production builds.

## 12. Performance Targets

| Metric | Before Phase 3 | After Phase 3 | After Full Implementation |
|---|---|---|---|
| Unchanged function re-optimization | Always re-optimizes | <1ms (certificate hit) | <1ms |
| Identity elimination (x+0, x*1) | Relies on LLVM -O2 | Proven at HIR level | Proven at HIR level |
| Strength reduction (x*2 -> x<<1) | May or may not be caught by LLVM | Guaranteed by e-graph | Guaranteed + profile-guided |
| Pure function LLVM hints | None | readonly/noalias attributes | readonly/noalias + PGO data |
| Equivalent mutant pruning | N/A (no mutation testing) | 50%+ trivially equivalent pruned | 70%+ with extended rules |
| E-graph pass overhead (cold) | N/A | ~50ms per function (50K nodes) | ~30ms with profile-guided rules |
| E-graph pass overhead (warm) | N/A | <1ms (certificate cache hit) | <1ms |

## 13. Appendix: Mapping to Existing Code

| New Feature | Existing Code to Reuse | Modification Needed |
|---|---|---|
| GlyimExpr language | `glyim-hir/src/normalize.rs` (NormalizedExpr pattern) | Adapt NormalizedExpr variants to egg Language trait; reuse commutative sorting logic |
| HIR-to-e-graph conversion | `glyim-hir/src/lower/expr.rs` (lowering pattern) | Mirror the HirExpr walking pattern but create e-nodes instead of HIR nodes |
| Type constraints on rules | `glyim-typeck` expr_types: Vec<HirType> | Pass expr_types to the e-graph as an external type map; consult during rule application |
| Semantic hash for certificates | `glyim-hir/src/semantic_hash.rs` | Reuse SemanticHash as the canonical_form_hash in InvariantCertificate |
| Certificate storage | `glyim-merkle` MerkleStore | Store certificates as MerkleNode with CAS backing; no MerkleStore changes needed |
| Query integration | `glyim-query` QueryContext | Register optimize_egraph as a new query; no QueryContext API changes needed |
| Effect analysis seeds | `glyim-compiler` pipeline.rs (prelude builtins) | Mark prelude builtins (print, alloc, free, abort) with their known effects |
| LLVM attribute emission | `glyim-codegen-llvm` Codegen struct | Add optional EffectAnalysis parameter; annotate FunctionValue when present |
| Commutative op detection | `HirBinOp::is_commutative()` in normalize.rs | Reuse directly in e-graph commutativity rules |
| Monomorphized HIR as input | `glyim-hir` monomorphize module output | No changes needed; mono_hir is the direct input to the e-graph pass |
