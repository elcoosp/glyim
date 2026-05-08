# Glyim Typeck V3: The Metacompiler — Full Specification

**Document ID:** GLYIM-TYPECK-V3-SPEC  
**Version:** 1.0.0-draft  
**Status:** Draft  
**Date:** 2025-06-23  
**Owner:** Glyim Compiler Team  
**Approvers:** [Pending]

---

# Table of Contents

- [Part I: Vision & Strategic Alignment](#part-i-vision--strategic-alignment)
- [Part II: Business & Stakeholder Requirements](#part-ii-business--stakeholder-requirements)
- [Part III: Software Requirements Specification](#part-iii-software-requirements-specification)
- [Part IV: Architecture & Design Specification](#part-iv-architecture--design-specification)
- [Part V: Behavioral Specification & Test Verification](#part-v-behavioral-specification--test-verification)
- [Appendices](#appendices)

---

# Part I: Vision & Strategic Alignment

## 1.1 Vision Statement

Glyim Typeck V3 transforms the type checker from a passive constraint validator into an **active metacompiler** — a demand-driven, query-cached engine that unifies Hindley-Milner inference, CHR-based trait resolution, profunctor-optics reflection, and multi-stage metaprogramming within a single coherent system, while maintaining a drop-in compatible interface with the existing compilation pipeline.

## 1.2 Elevator Pitch

> For compiler engineers building high-performance systems languages, who are dissatisfied with the poor error messages and rigid metaprogramming of Rust and Zig, Glyim Typeck V3 is a metacompiling type checker that provides CHR-driven trait resolution with bi-abductive diagnostics, GHC-style generic reflection via profunctor optics, and Wasm-sandboxed multi-stage metaprogramming — all through a drop-in replacement for the existing type checker that produces identical pipeline outputs.

## 1.3 Problem Statement & Business Context

**Problem:** The current Glyim type checker (V1) uses syntax-directed eager substitution. It cannot:

1. Provide actionable error messages for complex generic mismatches (users see "expected T, found U" with no structural diff or autofix).
2. Support compile-time reflection or generic derivation (users hand-write serializers, visitors, and ORMs).
3. Express type-state transitions cleanly (workarounds via enums are verbose and error-prone).
4. Support comptime evaluation or macro expansion during type checking (macros run in a separate pass with no access to type information).
5. Scale incrementally — the entire module is re-checked on every change.

**Why now:** Glyim is targeting production use in systems programming domains where reflection and metaprogramming are table stakes (serialization, FFI bindings, protocol generation). Without these capabilities, Glyim cannot compete with Rust (proc macros), Zig (comptime), or D (CTFE).

## 1.4 Target Users & Customers

| User Class | Primary Needs | Frequency |
|---|---|---|
| **Application developers** | Correct type inference; actionable error messages; derive macros | Daily |
| **Library authors** | Generic derivation; reflection for serialization/ORM; type-state APIs | Weekly |
| **Compiler contributors** | Understandable architecture; query-based incremental recompilation | Monthly |
| **IDE/LSP integrators** | Fast incremental type checking; hole-driven development; hover types | Continuous |

## 1.5 Desired Outcomes & Success Metrics

| Business Outcome | Key Results / KPIs | Measurement Method |
|---|---|---|
| **Improved developer experience** | ≥80% of type errors include structural diff or autofix suggestion | Diagnostic sampling from CI |
| **Enable metaprogramming** | ≥5 core stdlib types use derived reflection by v3.0 release | Stdlib audit |
| **Incremental performance** | Re-typecheck after single-line edit completes in <50ms for 10K LOC modules | Benchmark suite |
| **Pipeline compatibility** | 100% of existing integration tests pass without modification | CI gate |

## 1.6 Goals and Non-Goals

### Goals

- **G1:** Implement CHR-based constraint solving replacing the ad-hoc method resolution hashmap.
- **G2:** Implement bidirectional type elaboration with hole-driven development.
- **G3:** Implement GHC-style `Rep` generic representation and profunctor-optics-based reflection.
- **G4:** Implement comptime evaluation integrated with the Wasm macro executor (`glyim-macro-core`).
- **G5:** Implement query-based incremental type checking using `glyim-query`.
- **G6:** Implement bi-abductive diagnostic synthesis (structural diffing + autofix suggestions).
- **G7:** Maintain 100% backward compatibility with the `TypeChecker` public API.

### Non-Goals

- **NG1:** Changing the HIR data structures or lowering pipeline.
- **NG2:** Implementing full dependent types or higher-kinded types in V3.
- **NG3:** Rewriting the monomorphizer or codegen backend.
- **NG4:** Supporting cross-module incremental recompilation in V3 (single-module only).
- **NG5:** Implementing algebraic effects as a runtime feature (V3 provides the type-level foundation only).
- **NG6:** Optimizing for compile-time throughput at the expense of correctness.

## 1.7 Strategic Constraints

| Constraint | Description |
|---|---|
| **Pipeline compatibility** | Output must conform to `Vec<HirType>` (indexed by `ExprId`) and `HashMap<ExprId, Vec<HirType>>`. The desugarer and monomorphizer must work unchanged. |
| **No HIR mutation** | The type checker must not mutate the HIR. Annotations may be stored in side-tables. |
| **Wasm sandboxing** | All macro/comptime execution must use `glyim-macro-core`'s Wasm executor with fuel metering. |
| **Incremental query foundation** | All memoized computation must flow through `glyim-query`'s fingerprint-based caching and dependency tracking. |
| **Error recovery** | The `ErrorGuaranteed` pattern must be used; the type checker must never panic on bad code. |

## 1.8 Risks, Assumptions, and Open Questions

| ID | Risk / Assumption | Impact if wrong | Mitigation |
|---|---|---|---|
| R-01 | CHR solver may not scale to Rust-level trait complexity | Type checking times exceed 1s for large crates | Benchmark early; implement simplification rules for common patterns |
| R-02 | Comptime evaluation may create unresolvable dependency cycles | Pipeline hangs or produces inconsistent results | Implement cycle detection in query system; limit comptime fuel |
| R-03 | Wasm serialization overhead for reflection metadata | Macro expansion >10ms for complex types | Cache serialized Rep; pre-compute SoA layouts |
| R-04 | Optics generation may cause code bloat | Binary size increase >20% | Only generate optics for `@reflectable` types; dead-code elimination |
| O-01 | Pipeline will not need changes beyond `TypeChecker` API | Significant rework if assumptions violated | Validate with integration tests before implementing V3 features |
| Q-01 | Should `Code<T>` and staging be exposed to users in V3 or deferred to V4? | Scope and complexity | Decision deferred; infrastructure built, surface area minimized |
| Q-02 | What is the maximum fuel budget for comptime blocks? | UX vs safety trade-off | Start with 1M instructions; make configurable |

---

# Part II: Business & Stakeholder Requirements

## 2.1 Stakeholder Map

| Stakeholder | Role | Primary Concern | Influence |
|---|---|---|---|
| **Compiler team** | Implementers | Architecture clarity; incremental correctness | High |
| **Language designers** | Spec owners | Feature completeness; semantic coherence | High |
| **Application developers** | End users | Error quality; derive macro availability | Medium |
| **IDE team** | LSP integrators | Incremental speed; hole results | Medium |
| **CI/infrastructure** | Operators | Build determinism; cache validity | Low |

## 2.2 Business Requirements

| ID | Requirement | Fit Criterion | Priority |
|---|---|---|---|
| BR-01 | The type checker must produce identical `expr_types` and `call_type_args` as V1 for all existing test cases | 100% of `glyim-cli-tests-full` integration tests pass | Must |
| BR-02 | The type checker must report at least one autofix suggestion for ≥50% of type mismatch errors in the test corpus | Automated diagnostic audit | Should |
| BR-03 | The type checker must support `@reflectable` annotation and `comptime` blocks | 3+ stdlib types use derived reflection | Must |
| BR-04 | Incremental re-typecheck after a single-statement edit must complete in <100ms for modules ≤10K LOC | Benchmark suite | Should |
| BR-05 | The type checker must never crash on invalid input | Zero panics in fuzz testing | Must |

## 2.3 Domain Model and Ubiquitous Language

```
┌─────────────────────────────────────────────────────────────────┐
│                    TYPECK V3 UBIQUITOUS LANGUAGE                │
├─────────────────┬───────────────────────────────────────────────┤
│ Term            │ Definition                                  │
├─────────────────┼───────────────────────────────────────────────┤
│ Ty              │ Internal type reference (arena index)        │
│ TyKind          │ The kind of a type (Int, App, Code, etc.)   │
│ TyArena         │ Append-only arena allocating Ty values       │
│ Goal            │ A logical proposition to be proven           │
│ ChrRule         │ A rewrite rule for the CHR solver            │
│ ChrStore        │ The collection of rules + pending/proven    │
│ Rep             │ Generic representation of a type (GHC-style) │
│ Optic           │ First-class getter/setter pair (Lens/Prism)  │
│ Code<T>         │ A quoted expression of type T at future stage│
│ Level           │ Staging level (Comptime/Buildtime/Runtime)   │
│ ExprId          │ Stable expression identifier (u32 newtype)   │
│ HirType         │ Pipeline-facing type representation          │
│ Elaboration     │ Translating untyped HIR → typed constraints │
│ Unification     │ Equating two Ty variables via Union-Find     │
│ Hole            │ An unresolved inference variable             │
│ Freeze          │ Resolving all Infer vars → HirType           │
│ TypeMetaSoA     │ Struct-of-Arrays reflection metadata         │
│ MPHF            │ Minimal Perfect Hash Function               │
│ ErrorGuaranteed │ Token proving an error was emitted           │
│ MacroExecutor   │ Wasm-based sandbox for macro/comptime exec   │
│ QueryContext     │ glyim-query caching and dependency engine    │
│ Fingerprint     │ SHA-256 content hash for query keys          │
│ Fuel            │ Instruction budget for Wasm execution        │
│ Bi-abduction    │ Synthesizing missing wrappers (Some, Ok)     │
│ Zippering       │ Structural diffing of type trees             │
│ Type-state      │ CHR-proven state transition on nominal types │
└─────────────────┴───────────────────────────────────────────────┘
```

## 2.4 Business Rules and Policies

| ID | Rule | Source |
|---|---|---|
| BRule-01 | Every `ExprId` encountered in the HIR must have an entry in `expr_types` | Pipeline contract |
| BRule-02 | `call_type_args` must be keyed by the original `ExprId` (stable through desugaring) | Pipeline contract |
| BRule-03 | Comptime blocks must not perform I/O beyond the virtual filesystem | Determinism |
| BRule-04 | Macro Wasm modules must have an `expand` export accepting `(i32, i32, i32) → i32` | `glyim-macro-core` contract |
| BRule-05 | All macro execution must be fuel-metered (default: 1M instructions) | Safety |
| BRule-06 | `ErrorGuaranteed` must be infectious: any type involving `Error` unifies with anything | Error recovery |
| BRule-07 | Reflection metadata is only generated for types proven `Reflectable` by the CHR solver | Zero-cost principle |

---

# Part III: Software Requirements Specification

## 3.1 Introduction and Scope

This SRS specifies the software requirements for Glyim Typeck V3 — a complete rewrite of the `glyim-typeck` crate that replaces the V1 syntax-directed eager substitution engine with a demand-driven, query-cached metacompiler.

**In scope:**
- Internal type system (`Ty`, `TyKind`, `TyArena`)
- Unification engine (Union-Find with `ErrorGuaranteed`)
- CHR constraint solver
- Bidirectional elaboration (check/synth modes)
- Generic representation (`Rep`) and profunctor optics generation
- Comptime evaluation via `glyim-macro-core`
- Query-based incremental caching via `glyim-query`
- Diagnostic synthesis (bi-abduction, zippering)
- Freeze phase (translating `Ty` → `HirType`)
- Reflection metadata generation (SoA, MPHF)

**Out of scope:**
- HIR data structure modifications
- Desugarer changes
- Monomorphizer changes
- Codegen backend changes
- LSP protocol changes

## 3.2 External Interface Requirements

### 3.2.1 Input Interface

The type checker receives:

| Input | Type | Description |
|---|---|---|
| `hir` | `&glyim_hir::Hir` | The lowered HIR (read-only) |
| `interner` | `&mut glyim_interner::Interner` | String interner (may intern new symbols) |

### 3.2.2 Output Interface

The type checker produces `TypeCheckOutput`:

```rust
pub struct TypeCheckOutput {
    /// Types for every ExprId encountered, indexed by ExprId::as_usize()
    pub expr_types: Vec<HirType>,
    
    /// Concrete type arguments for generic call sites, keyed by ExprId
    pub call_type_args: HashMap<ExprId, Vec<HirType>>,
    
    /// Reflection metadata for @reflectable types (consumed by codegen/JIT)
    pub reflect_metadata: Vec<crate::reflect::TypeMetaSoA>,
    
    /// Items generated by comptime blocks (must be appended to HIR before mono)
    pub generated_items: Vec<glyim_hir::HirItem>,
}
```

**Critical constraints:**
- `expr_types` must contain an entry for every `ExprId` encountered in the HIR.
- `call_type_args` keys must be the original `ExprId` (stable through desugaring).
- The type checker must not mutate the HIR.

### 3.2.3 Error Interface

Errors must implement `Into<Vec<glyim_diag::Diagnostic>>` for pipeline integration.

## 3.3 Functional Requirements

### 3.3.1 Type Inference and Elaboration

| ID | Requirement | EARS Pattern | Priority |
|---|---|---|---|
| FR-01 | The system shall infer types for all HIR expression forms (IntLit, Binary, Call, MethodCall, Match, FieldAccess, StructLit, etc.) | Ubiquitous | Must |
| FR-02 | When the expected type is known, the system shall check expressions against that type (check mode) | Event-driven | Must |
| FR-03 | When the expected type is unknown, the system shall synthesize the type from the expression (synth mode) | Ubiquitous | Must |
| FR-04 | When a hole expression is encountered, the system shall constrain its inference variable to match the expected type | Event-driven | Must |
| FR-05 | When a method call is encountered, the system shall emit a `TraitImpl` goal for resolution | Event-driven | Must |
| FR-06 | If a method call receiver has a type-state pattern, the system shall emit a `StateTransition` goal | Event-driven | Should |
| FR-07 | When a generic call is encountered, the system shall infer concrete type arguments and record them in `call_type_args` | Event-driven | Must |
| FR-08 | When an expression has type `Error`, the system shall unify it with any expected type without emitting a new error | Unwanted behavior | Must |
| FR-09 | When unification encounters `?0 = Vec<?0>`, the system shall emit an `InfiniteType` error and poison the variable | Unwanted behavior | Must |

### 3.3.2 CHR Constraint Solving

| ID | Requirement | EARS Pattern | Priority |
|---|---|---|---|
| FR-10 | The system shall solve trait implementation goals via CHR rules registered from `impl` blocks | Ubiquitous | Must |
| FR-11 | When an `impl<T> Display for Vec<T> where T: Display` is encountered, the system shall register a CHR `Propagate` rule | Event-driven | Must |
| FR-12 | When a goal matches a `Simplify` rule whose premises are all proven, the system shall mark the goal as proven | Event-driven | Must |
| FR-13 | When a goal matches a `Propagate` rule whose premises are all proven, the system shall mark the goal as proven and emit new goals | Event-driven | Must |
| FR-14 | If no CHR rule fires for a pending goal, the system shall emit a "trait not implemented" error | Unwanted behavior | Must |
| FR-15 | The system shall solve goals to fixed point before the Freeze phase | Ubiquitous | Must |
| FR-16 | When a `StateTransition` goal is proven, the system shall bind the receiver's type to the new state type | Event-driven | Should |

### 3.3.3 Reflection and Optics

| ID | Requirement | EARS Pattern | Priority |
|---|---|---|---|
| FR-17 | When a type is annotated `@reflectable`, the system shall register a `Reflectable` goal and generate a `Rep` generic representation | Event-driven | Must |
| FR-18 | When a `HasField` goal is emitted for a reflectable type, the system shall prove it via the `Rep` structure | Event-driven | Must |
| FR-19 | For each field in a reflectable type, the system shall generate a monomorphized Lens optic | Ubiquitous | Must |
| FR-20 | For each single-field constructor in a reflectable type, the system shall generate a Prism optic | Ubiquitous | Should |
| FR-21 | The system shall generate `TypeMetaSoA` metadata for all types proven `Reflectable` by the CHR solver | Ubiquitous | Must |
| FR-22 | The system shall compute a minimal perfect hash function for field name lookup within each type's metadata | Ubiquitous | Should |
| FR-23 | When a reflective `getField` call is encountered with a statically-known receiver type, the system shall unify the result type directly | Event-driven | Must |

### 3.3.4 Comptime and Macro Expansion

| ID | Requirement | EARS Pattern | Priority |
|---|---|---|---|
| FR-24 | When a `comptime` block is encountered during elaboration, the system shall execute it via the Wasm `MacroExecutor` | Event-driven | Must |
| FR-25 | When a macro call is encountered during elaboration, the system shall expand it via the Wasm `MacroExecutor` | Event-driven | Must |
| FR-26 | When a comptime block or macro execution exceeds the fuel budget, the system shall emit a deterministic error | Unwanted behavior | Must |
| FR-27 | When a comptime block generates HIR items, the system shall add them to `generated_items` in the output | Event-driven | Must |
| FR-28 | When a macro call's input AST hash matches a cached result, the system shall return the cached output without Wasm execution | Event-driven | Must |
| FR-29 | When a comptime block queries type information, the system shall record a query dependency for invalidation | Event-driven | Must |
| FR-30 | If a comptime block creates a cyclic dependency (querying a type that depends on the block's output), the system shall detect the cycle and emit an error | Unwanted behavior | Must |

### 3.3.5 Staging and Phase Consistency

| ID | Requirement | EARS Pattern | Priority |
|---|---|---|---|
| FR-31 | When a `Quote` expression is encountered, the system shall elaborate the body at an elevated staging level | Event-driven | Should |
| FR-32 | When a `Splice` expression is encountered, the system shall extract the inner type from `Code<T>` | Event-driven | Should |
| FR-33 | If a value from a later stage is used at an earlier stage, the system shall emit a `PhaseViolation` error | Unwanted behavior | Should |
| FR-34 | The system shall support cross-stage persistence for comptime constants | Ubiquitous | Should |

### 3.3.6 Diagnostics

| ID | Requirement | EARS Pattern | Priority |
|---|---|---|---|
| FR-35 | When unification fails between two structural types, the system shall compute a structural diff (zippering) | Event-driven | Must |
| FR-36 | When a type mismatch could be resolved by wrapping in `Some` or `Ok`, the system shall suggest an `AutoFix` | Event-driven | Should |
| FR-37 | When an infinite type is detected, the system shall report the origin span of the inference variable | Event-driven | Must |
| FR-38 | When a CHR goal fails, the system shall report the failed goal with context | Event-driven | Must |
| FR-39 | When a phase violation occurs, the system shall report the used-at and defined-at stages | Event-driven | Should |

### 3.3.7 Incremental Compilation

| ID | Requirement | EARS Pattern | Priority |
|---|---|---|---|
| FR-40 | The system shall use `glyim-query::QueryContext` for memoizing type check results | Ubiquitous | Should |
| FR-41 | When a source file changes, the system shall invalidate only queries that depend on the changed file | Event-driven | Should |
| FR-42 | The system shall record query dependencies automatically via thread-local collection | Ubiquitous | Should |
| FR-43 | The system shall persist query results via `glyim-query::IncrementalState` | Ubiquitous | Could |

## 3.4 Quality Requirements

### 3.4.1 Performance

| ID | Requirement | Fit Criterion | Priority |
|---|---|---|---|
| NFR-PERF-01 | Full type check of a 10K LOC module (cold cache) | <500ms p95 | Must |
| NFR-PERF-02 | Incremental re-check after single-statement edit (warm cache) | <100ms p95 | Should |
| NFR-PERF-03 | CHR solver for a module with 100 impl blocks | <50ms p95 | Must |
| NFR-PERF-04 | Macro expansion (cache hit) | <1ms | Must |
| NFR-PERF-05 | Macro expansion (cache miss) | <100ms p95 | Should |
| NFR-PERF-06 | Reflection metadata generation for 50 reflectable types | <20ms | Should |
| NFR-PERF-07 | Memory usage per module (10K LOC) | <50MB | Should |

### 3.4.2 Reliability

| ID | Requirement | Fit Criterion | Priority |
|---|---|---|---|
| NFR-REL-01 | Zero panics on any input | Fuzz testing with 100K random programs | Must |
| NFR-REL-02 | Error recovery: a single type error shall not prevent checking the rest of the module | >90% of expressions typechecked in error-containing modules | Must |
| NFR-REL-03 | Deterministic: same input always produces same output | CI comparison of outputs | Must |

### 3.4.3 Maintainability

| ID | Requirement | Fit Criterion | Priority |
|---|---|---|---|
| NFR-MAINT-01 | Each subsystem (unification, CHR, elaboration, reflection) shall be independently testable | Unit test coverage >80% per module | Must |
| NFR-MAINT-02 | Adding a new `TyKind` variant shall not require changes to more than 3 files | Architectural rule | Should |

## 3.5 External Interface Requirements

### 3.5.1 Pipeline Contract

The type checker's output must be consumed by:

1. **Desugarer** (`glyim_hir::desugar_method_calls`) — uses `expr_types` to mangle method names.
2. **Monomorphizer** (`glyim_hir::monomorphize`) — uses `expr_types` and `call_type_args`.

| Interface Element | Type | Constraint |
|---|---|---|
| `expr_types` | `Vec<HirType>` | Indexed by `ExprId::as_usize()`; every encountered ID has an entry |
| `call_type_args` | `HashMap<ExprId, Vec<HirType>>` | Keyed by original `ExprId` (pre-desugaring) |

### 3.5.2 Macro Engine Contract

The type checker uses `glyim-macro-core::MacroExecutor` for comptime/macro execution.

| Interface Element | Provider | Consumer |
|---|---|---|
| `MacroExecutor::execute(&self, wasm, input) -> Result<Vec<u8>>` | `glyim-macro-core` | Type checker |
| `wasm_interface::serialize_expr(&HirExpr) -> Vec<u8>` | `glyim-macro-core` | Type checker |
| `wasm_interface::deserialize_expr(&[u8]) -> Option<HirExpr>` | `glyim-macro-core` | Type checker |
| `MacroContext::trait_is_implemented(Symbol, Symbol) -> bool` | Type checker | Wasm macro |
| `MacroContext::get_fields(Symbol) -> Vec<Field>` | Type checker | Wasm macro |

### 3.5.3 Query Engine Contract

The type checker uses `glyim-query::QueryContext` for caching.

| Interface Element | Provider | Consumer |
|---|---|---|
| `QueryContext::query(key, compute, fingerprint, deps) -> V` | `glyim-query` | Type checker |
| `QueryContext::invalidate_fingerprints(changed)` | `glyim-query` | Pipeline |
| `Fingerprint::of(data) -> Fingerprint` | `glyim-query` | Type checker |
| `Dependency::query(key_fingerprint) -> Dependency` | `glyim-query` | Type checker |
| `IncrementalState::load_or_create(path)` | `glyim-query` | Pipeline |

## 3.6 Constraints, Assumptions, and Dependencies

| ID | Type | Description |
|---|---|---|
| CON-01 | Design | Type checker must not mutate the HIR |
| CON-02 | Design | All Wasm execution must be fuel-metered |
| CON-03 | Design | `Error` type is infectious — unifies with anything |
| CON-04 | Design | Reflection metadata is opt-in (`@reflectable`) |
| CON-05 | Dependency | Requires `glyim-macro-core` v0.5+ (Wasm executor) |
| CON-06 | Dependency | Requires `glyim-query` v0.3+ (incremental state) |
| CON-07 | Dependency | Requires `glyim-hir` v0.8+ (stable ExprId through desugaring) |
| DEP-01 | Assumption | HIR items are processed in dependency order |
| DEP-02 | Assumption | `DeclTable` is available for forward references |
| DEP-03 | Assumption | All macro Wasm modules conform to the `expand` signature |
| TBD-01 | Open | Maximum fuel budget for comptime blocks (initial: 1M) |
| TBD-02 | Open | Whether to expose `Code<T>` to users in V3 |

---

# Part IV: Architecture & Design Specification

## 4.1 Architecture Decision Records

### ADR-0001: CHR Solver for Trait Resolution

**Status:** Accepted  
**Date:** 2025-06-23  
**Context:** The V1 type checker uses a `HashMap<Symbol, Vec<MethodDef>>` for method lookup. This cannot handle conditional impls, overlapping instances, or type-state transitions.  
**Decision Drivers:** FR-10 through FR-16; NFR-MAINT-01  
**Decision:** Use Constraint Handling Rules (CHR) for trait resolution. CHR provides a declarative, fixed-point-solving approach that naturally handles conditional impls (`impl<T> Display for Vec<T> where T: Display`) and type-state transitions.  
**Alternatives Considered:**  
- (A) Coq-style canonical structures — too complex for systems language  
- (B) Haskell-style typeclass resolution via term rewriting — similar power, less formalized  
**Consequences:** + Clean separation of rule registration and solving; + Naturally extensible for new goal types; - Must implement CHR solver from scratch; - Performance depends on rule ordering and simplification strategy  
**Links:** FR-10, FR-11, FR-12, FR-13, FR-14, FR-15, FR-16

### ADR-0002: Arena-Allocated Internal Types

**Status:** Accepted  
**Date:** 2025-06-23  
**Context:** The pipeline uses `HirType` (recursive enum with `Box`). Internally, the type checker needs O(1) comparison, copying, and hashing for inference variables and structural types.  
**Decision Drivers:** NFR-PERF-01, NFR-MAINT-02  
**Decision:** Use an append-only `TyArena` with `Ty(usize)` references. All internal type manipulation operates on `Ty`; translation to `HirType` happens only in the Freeze phase.  
**Alternatives Considered:**  
- (A) Interning all types — more complex allocation; harder to manage inference variables  
- (B) Using `Rc<TyKind>` — slower comparison; no O(1) hashing  
**Consequences:** + O(1) copy/hash/eq for `Ty`; + Inference variables are just arena entries; + Clean separation from pipeline types; - Must maintain arena lifetime carefully; - Freeze phase is mandatory  
**Links:** NFR-PERF-01, NFR-MAINT-02

### ADR-0003: Bidirectional Type Checking

**Status:** Accepted  
**Date:** 2025-06-23  
**Context:** V1 uses top-down inference only, which cannot handle holes, lambda arguments, or overloaded literals well.  
**Decision Drivers:** FR-02, FR-03, FR-04  
**Decision:** Implement bidirectional type checking with explicit `check_expr` (expected type known) and `synth_expr` (expected type unknown) modes.  
**Alternatives:**  
- (A) Algorithm W (pure inference) — doesn't support check mode  
- (B) Complete type annotation — too verbose for users  
**Consequences:** + Hole-driven development; + Better inference for lambdas and overloads; - More complex elaboration logic; - Must carefully manage check/synth transitions  
**Links:** FR-02, FR-03, FR-04

### ADR-0004: Query-Based Incremental Compilation

**Status:** Accepted  
**Date:** 2025-06-23  
**Context:** V1 re-checks the entire module on every change. For IDE responsiveness and large codebases, this is unacceptable.  
**Decision Drivers:** NFR-PERF-02, FR-40, FR-41, FR-42  
**Decision:** Use `glyim-query::QueryContext` to memoize all type check results, with automatic dependency tracking via thread-local collection.  
**Alternatives:**  
- (A) Manual dirty-bit tracking — error-prone; doesn't scale  
- (B) Full Salsa framework — too heavy for single-crate integration  
**Consequences:** + Sub-100ms incremental rechecks; + Automatic invalidation; - Thread-local collector adds complexity; - Must ensure all reads go through queries  
**Links:** NFR-PERF-02, FR-40, FR-41, FR-42, FR-43

### ADR-0005: GHC-Style Rep for Reflection

**Status:** Accepted  
**Date:** 2025-06-23  
**Context:** Reflection needs a uniform way to inspect any reflectable type's structure (fields, constructors, annotations).  
**Decision Drivers:** FR-17, FR-18, FR-19, FR-20, FR-21  
**Decision:** Generate a GHC-style generic representation (`Rep`) for each `@reflectable` type, and derive optics and SoA metadata from `Rep`.  
**Alternatives:**  
- (A) Runtime type information (RTTI) via vtables — zero-cost principle violation  
- (B) Manual derive macros — no uniform representation; harder to write generic combinators  
**Consequences:** + Uniform representation for all types; + Natural derivation of optics and metadata; + Foundation for comptime reflection; - Rep generation adds compilation overhead for reflectable types  
**Links:** FR-17, FR-18, FR-19, FR-20, FR-21, FR-22

### ADR-0006: Wasm-Based Comptime Execution

**Status:** Accepted  
**Date:** 2025-06-23  
**Context:** Comptime evaluation must be sandboxed, deterministic, and fuel-metered. `glyim-macro-core` already provides this for proc macros.  
**Decision Drivers:** FR-24, FR-25, FR-26, BRule-03, BRule-05  
**Decision:** Reuse `glyim-macro-core::MacroExecutor` for comptime execution, treating comptime blocks as a special case of macro expansion with access to type information via `MacroContext`.  
**Alternatives:**  
- (A) In-process evaluation — no sandboxing; non-deterministic  
- (B) Separate process — IPC overhead; complex lifecycle  
**Consequences:** + Proven sandboxing; + CAS-based caching for free; + Fuel metering for free; - Serialization overhead for type info queries; - Wasm interface limitations for complex types  
**Links:** FR-24, FR-25, FR-26, FR-28, FR-29, FR-30

## 4.2 System Context

```
┌───────────────────────────────────────────────────────────────────┐
│                        GLYIM COMPILER PIPELINE                   │
│                                                                   │
│  Source → Lexer → Parser → HIR Lowering → [TYPE CHECKER V3] ──→  │
│  → Desugarer → Monomorphizer → Codegen → Link/JIT                │
└───────────────────────────┬───────────────────────────────────────┘
                            │
         ┌──────────────────┼──────────────────┐
         │                  │                  │
         ▼                  ▼                  ▼
┌─────────────┐   ┌─────────────────┐   ┌──────────────┐
│  HIR (RO)   │   │  Interner (RW)  │   │  Diagnostics │
│  HirItem    │   │  Symbol (u32)   │   │  TypeError   │
│  HirExpr    │   │                 │   │  AutoFix     │
│  ExprId     │   │                 │   │              │
└─────────────┘   └─────────────────┘   └──────────────┘
         │                  │                  │
         │         ┌───────┴───────┐          │
         │         │               │          │
         ▼         ▼               ▼          ▼
┌──────────────────────────────────────────────────────────────┐
│                    TYPECK V3 INTERNALS                        │
│                                                              │
│  ┌──────────┐ ┌──────────┐ ┌──────────┐ ┌───────────────┐  │
│  │ TyArena  │ │  CHR     │ │Unification│ │  MacroExecutor│  │
│  │ Ty       │ │  Store   │ │  Table    │ │  (Wasm+Fuel)  │  │
│  │ TyKind   │ │  Goal    │ │  Union-  │ │  MacroContext │  │
│  │          │ │  ChrRule │ │  Find     │ │  (from V3 DB) │  │
│  └──────────┘ └──────────┘ └──────────┘ └───────────────┘  │
│                                                              │
│  ┌──────────┐ ┌──────────┐ ┌──────────┐ ┌───────────────┐  │
│  │   Rep    │ │  Optics  │ │   Freeze │ │  QueryContext │  │
│  │  (GHC)   │ │ Lens/    │ │  Ty →    │ │  (glyim-query)│  │
│  │          │ │ Prism    │ │  HirType │ │  Fingerprint  │  │
│  └──────────┘ └──────────┘ └──────────┘ └───────────────┘  │
│                                                              │
│  ┌──────────────────────┐ ┌──────────────────────────────┐  │
│  │  Elaborator          │ │  Diagnostics                 │  │
│  │  check_expr /        │ │  Bi-abduction (autofix)      │  │
│  │  synth_expr          │ │  Zippering (structural diff) │  │
│  └──────────────────────┘ └──────────────────────────────┘  │
└──────────────────────────────────────────────────────────────┘
                            │
                            ▼
                   ┌─────────────────┐
                   │ TypeCheckOutput │
                   │ expr_types      │
                   │ call_type_args  │
                   │ reflect_meta    │
                   │ generated_items │
                   └─────────────────┘
```

## 4.3 Container Diagram (C4 Level 2)

```
┌─────────────────────────────────────────────────────────────────┐
│                      glyim-typeck (Crate)                       │
│                                                                 │
│  ┌───────────────────────────────────────────────────────────┐  │
│  │                    TyDatabase (Orchestrator)               │  │
│  │                                                           │  │
│  │  ┌──────────┐ ┌────────────────┐ ┌────────────────────┐  │  │
│  │  │TyArena   │ │UnificationTable│ │  ChrStore          │  │  │
│  │  │          │ │                │ │                    │  │  │
│  │  │alloc()   │ │new_var()       │ │add_rules()        │  │  │
│  │  │fresh_    │ │find()          │ │solve()            │  │  │
│  │  │infer()   │ │unify()         │ │                   │  │  │
│  │  └──────────┘ └────────────────┘ └────────────────────┘  │  │
│  │                                                           │  │
│  │  ┌────────────────────────┐ ┌──────────────────────────┐  │  │
│  │  │    Elaborator          │ │       Freezer            │  │  │
│  │  │                        │ │                          │  │  │
│  │  │  check_expr()          │ │  resolve_expr_types()    │  │  │
│  │  │  synth_expr()          │ │  resolve_call_args()     │  │  │
│  │  │  elaborate_item()      │ │  generate_reflect_meta() │  │  │
│  │  └────────────────────────┘ └──────────────────────────┘  │  │
│  │                                                           │  │
│  │  ┌────────────────────────┐ ┌──────────────────────────┐  │  │
│  │  │   RepGenerator         │ │    DiagnosticEngine      │  │  │
│  │  │                        │ │                          │  │  │
│  │  │  build_rep()           │ │  zip_diff()              │  │  │
│  │  │  generate_lenses()     │ │  bi_abductive_synthesis()│  │  │
│  │  │  generate_prisms()     │ │                          │  │  │
│  │  └────────────────────────┘ └──────────────────────────┘  │  │
│  └───────────────────────────────────────────────────────────┘  │
│                                                                 │
│  ┌───────────────────────────────────────────────────────────┐  │
│  │                  Public API (lib.rs)                       │  │
│  │                                                           │  │
│  │  TypeChecker::new(interner) -> TypeChecker                │  │
│  │  TypeChecker::check(&Hir) -> Result<TypeCheckOutput,     │  │
│  │                                      Vec<TypeError>>     │  │
│  └───────────────────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────────────────┘

         │                    │                     │
         │ uses               │ uses                │ uses
         ▼                    ▼                     ▼
┌─────────────────┐  ┌──────────────────┐  ┌───────────────────┐
│ glyim-query     │  │ glyim-macro-core │  │ glyim-hir         │
│                 │  │                  │  │                   │
│ QueryContext    │  │ MacroExecutor    │  │ Hir               │
│ Fingerprint     │  │ MacroRegistry   │  │ HirType           │
│ Dependency      │  │ wasm_interface   │  │ ExprId            │
│ IncrementalState│  │ MacroContext     │  │ HirItem           │
└─────────────────┘  └──────────────────┘  └───────────────────┘
```

## 4.4 Component Design

### 4.4.1 TyArena and Internal Types

```rust
/// O(1) reference to a type in the arena.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Ty(pub usize);

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum TyKind {
    // Primitives
    Int, Float, Bool, Str, Unit, Never,
    Error, Infer,
    
    // Nominal types
    Named(Symbol),
    App(Symbol, Vec<Ty>),
    Fn(Vec<Ty>, Ty),
    RawPtr(Ty),
    
    // V3: Staging
    Code(Ty),
    
    // V3: Const Generics
    Const(Ty, ValueId),
    
    // V3: Effects
    EffectFn(Vec<Ty>, Ty, EffectRow),
    
    // V3: Reflection
    Any,
    TypeInfo(Ty),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum EffectRow {
    Empty,
    Extend(Symbol, Box<EffectRow>),
    Var(Ty),
}

pub struct TyArena {
    kinds: Vec<TyKind>,
    infer_spans: Vec<Span>,
}

impl TyArena {
    pub fn alloc(&mut self, kind: TyKind) -> Ty { /* ... */ }
    pub fn fresh_infer(&mut self, span: Span) -> Ty { /* ... */ }
    pub fn get(&self, ty: Ty) -> &TyKind { /* ... */ }
    pub fn get_infer_span(&self, ty: Ty) -> Option<Span> { /* ... */ }
}
```

### 4.4.2 Unification Table

```rust
pub struct ErrorGuaranteed(#[doc(hidden)] std::convert::Infallible);

impl ErrorGuaranteed {
    pub fn new() -> Self { Self(std::convert::Infallible::new()) }
}

pub struct UnificationTable {
    parents: Vec<Ty>,
    ranks: Vec<u8>,
}

impl UnificationTable {
    pub fn new_var(&mut self, arena: &mut TyArena, span: Span) -> Ty { /* ... */ }
    pub fn find(&self, arena: &TyArena, ty: Ty) -> Ty { /* ... */ }
    pub fn unify(
        &mut self, arena: &mut TyArena, a: Ty, b: Ty,
        span: Span, emit_err: &mut dyn FnMut(TypeError)
    ) -> Result<(), ErrorGuaranteed> { /* ... */ }
}
```

### 4.4.3 CHR Store

```rust
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Goal {
    TraitImpl(Symbol, Vec<Ty>),
    StateTransition(Symbol, Ty, Ty),
    Reflectable(Ty),
    HasField(Ty, Symbol),
    IsoCoerce(Ty, Ty),
}

pub enum ChrRule {
    Simplify { goal: Goal, premises: Vec<Goal> },
    Propagate { goal: Goal, premises: Vec<Goal>, new_goals: Vec<Goal> },
}

pub struct ChrStore {
    rules: Vec<ChrRule>,
    pending_goals: Vec<Goal>,
    proven_goals: HashSet<Goal>,
}

impl ChrStore {
    pub fn new(rules: Vec<ChrRule>) -> Self { /* ... */ }
    pub fn add_rules(&mut self, rules: Vec<ChrRule>) { /* ... */ }
    pub fn solve(&mut self, arena: &TyArena) -> Result<(), ErrorGuaranteed> { /* ... */ }
}
```

### 4.4.4 Rep Generator and Optics

```rust
#[derive(Debug, Clone)]
pub enum Rep {
    Meta(RepMeta, Box<Rep>),
    Sum(Box<Rep>, Box<Rep>),
    Product(Box<Rep>, Box<Rep>),
    Constructor(RepMeta, Box<Rep>),
    Field(RepMeta, Ty),
    Unit,
}

#[derive(Debug, Clone)]
pub struct RepMeta {
    pub name: Symbol,
    pub annotations: Vec<Symbol>,
}

pub struct Lens<S, A> {
    pub get: unsafe fn(*const S) -> *const A,
    pub set: unsafe fn(*mut S, A),
}

pub struct Prism<S, A> {
    pub try_get: unsafe fn(*const S) -> *const A,
    pub inject: unsafe fn(A) -> S,
}

pub struct TypeMetaSoA {
    pub type_id: u32,
    pub field_count: u32,
    pub name_hashes: Vec<u64>,
    pub offsets: Vec<usize>,
    pub type_ids: Vec<u32>,
    pub getters: Vec<usize>,
    pub mph_seed: u64,
}
```

### 4.4.5 TyDatabase (Orchestrator)

```rust
pub struct TyDatabase {
    pub arena: TyArena,
    pub interner: Interner,
    pub macro_executor: MacroExecutor,
    pub macro_registry: RwLock<MacroRegistry>,
    pub hir_store: RwLock<HashMap<PathBuf, Arc<glyim_hir::Module>>>,
    pub query_ctx: QueryContext,
}

impl TyDatabase {
    pub fn check_module(&mut self, hir: &Hir) -> Result<TypeCheckOutput, Vec<TypeError>> {
        // 1. Registration: Walk HIR items, allocate Ty, register CHR rules, build Reps
        // 2. Elaboration: check/synth all items; expand macros; evaluate comptime
        // 3. Solving: Run CHR solver to fixed point
        // 4. Freeze: Resolve Ty → HirType; generate reflection metadata
        // 5. Output: Construct TypeCheckOutput
    }
}
```

### 4.4.6 Freeze Phase

```rust
pub fn resolve_expr_types(
    arena: &TyArena,
    unification: &UnificationTable,
    elab_map: &HashMap<ExprId, Ty>,
) -> Vec<HirType> {
    let max_id = elab_map.keys().map(|id| id.as_usize()).max().unwrap_or(0);
    let mut expr_types = vec![HirType::Error; max_id + 1];
    for (id, ty) in elab_map {
        let resolved = unification.find(arena, *ty);
        expr_types[id.as_usize()] = resolve_ty(arena, unification, resolved);
    }
    expr_types
}

fn resolve_ty(arena: &TyArena, uni: &UnificationTable, ty: Ty) -> HirType {
    let ty = uni.find(arena, ty);
    match arena.get(ty) {
        TyKind::Int => HirType::Int,
        TyKind::Named(sym) => HirType::Named(*sym),
        TyKind::App(sym, args) => HirType::Generic(
            *sym,
            args.iter().map(|a| resolve_ty(arena, uni, *a)).collect(),
        ),
        TyKind::Infer => HirType::Error, // Unresolved hole
        TyKind::Error => HirType::Error,
        // ... other mappings
        _ => HirType::Error,
    }
}
```

## 4.5 Key Sequence: Type Checking a Method Call with Type-State

```
User writes: file.close()

1. Elaborator::synth_expr(MethodCall { receiver: file, method: "close" })
2. Synthesize receiver type → TyKind::App(file_sym, [Open])
3. Create inference var for result → ?0
4. Emit Goals:
   - Goal::TraitImpl(close_sym, [File<Open>, ?0])      // Standard trait
   - Goal::StateTransition(close_sym, File<Open>, ?1)    // Type-state
5. CHR Solver:
   - Matches StateTransition rule for File
   - Rule: StateTransition(close, File<Open>, File<Closed>)
   - Proves goal; unifies ?1 = File<Closed>
6. Unification: unify(?0, File<Closed>) → Success
7. Freeze: resolve ?0 → HirType::Generic(file_sym, [Closed])
8. Output: expr_types[file_expr_id] = File<Closed>
```

## 4.6 Key Sequence: Macro Expansion with Query Integration

```
User writes: @derive(Serialize) struct Foo { name: Str, age: Int }

1. Registration:
   - Alloc TyKind::Named(foo_sym)
   - Register CHR rule: Reflectable(foo_sym) → true
   - Build Rep for Foo (since @derive implies @reflectable)
   - Register macro "derive_Serialize" in MacroRegistry

2. Elaboration:
   - Encounter macro call derive_Serialize(Foo)
   - Query cache: compute fingerprint(derive_Serialize, Foo_rep_hash)
   - Cache MISS → execute via MacroExecutor
   - Serialize Rep to bytes via wasm_interface::serialize_expr
   - Call MacroExecutor::execute(wasm, input_bytes)
   - MacroExecutor checks its own CAS cache (via ContentStore)
   - CAS MISS → run Wasm with 1M fuel
   - Wasm macro calls back into MacroContext::get_fields(foo_sym)
   - MacroContext records Dependency::query(...) for invalidation
   - Wasm returns serialized output bytes
   - MacroExecutor stores in CAS
   - Deserialize output bytes → generated HIR items
   - Store result in QueryContext with dependencies
   - Add generated items to output

3. Next compilation (incremental):
   - Source unchanged → Query cache HIT → return cached result
   - Source changed (Foo renamed) → Query cache INVALIDATED
     → MacroExecutor CAS MISS (different input hash) → re-execute
```

## 4.7 Directory Structure

```text
glyim-typeck/src/
├── lib.rs                       // TypeChecker API, TypeCheckOutput
├── db.rs                        // TyDatabase orchestrator
├── ty.rs                        // Ty, TyKind, TyArena, ValueId, EffectRow
├── unify.rs                     // UnificationTable, ErrorGuaranteed, occurs check
├── chr.rs                       // Goal, ChrRule, ChrStore
│
├── elab/
│   ├── mod.rs                   // ElabContext, elaborate_item
│   ├── check.rs                 // check_expr (bidirectional check mode)
│   ├── synth.rs                 // synth_expr (bidirectional synth mode)
│   ├── scope.rs                 // Scope struct, variable bindings
│   ├── effects.rs               // Effect row unification
│   └── reflect.rs               // Reflection expression lowering
│
├── rep/
│   ├── mod.rs                   // Rep, RepMeta, FieldInfo
│   ├── optics.rs                // Lens, Prism generation from Rep
│   └── mph.rs                   // Minimal perfect hash computation
│
├── reflect/
│   ├── mod.rs                   // TypeMetaSoA, OpticDispatchTable
│   └── layout.rs                // SoA layout computation
│
├── diagnostics/
│   ├── mod.rs                   // TypeError enum (miette integration)
│   ├── zippering.rs             // Structural diffing
│   └── biabduction.rs           // Autofix synthesis
│
├── freeze.rs                    // resolve_ty, freeze_module
│
├── queries/
│   ├── mod.rs                   // Thread-local dep collector, query helpers
│   └── keys.rs                  // QueryKey implementations
│
└── tests/
    ├── unit_unify.rs            // Pure math tests for UnificationTable
    ├── unit_chr.rs              // Pure logic tests for CHR solver
    ├── unit_freeze.rs           // Ty → HirType translation tests
    ├── integration_elab.rs      // Full elaboration tests
    ├── integration_reflect.rs   // Reflection + optics tests
    ├── integration_macro.rs     // Macro expansion integration tests
    ├── query_integration.rs     // Query caching + invalidation tests
    └── snapshot_errors.rs       // Insta snapshots for miette errors
```

---

# Part V: Behavioral Specification & Test Verification

## 5.1 Test Strategy

| Level | Type | Scope | Tool |
|---|---|---|---|
| Unit | Unification, CHR solving, Freeze translation | Single functions | `#[test]` |
| Integration | Elaboration, Reflection, Macro expansion | Full typecheck module | `#[test]` + temp files |
| Snapshot | Error messages | Diagnostic output | `insta` |
| End-to-end | Full pipeline compatibility | Source → exit code | `glyim-cli-tests-full` |

## 5.2 Acceptance Criteria (Key Scenarios)

### AC-01: Type Inference for Method Call

```gherkin
Feature: Method call type inference

  Scenario: Method call on concrete type
    Given a struct File with method close()
    And a variable file of type File<Open>
    When the user writes file.close()
    Then the method call has type File<Closed>
    And the call_type_args entry contains [File<Open>]
```

### AC-02: CHR Trait Resolution

```gherkin
Feature: Conditional trait implementation

  Scenario: Conditional impl is resolved
    Given an impl Display for Vec<T> where T: Display
    And a type Vec<Int> where Int: Display
    When the system checks if Vec<Int> implements Display
    Then the goal TraitImpl(Display, [Vec<Int>]) is proven
```

### AC-03: Hole-Driven Development

```gherkin
Feature: Hole expressions

  Scenario: Hole with known expected type
    Given a function expecting i64
    When the user writes _
    Then the hole is resolved to i64
    And the span of the hole is recorded for LSP
```

### AC-04: Reflection Field Access

```gherkin
Feature: Reflective field access

  Scenario: Static field access on reflectable type
    Given a @reflectable struct User { name: Str, age: Int }
    And a variable user of type User
    When the user writes user.reflect_get("age")
    Then the result type is Int
    And a HasField(User, "age") goal is proven
```

### AC-05: Macro Expansion with Caching

```gherkin
Feature: Macro expansion caching

  Scenario: First expansion (cache miss)
    Given a derive(Serialize) macro
    When the macro is invoked on struct Foo
    Then the Wasm executor is called
    And the result is cached with fingerprint(derive_Serialize, Foo_rep_hash)

  Scenario: Second expansion (cache hit)
    Given the same derive(Serialize) macro on the same struct Foo
    When the macro is invoked
    Then the Wasm executor is NOT called
    And the cached result is returned
```

### AC-06: Incremental Invalidation

```gherkin
Feature: Incremental query invalidation

  Scenario: Unrelated file change preserves cache
    Given a fully typechecked module A
    When a comment in A is changed (no semantic change)
    Then all query results remain Green

  Scenario: Semantic change invalidates dependents
    Given module A defines struct Foo
    And module B imports and uses Foo
    When field "age" is added to Foo
    Then queries for B are invalidated (Red)
    And queries for A are invalidated (Red)
```

### AC-07: Error Recovery

```gherkin
Feature: Error recovery via ErrorGuaranteed

  Scenario: Type error does not cascade
    Given a function f(x: Int)
    When the user writes f("string")
    Then a type mismatch error is emitted
    And the call expression is assigned type Error
    And subsequent expressions using the result are typechecked without cascading errors
```

### AC-08: Phase Violation Detection

```gherkin
Feature: Phase consistency for staging

  Scenario: Runtime value used at comptime
    Given a comptime block
    And a runtime variable x
    When the comptime block references x
    Then a PhaseViolation error is emitted
    And the error reports the used-at and defined-at levels
```

## 5.3 Test Case Specifications

| TC ID | Requirement | Test Type | Description |
|---|---|---|---|
| TC-01 | FR-01 | Unit | IntLit, BoolLit, StringLit synthesis |
| TC-02 | FR-01 | Integration | Binary op with inference variables |
| TC-03 | FR-05, FR-10 | Integration | Method call with CHR trait resolution |
| TC-04 | FR-09 | Unit | Infinite type detection |
| TC-05 | FR-08 | Unit | Error type unification |
| TC-06 | FR-11 | Unit | CHR rule registration from impl |
| TC-07 | FR-12, FR-13 | Unit | CHR Simplify and Propagate rule firing |
| TC-08 | FR-14 | Unit | CHR goal failure |
| TC-09 | FR-17 | Integration | @reflectable annotation generates Rep |
| TC-10 | FR-18, FR-23 | Integration | Reflective getField with known type |
| TC-11 | FR-24, FR-25 | Integration | Comptime/macro expansion via Wasm |
| TC-12 | FR-26 | Integration | Fuel exhaustion error |
| TC-13 | FR-27 | Integration | Generated items in output |
| TC-14 | FR-28 | Integration | Macro CAS cache hit |
| TC-15 | FR-35 | Snapshot | Structural diff for nested generic mismatch |
| TC-16 | FR-36 | Snapshot | Bi-abductive autofix (wrap in Some/Ok) |
| TC-17 | FR-40, FR-41 | Integration | Query caching and invalidation |
| TC-18 | BR-01 | E2E | Full pipeline compatibility (existing test suite) |
| TC-19 | NFR-REL-01 | Fuzz | Random programs don't panic |
| TC-20 | FR-31, FR-32, FR-33 | Integration | Staging: quote, splice, phase violation |

## 5.4 Verification Methods

| Requirement Category | Primary Verification | Secondary Verification |
|---|---|---|
| Type inference correctness | Test (automated) | Analysis (type system formalization) |
| CHR solver correctness | Test (automated) | Analysis (fixed-point proof) |
| Error recovery | Test (automated) | Demonstration (error audit) |
| Performance targets | Test (benchmark suite) | Analysis (complexity analysis) |
| Pipeline compatibility | Test (E2E integration) | Inspection (output comparison) |
| Macro sandboxing | Test (fuel exhaustion) | Analysis (Wasm security model) |
| Incremental correctness | Test (invalidation) | Analysis (dependency graph soundness) |

## 5.5 Requirements Traceability Matrix (Subset)

| Business Goal | Business Req | System Req | Test Case |
|---|---|---|---|
| BG-01: Pipeline compatibility | BR-01 | FR-01 through FR-09 | TC-18 |
| BG-02: Error quality | BR-02 | FR-35, FR-36 | TC-15, TC-16 |
| BG-03: Metaprogramming | BR-03 | FR-17 through FR-30 | TC-09 through TC-14 |
| BG-04: Incremental perf | BR-04 | FR-40, FR-41, NFR-PERF-02 | TC-17 |
| BG-05: Reliability | BR-05 | NFR-REL-01, NFR-REL-02 | TC-05, TC-19 |

---

# Appendices

## Appendix A: Glossary

See §2.3 Domain Model and Ubiquitous Language.

## Appendix B: TBD / Open Issues Log

| TBD ID | Description | Owner | Due |
|---|---|---|---|
| TBD-01 | Maximum fuel budget for comptime blocks | Compiler team | Before V3 alpha |
| TBD-02 | Whether to expose `Code<T>` to users in V3 | Language design | Before V3 beta |
| TBD-03 | MPHF algorithm choice (CHD vs Brz) | Compiler team | Before V3 alpha |
| TBD-04 | Maximum field count for SIMD linear scan vs MPHF | Compiler team | Before V3 beta |
| TBD-05 | Cross-module incremental recompilation scope | Compiler team | V4 planning |

## Appendix C: Change Log

| Version | Date | Author | Changes |
|---|---|---|---|
| 1.0.0-draft | 2025-06-23 | Compiler team | Initial draft |
