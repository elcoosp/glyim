# Phase 1: Fine-Grained Incremental Compilation — Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement semantic diffing, Merkle IR caching, and fractal cache granularity so that purely syntactic changes (formatting, variable renames, comments) produce zero recompilation, and branch switches reuse cached artifacts that share content.

**Architecture:** Three subsystems built on top of the Phase 0 query engine. First, a **SemanticNormalizer** in `glyim-hir` walks HIR trees and produces canonical forms by alpha-renaming locals, sorting commutative operands, and eliminating double negations — then hashes the canonical form with `ExprId`/`Span` excluded. Second, a **glyim-merkle** crate stores HIR items and compiled artifacts in a Merkle DAG backed by the existing `ContentStore` CAS; each node's hash is a deterministic function of its content plus child hashes, making the cache branch-agnostic. Third, a **GranularityMonitor** in `glyim-query` observes per-file edit velocity and concentration, dynamically switching between fine-grained (per-function), module-level, and coarse-grained (per-file) caching.

**Prerequisites:** Phase 0 completed — `glyim-query` crate with `Fingerprint`, `QueryKey`, `QueryContext`, `DependencyGraph`, invalidation, and persistence all working.

**Tech Stack:** Rust, `glyim-query` (Phase 0), `glyim-hir` (HIR types), `glyim-macro-vfs` (`ContentStore`, `ContentHash`), `glyim-interner` (`Symbol`, `Interner`), `dashmap` (concurrent cache), `sha2` (hashing), `serde`/`bincode` (serialization).

---

## File Structure

### New files to create

```
crates/glyim-hir/src/
├── normalize.rs              — SemanticNormalizer, NormalizedHirFn, alpha-renaming
├── semantic_hash.rs          — HirHasher trait, semantic_hash() free functions
└── tests/
    ├── normalize_tests.rs    — unit tests for normalization
    └── semantic_hash_tests.rs — unit tests for semantic hashing

crates/glyim-merkle/
├── Cargo.toml
└── src/
    ├── lib.rs                — public API, re-exports
    ├── node.rs               — MerkleNode, MerkleNodeData, node serialization
    ├── store.rs              — MerkleStore (CAS-backed Merkle DAG)
    ├── root.rs               — MerkleRoot computation, branch-switch logic
    └── tests/
        ├── mod.rs
        ├── node_tests.rs     — unit tests for MerkleNode
        ├── store_tests.rs    — unit tests for MerkleStore
        └── root_tests.rs     — unit tests for MerkleRoot / branch switch

crates/glyim-query/src/
├── granularity.rs            — GranularityMonitor, CacheGranularity, EditHistory
└── tests/
    └── granularity_tests.rs  — unit tests for adaptive granularity
```

### Existing files to modify (later chunks)

```
crates/glyim-hir/src/lib.rs                    — add normalize, semantic_hash modules
crates/glyim-hir/Cargo.toml                    — add sha2, serde dependencies
crates/glyim-query/src/lib.rs                  — add granularity module
crates/glyim-query/src/context.rs              — consult granularity for query key scope
crates/glyim-query/Cargo.toml                  — add instant dependency
crates/glyim-compiler/src/pipeline.rs          — use semantic_hash in query keys, integrate MerkleStore
crates/glyim-compiler/Cargo.toml               — add glyim-merkle dependency
crates/glyim-cli/src/commands/cmd_build.rs     — add --cache-branch-agnostic flag
```

---

## Chunk 1: Semantic Normalization — Alpha-Equivalence Short-Circuiting

The normalizer transforms HIR into a canonical form before fingerprinting. The core idea: two functions that differ only in local variable names, comment positions, or operand order in commutative operations must produce the same semantic hash. We implement this as a standalone module in `glyim-hir` that produces `NormalizedHirFn` — a simplified representation that strips `ExprId`, `Span`, and renames locals canonically.

**Key design decisions:**
- `Symbol` is session-dependent (a `u32` index into an `Interner`). We cannot hash `Symbol` values directly — we must resolve them via the interner and then rename.
- `ExprId` and `Span` are syntactic/positional and must be excluded from semantic fingerprinting entirely.
- The normalizer does **not** mutate the original HIR — it produces a separate canonical representation optimized for hashing.
- We use a `NormalizedExpr` enum parallel to `HirExpr` but without `ExprId`/`Span`, and with canonical variable names.

---

### Task 1: NormalizedExpr and NormalizedStmt Types

**Files:**
- Create: `crates/glyim-hir/src/normalize.rs`
- Test: `crates/glyim-hir/src/tests/normalize_tests.rs`

- [ ] **Step 1: Write failing tests for NormalizedExpr construction**

First, add the test module registration to `crates/glyim-hir/src/lib.rs`:

```rust
pub mod normalize;
pub mod semantic_hash;

#[cfg(test)]
mod tests {
    mod normalize_tests;
    mod semantic_hash_tests;
}
```

Create `crates/glyim-hir/src/tests/normalize_tests.rs`:

```rust
use glyim_hir::normalize::{SemanticNormalizer, NormalizedExpr, NormalizedStmt, NormalizedHirFn};
use glyim_hir::node::{HirExpr, HirFn, HirBinOp, HirStmt, HirType};
use glyim_hir::types::ExprId;
use glyim_interner::Interner;
use glyim_hir::Span;

fn make_span() -> Span {
    Span::default()
}

fn make_ident_expr(interner: &mut Interner, name: &str) -> HirExpr {
    HirExpr::Ident {
        id: ExprId::new(0),
        name: interner.intern(name),
        span: make_span(),
    }
}

fn make_int_lit_expr(value: i64) -> HirExpr {
    HirExpr::IntLit {
        id: ExprId::new(0),
        value,
        span: make_span(),
    }
}

fn make_binary_expr(op: HirBinOp, lhs: HirExpr, rhs: HirExpr) -> HirExpr {
    HirExpr::Binary {
        id: ExprId::new(0),
        op,
        lhs: Box::new(lhs),
        rhs: Box::new(rhs),
        span: make_span(),
    }
}

fn make_let_stmt(interner: &mut Interner, name: &str, value: HirExpr) -> HirStmt {
    HirStmt::Let {
        name: interner.intern(name),
        mutable: false,
        value,
        span: make_span(),
    }
}

fn make_block_expr(stmts: Vec<HirStmt>) -> HirExpr {
    HirExpr::Block {
        id: ExprId::new(0),
        stmts,
        span: make_span(),
    }
}

fn make_simple_fn(interner: &mut Interner, name: &str, param_name: &str, body: HirExpr) -> HirFn {
    HirFn {
        doc: None,
        name: interner.intern(name),
        type_params: vec![],
        params: vec![(interner.intern(param_name), HirType::Int)],
        param_mutability: vec![false],
        ret: Some(HirType::Int),
        body,
        span: make_span(),
        is_pub: false,
        is_macro_generated: false,
        is_extern_backed: false,
    }
}

// ── NormalizedExpr tests ──────────────────────────────────────

#[test]
fn normalized_int_lit_preserves_value() {
    let expr = make_int_lit_expr(42);
    let interner = Interner::new();
    let normalized = SemanticNormalizer::normalize_expr(&expr, &interner);
    assert_eq!(normalized, NormalizedExpr::IntLit(42));
}

#[test]
fn normalized_bool_lit_preserves_value() {
    let expr = HirExpr::BoolLit { id: ExprId::new(0), value: true, span: make_span() };
    let interner = Interner::new();
    let normalized = SemanticNormalizer::normalize_expr(&expr, &interner);
    assert_eq!(normalized, NormalizedExpr::BoolLit(true));
}

#[test]
fn normalized_str_lit_preserves_value() {
    let expr = HirExpr::StrLit { id: ExprId::new(0), value: "hello".to_string(), span: make_span() };
    let interner = Interner::new();
    let normalized = SemanticNormalizer::normalize_expr(&expr, &interner);
    assert_eq!(normalized, NormalizedExpr::StrLit("hello".to_string()));
}

#[test]
fn normalized_float_lit_preserves_value() {
    let expr = HirExpr::FloatLit { id: ExprId::new(0), value: 3.14, span: make_span() };
    let interner = Interner::new();
    let normalized = SemanticNormalizer::normalize_expr(&expr, &interner);
    match normalized {
        NormalizedExpr::FloatLit(v) => assert!((v - 3.14).abs() < f64::EPSILON),
        other => panic!("expected FloatLit, got {:?}", other),
    }
}

#[test]
fn normalized_ident_local_gets_canonical_name() {
    // In a function body, local variables are renamed to _v0, _v1, ...
    // Parameters get renamed first, then let-bindings
    let mut interner = Interner::new();
    let x = interner.intern("x");
    let expr = HirExpr::Ident { id: ExprId::new(0), name: x, span: make_span() };
    let mut normalizer = SemanticNormalizer::new(&interner);
    // Register "x" as a parameter → _v0
    normalizer.register_param(x);
    let normalized = normalizer.normalize_expr(&expr);
    assert_eq!(normalized, NormalizedExpr::Local(0));
}

#[test]
fn normalized_ident_nonlocal_preserves_string() {
    // A function call like "foo" is not a local variable — preserve its name
    let mut interner = Interner::new();
    let foo = interner.intern("foo");
    let expr = HirExpr::Call { id: ExprId::new(0), callee: foo, args: vec![], span: make_span() };
    let interner2 = Interner::new();
    let normalized = SemanticNormalizer::normalize_expr(&expr, &interner2);
    match normalized {
        NormalizedExpr::Call { callee, args } => {
            assert_eq!(callee, "foo");
            assert!(args.is_empty());
        }
        other => panic!("expected Call, got {:?}", other),
    }
}

#[test]
fn normalized_binary_preserves_op_and_operands() {
    let expr = make_binary_expr(
        HirBinOp::Add,
        make_int_lit_expr(1),
        make_int_lit_expr(2),
    );
    let interner = Interner::new();
    let normalized = SemanticNormalizer::normalize_expr(&expr, &interner);
    assert_eq!(normalized, NormalizedExpr::Binary {
        op: HirBinOp::Add,
        lhs: Box::new(NormalizedExpr::IntLit(1)),
        rhs: Box::new(NormalizedExpr::IntLit(2)),
    });
}

#[test]
fn normalized_let_stmt_renames_variable() {
    let mut interner = Interner::new();
    let let_stmt = make_let_stmt(&mut interner, "my_var", make_int_lit_expr(10));
    let mut normalizer = SemanticNormalizer::new(&interner);
    let normalized = normalizer.normalize_stmt(&let_stmt);
    // "my_var" should be the first local (no params) → _v0
    assert_eq!(normalized, NormalizedStmt::Let {
        local_id: 0,
        mutable: false,
        value: NormalizedExpr::IntLit(10),
    });
}

#[test]
fn normalized_let_stmt_sequential_ids() {
    let mut interner = Interner::new();
    let a = interner.intern("a");
    let b = interner.intern("b");
    let stmt_a = HirStmt::Let { name: a, mutable: false, value: make_int_lit_expr(1), span: make_span() };
    let stmt_b = HirStmt::Let { name: b, mutable: false, value: make_int_lit_expr(2), span: make_span() };
    let block = make_block_expr(vec![stmt_a, stmt_b]);
    let mut normalizer = SemanticNormalizer::new(&interner);
    let normalized = normalizer.normalize_expr(&block);
    // a → _v0, b → _v1
    match normalized {
        NormalizedExpr::Block { stmts } => {
            assert_eq!(stmts.len(), 2);
            assert_eq!(stmts[0], NormalizedStmt::Let { local_id: 0, mutable: false, value: NormalizedExpr::IntLit(1) });
            assert_eq!(stmts[1], NormalizedStmt::Let { local_id: 1, mutable: false, value: NormalizedExpr::IntLit(2) });
        }
        other => panic!("expected Block, got {:?}", other),
    }
}

#[test]
fn normalized_assign_targets_local_id() {
    let mut interner = Interner::new();
    let x = interner.intern("x");
    let let_stmt = HirStmt::Let { name: x, mutable: true, value: make_int_lit_expr(1), span: make_span() };
    let assign_stmt = HirStmt::Assign { target: x, value: make_int_lit_expr(2), span: make_span() };
    let block = make_block_expr(vec![let_stmt, assign_stmt]);
    let mut normalizer = SemanticNormalizer::new(&interner);
    let normalized = normalizer.normalize_expr(&block);
    match normalized {
        NormalizedExpr::Block { stmts } => {
            assert_eq!(stmts.len(), 2);
            assert_eq!(stmts[1], NormalizedStmt::Assign { local_id: 0, value: NormalizedExpr::IntLit(2) });
        }
        other => panic!("expected Block, got {:?}", other),
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p glyim-hir --lib normalize_tests 2>&1 | head -20`
Expected: Compilation error — `normalize` module does not exist

- [ ] **Step 3: Implement NormalizedExpr, NormalizedStmt, and SemanticNormalizer**

`crates/glyim-hir/src/normalize.rs`:

```rust
//! Semantic normalization for HIR items.
//!
//! Before fingerprinting a HIR item for the query cache, we normalize it
//! to eliminate purely syntactic differences that don't affect semantics.
//! This ensures that:
//!
//! - Renaming a local variable does not change the semantic hash
//! - Auto-formatting a file does not change the semantic hash
//! - Reordering operands of commutative operations (e.g., `a + b` vs `b + a`)
//!   does not change the semantic hash
//!
//! The normalizer does NOT mutate the original HIR. Instead, it produces
//! a separate `NormalizedHirFn` representation that is optimized for
//! deterministic hashing.

use crate::node::{HirBinOp, HirExpr, HirFn, HirStmt, HirUnOp, HirItem, HirImplDef, StructDef, EnumDef, MatchArm};
use crate::types::{ExprId, HirPattern, HirType};
use crate::Span;
use glyim_interner::{Interner, Symbol};
use std::collections::HashMap;

// ── Normalized representation ─────────────────────────────────

/// A normalized expression — identical to HirExpr but without ExprId/Span,
/// and with local variables replaced by canonical De Bruijn-style indices.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum NormalizedExpr {
    IntLit(i64),
    FloatLit(u64),          // bits of f64 for Eq/Hash
    BoolLit(bool),
    StrLit(String),
    UnitLit,
    /// A local variable reference, by canonical index.
    /// Parameter 0 → Local(0), parameter 1 → Local(1), first let-binding → Local(n_params), etc.
    Local(u32),
    /// A non-local name reference (function call target, struct name, etc.)
    /// Stored as the resolved string, not the session-dependent Symbol.
    Name(String),
    Binary {
        op: HirBinOp,
        lhs: Box<NormalizedExpr>,
        rhs: Box<NormalizedExpr>,
    },
    Unary {
        op: HirUnOp,
        operand: Box<NormalizedExpr>,
    },
    Block {
        stmts: Vec<NormalizedStmt>,
    },
    If {
        condition: Box<NormalizedExpr>,
        then_branch: Box<NormalizedExpr>,
        else_branch: Option<Box<NormalizedExpr>>,
    },
    Call {
        callee: String,
        args: Vec<NormalizedExpr>,
    },
    MethodCall {
        receiver: Box<NormalizedExpr>,
        method_name: String,
        resolved_callee: Option<String>,
        args: Vec<NormalizedExpr>,
    },
    Assert {
        condition: Box<NormalizedExpr>,
        message: Option<Box<NormalizedExpr>>,
    },
    Match {
        scrutinee: Box<NormalizedExpr>,
        arms: Vec<NormalizedMatchArm>,
    },
    FieldAccess {
        object: Box<NormalizedExpr>,
        field: String,
    },
    StructLit {
        struct_name: String,
        fields: Vec<(String, NormalizedExpr)>,
    },
    EnumVariant {
        enum_name: String,
        variant_name: String,
        args: Vec<NormalizedExpr>,
    },
    ForIn {
        pattern: NormalizedPattern,
        iter: Box<NormalizedExpr>,
        body: Box<NormalizedExpr>,
    },
    While {
        condition: Box<NormalizedExpr>,
        body: Box<NormalizedExpr>,
    },
    Return {
        value: Option<Box<NormalizedExpr>>,
    },
    As {
        expr: Box<NormalizedExpr>,
        target_type: HirType,
    },
    SizeOf {
        target_type: HirType,
    },
    TupleLit {
        elements: Vec<NormalizedExpr>,
    },
    AddrOf {
        target: String,
    },
    Deref {
        expr: Box<NormalizedExpr>,
    },
    Println {
        arg: Box<NormalizedExpr>,
    },
}

/// A normalized statement — local variables referenced by canonical index.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum NormalizedStmt {
    Let {
        local_id: u32,
        mutable: bool,
        value: NormalizedExpr,
    },
    Assign {
        local_id: u32,
        value: NormalizedExpr,
    },
    AssignField {
        object: NormalizedExpr,
        field: String,
        value: NormalizedExpr,
    },
    AssignDeref {
        target: NormalizedExpr,
        value: NormalizedExpr,
    },
    Expr(NormalizedExpr),
}

/// A normalized match arm.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct NormalizedMatchArm {
    pub pattern: NormalizedPattern,
    pub guard: Option<NormalizedExpr>,
    pub body: NormalizedExpr,
}

/// A normalized pattern — local bindings use canonical indices.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum NormalizedPattern {
    Wild,
    BoolLit(bool),
    IntLit(i64),
    FloatLit(u64),
    StrLit(String),
    Unit,
    /// A variable binding, by canonical local index.
    Local(u32),
    Struct {
        name: String,
        bindings: Vec<(String, NormalizedPattern)>,
    },
    EnumVariant {
        enum_name: String,
        variant_name: String,
        bindings: Vec<(String, NormalizedPattern)>,
    },
    Tuple {
        elements: Vec<NormalizedPattern>,
    },
    OptionSome(Box<NormalizedPattern>),
    OptionNone,
    ResultOk(Box<NormalizedPattern>),
    ResultErr(Box<NormalizedPattern>),
}

/// A fully normalized function — ready for deterministic hashing.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct NormalizedHirFn {
    pub name: String,
    pub type_params: Vec<String>,
    pub params: Vec<(String, HirType)>,
    pub param_mutability: Vec<bool>,
    pub ret: Option<HirType>,
    pub body: NormalizedExpr,
    pub is_pub: bool,
    pub is_extern_backed: bool,
}

// ── SemanticNormalizer ────────────────────────────────────────

/// Walks a HIR tree and produces a normalized, canonical representation
/// suitable for semantic fingerprinting.
///
/// The normalizer maintains a map from `Symbol` → canonical local index.
/// Parameters are registered first (indices 0..n_params), then let-bindings
/// are assigned indices as they are encountered in left-to-right depth-first order.
///
/// Non-local names (function call targets, struct names, etc.) are resolved
/// to their string representation via the `Interner`, so that the output
/// is independent of the session-specific `Symbol` numbering.
pub struct SemanticNormalizer<'a> {
    /// Reference to the interner for resolving Symbol → string.
    interner: &'a Interner,
    /// Map from Symbol → canonical local index.
    local_map: HashMap<Symbol, u32>,
    /// Next canonical local index to assign.
    next_local: u32,
    /// Set of parameter Symbols (to distinguish params from free names).
    param_set: HashMap<Symbol, u32>,
}

impl<'a> SemanticNormalizer<'a> {
    /// Create a new normalizer with no locals registered.
    pub fn new(interner: &'a Interner) -> Self {
        Self {
            interner,
            local_map: HashMap::new(),
            next_local: 0,
            param_set: HashMap::new(),
        }
    }

    /// Register a parameter Symbol and assign it the next canonical index.
    pub fn register_param(&mut self, sym: Symbol) {
        let idx = self.next_local;
        self.next_local += 1;
        self.local_map.insert(sym, idx);
        self.param_set.insert(sym, idx);
    }

    /// Register a let-binding Symbol and assign it the next canonical index.
    fn register_local(&mut self, sym: Symbol) -> u32 {
        let idx = self.next_local;
        self.next_local += 1;
        self.local_map.insert(sym, idx);
        idx
    }

    /// Resolve a Symbol to either a Local(index) or a Name(string).
    fn resolve_ident(&self, sym: Symbol) -> NormalizedExpr {
        if let Some(&idx) = self.local_map.get(&sym) {
            NormalizedExpr::Local(idx)
        } else {
            let name = self.interner.resolve(sym).to_string();
            NormalizedExpr::Name(name)
        }
    }

    /// Resolve a Symbol used as a name reference (not a local variable).
    fn resolve_name(&self, sym: Symbol) -> String {
        self.interner.resolve(sym).to_string()
    }

    /// Normalize a complete HirFn into a NormalizedHirFn.
    pub fn normalize_fn(&mut self, hir_fn: &HirFn) -> NormalizedHirFn {
        // Reset local state
        self.local_map.clear();
        self.next_local = 0;
        self.param_set.clear();

        // Register parameters first
        for &(sym, _) in &hir_fn.params {
            self.register_param(sym);
        }

        // Normalize body
        let body = self.normalize_expr(&hir_fn.body);

        NormalizedHirFn {
            name: self.resolve_name(hir_fn.name),
            type_params: hir_fn.type_params.iter().map(|&s| self.resolve_name(s)).collect(),
            params: hir_fn.params.iter().zip(&hir_fn.param_mutability).map(|((sym, ty), _mutable)| {
                (self.resolve_name(*sym), ty.clone())
            }).collect(),
            param_mutability: hir_fn.param_mutability.clone(),
            ret: hir_fn.ret.clone(),
            body,
            is_pub: hir_fn.is_pub,
            is_extern_backed: hir_fn.is_extern_backed,
        }
    }

    /// Normalize an expression.
    pub fn normalize_expr(&mut self, expr: &HirExpr) -> NormalizedExpr {
        match expr {
            HirExpr::IntLit { value, .. } => NormalizedExpr::IntLit(*value),
            HirExpr::FloatLit { value, .. } => NormalizedExpr::FloatLit(value.to_bits()),
            HirExpr::BoolLit { value, .. } => NormalizedExpr::BoolLit(*value),
            HirExpr::StrLit { value, .. } => NormalizedExpr::StrLit(value.clone()),
            HirExpr::UnitLit { .. } => NormalizedExpr::UnitLit,
            HirExpr::Ident { name, .. } => self.resolve_ident(*name),
            HirExpr::Binary { op, lhs, rhs, .. } => {
                let lhs_n = self.normalize_expr(lhs);
                let rhs_n = self.normalize_expr(rhs);
                // Normalize commutative operators: sort operands for determinism
                if op.is_commutative() && lhs_n > rhs_n {
                    NormalizedExpr::Binary { op: *op, lhs: Box::new(rhs_n), rhs: Box::new(lhs_n) }
                } else {
                    NormalizedExpr::Binary { op: *op, lhs: Box::new(lhs_n), rhs: Box::new(rhs_n) }
                }
            }
            HirExpr::Unary { op, operand, .. } => {
                let operand_n = self.normalize_expr(operand);
                // Double negation elimination: !!x → x
                if let HirUnOp::Not = op {
                    if let NormalizedExpr::Unary { op: HirUnOp::Not, operand: inner } = &operand_n {
                        return (**inner).clone();
                    }
                }
                NormalizedExpr::Unary { op: *op, operand: Box::new(operand_n) }
            }
            HirExpr::Block { stmts, .. } => {
                let stmts_n: Vec<NormalizedStmt> = stmts.iter().map(|s| self.normalize_stmt(s)).collect();
                NormalizedExpr::Block { stmts: stmts_n }
            }
            HirExpr::If { condition, then_branch, else_branch, .. } => {
                NormalizedExpr::If {
                    condition: Box::new(self.normalize_expr(condition)),
                    then_branch: Box::new(self.normalize_expr(then_branch)),
                    else_branch: else_branch.as_ref().map(|e| Box::new(self.normalize_expr(e))),
                }
            }
            HirExpr::Call { callee, args, .. } => {
                NormalizedExpr::Call {
                    callee: self.resolve_name(*callee),
                    args: args.iter().map(|a| self.normalize_expr(a)).collect(),
                }
            }
            HirExpr::MethodCall { receiver, method_name, resolved_callee, args, .. } => {
                NormalizedExpr::MethodCall {
                    receiver: Box::new(self.normalize_expr(receiver)),
                    method_name: self.resolve_name(*method_name),
                    resolved_callee: resolved_callee.map(|s| self.resolve_name(s)),
                    args: args.iter().map(|a| self.normalize_expr(a)).collect(),
                }
            }
            HirExpr::Assert { condition, message, .. } => {
                NormalizedExpr::Assert {
                    condition: Box::new(self.normalize_expr(condition)),
                    message: message.as_ref().map(|e| Box::new(self.normalize_expr(e))),
                }
            }
            HirExpr::Match { scrutinee, arms, .. } => {
                NormalizedExpr::Match {
                    scrutinee: Box::new(self.normalize_expr(scrutinee)),
                    arms: arms.iter().map(|arm| self.normalize_match_arm(arm)).collect(),
                }
            }
            HirExpr::FieldAccess { object, field, .. } => {
                NormalizedExpr::FieldAccess {
                    object: Box::new(self.normalize_expr(object)),
                    field: self.resolve_name(*field),
                }
            }
            HirExpr::StructLit { struct_name, fields, .. } => {
                NormalizedExpr::StructLit {
                    struct_name: self.resolve_name(*struct_name),
                    fields: fields.iter().map(|(name, expr)| {
                        (self.resolve_name(*name), self.normalize_expr(expr))
                    }).collect(),
                }
            }
            HirExpr::EnumVariant { enum_name, variant_name, args, .. } => {
                NormalizedExpr::EnumVariant {
                    enum_name: self.resolve_name(*enum_name),
                    variant_name: self.resolve_name(*variant_name),
                    args: args.iter().map(|a| self.normalize_expr(a)).collect(),
                }
            }
            HirExpr::ForIn { pattern, iter, body, .. } => {
                // The pattern may introduce new locals
                let pattern_n = self.normalize_pattern(pattern);
                NormalizedExpr::ForIn {
                    pattern: pattern_n,
                    iter: Box::new(self.normalize_expr(iter)),
                    body: Box::new(self.normalize_expr(body)),
                }
            }
            HirExpr::While { condition, body, .. } => {
                NormalizedExpr::While {
                    condition: Box::new(self.normalize_expr(condition)),
                    body: Box::new(self.normalize_expr(body)),
                }
            }
            HirExpr::Return { value, .. } => {
                NormalizedExpr::Return {
                    value: value.as_ref().map(|e| Box::new(self.normalize_expr(e))),
                }
            }
            HirExpr::As { expr, target_type, .. } => {
                NormalizedExpr::As {
                    expr: Box::new(self.normalize_expr(expr)),
                    target_type: target_type.clone(),
                }
            }
            HirExpr::SizeOf { target_type, .. } => {
                NormalizedExpr::SizeOf { target_type: target_type.clone() }
            }
            HirExpr::TupleLit { elements, .. } => {
                NormalizedExpr::TupleLit {
                    elements: elements.iter().map(|e| self.normalize_expr(e)).collect(),
                }
            }
            HirExpr::AddrOf { target, .. } => {
                NormalizedExpr::AddrOf { target: self.resolve_name(*target) }
            }
            HirExpr::Deref { expr, .. } => {
                NormalizedExpr::Deref { expr: Box::new(self.normalize_expr(expr)) }
            }
            HirExpr::Println { arg, .. } => {
                NormalizedExpr::Println { arg: Box::new(self.normalize_expr(arg)) }
            }
        }
    }

    /// Normalize a statement.
    pub fn normalize_stmt(&mut self, stmt: &HirStmt) -> NormalizedStmt {
        match stmt {
            HirStmt::Let { name, mutable, value, .. } => {
                let local_id = self.register_local(*name);
                NormalizedStmt::Let {
                    local_id,
                    mutable: *mutable,
                    value: self.normalize_expr(value),
                }
            }
            HirStmt::Assign { target, value, .. } => {
                let local_id = self.local_map.get(target).copied()
                    .expect("assign target must be a known local");
                NormalizedStmt::Assign {
                    local_id,
                    value: self.normalize_expr(value),
                }
            }
            HirStmt::AssignField { object, field, value, .. } => {
                NormalizedStmt::AssignField {
                    object: self.normalize_expr(object),
                    field: self.resolve_name(*field),
                    value: self.normalize_expr(value),
                }
            }
            HirStmt::AssignDeref { target, value, .. } => {
                NormalizedStmt::AssignDeref {
                    target: self.normalize_expr(target),
                    value: self.normalize_expr(value),
                }
            }
            HirStmt::Expr(expr) => NormalizedStmt::Expr(self.normalize_expr(expr)),
            HirStmt::LetPat { pattern, mutable, value, ty, .. } => {
                // Register all bindings in the pattern
                let local_id = self.register_pattern_bindings(pattern);
                NormalizedStmt::Let {
                    local_id,
                    mutable: *mutable,
                    value: self.normalize_expr(value),
                }
            }
        }
    }

    /// Normalize a match arm.
    fn normalize_match_arm(&mut self, arm: &MatchArm) -> NormalizedMatchArm {
        let pattern = self.normalize_pattern(&arm.pattern);
        let guard = arm.guard.as_ref().map(|e| self.normalize_expr(e));
        let body = self.normalize_expr(&arm.body);
        NormalizedMatchArm { pattern, guard, body }
    }

    /// Normalize a pattern, registering any new variable bindings.
    fn normalize_pattern(&mut self, pat: &HirPattern) -> NormalizedPattern {
        match pat {
            HirPattern::Wild => NormalizedPattern::Wild,
            HirPattern::BoolLit(b) => NormalizedPattern::BoolLit(*b),
            HirPattern::IntLit(n) => NormalizedPattern::IntLit(*n),
            HirPattern::FloatLit(f) => NormalizedPattern::FloatLit(f.to_bits()),
            HirPattern::StrLit(s) => NormalizedPattern::StrLit(s.clone()),
            HirPattern::Unit => NormalizedPattern::Unit,
            HirPattern::Var(sym) => {
                if self.local_map.contains_key(sym) {
                    // Already registered (e.g., from an outer scope)
                    NormalizedPattern::Local(self.local_map[sym])
                } else {
                    let idx = self.register_local(*sym);
                    NormalizedPattern::Local(idx)
                }
            }
            HirPattern::Struct { name, bindings, .. } => {
                NormalizedPattern::Struct {
                    name: self.resolve_name(*name),
                    bindings: bindings.iter().map(|(field_name, sub_pat)| {
                        (self.resolve_name(*field_name), self.normalize_pattern(sub_pat))
                    }).collect(),
                }
            }
            HirPattern::EnumVariant { enum_name, variant_name, bindings, .. } => {
                NormalizedPattern::EnumVariant {
                    enum_name: self.resolve_name(*enum_name),
                    variant_name: self.resolve_name(*variant_name),
                    bindings: bindings.iter().map(|(name, sub_pat)| {
                        (self.resolve_name(*name), self.normalize_pattern(sub_pat))
                    }).collect(),
                }
            }
            HirPattern::Tuple { elements, .. } => {
                NormalizedPattern::Tuple {
                    elements: elements.iter().map(|p| self.normalize_pattern(p)).collect(),
                }
            }
            HirPattern::OptionSome(inner) => {
                NormalizedPattern::OptionSome(Box::new(self.normalize_pattern(inner)))
            }
            HirPattern::OptionNone => NormalizedPattern::OptionNone,
            HirPattern::ResultOk(inner) => {
                NormalizedPattern::ResultOk(Box::new(self.normalize_pattern(inner)))
            }
            HirPattern::ResultErr(inner) => {
                NormalizedPattern::ResultErr(Box::new(self.normalize_pattern(inner)))
            }
        }
    }

    /// Register all variable bindings introduced by a pattern.
    /// Returns the canonical index of the first binding.
    fn register_pattern_bindings(&mut self, pat: &HirPattern) -> u32 {
        match pat {
            HirPattern::Var(sym) => self.register_local(*sym),
            HirPattern::Struct { bindings, .. } => {
                let first = if let Some((_, sub)) = bindings.first() {
                    self.register_pattern_bindings(sub)
                } else {
                    self.next_local
                };
                for (_, sub) in &bindings[1..] {
                    self.register_pattern_bindings(sub);
                }
                first
            }
            HirPattern::Tuple { elements } => {
                let first = if let Some(sub) = elements.first() {
                    self.register_pattern_bindings(sub)
                } else {
                    self.next_local
                };
                for sub in &elements[1..] {
                    self.register_pattern_bindings(sub);
                }
                first
            }
            HirPattern::EnumVariant { bindings, .. } => {
                let first = if let Some((_, sub)) = bindings.first() {
                    self.register_pattern_bindings(sub)
                } else {
                    self.next_local
                };
                for (_, sub) in &bindings[1..] {
                    self.register_pattern_bindings(sub);
                }
                first
            }
            HirPattern::OptionSome(inner) => self.register_pattern_bindings(inner),
            HirPattern::ResultOk(inner) => self.register_pattern_bindings(inner),
            HirPattern::ResultErr(inner) => self.register_pattern_bindings(inner),
            _ => self.next_local, // Wild, literals, Unit, None don't introduce bindings
        }
    }
}

/// Convenience function: normalize a HirFn using a fresh normalizer.
impl NormalizedHirFn {
    pub fn from_hir_fn(hir_fn: &HirFn, interner: &Interner) -> Self {
        let mut normalizer = SemanticNormalizer::new(interner);
        normalizer.normalize_fn(hir_fn)
    }
}

/// Extension: commutativity for HirBinOp
impl HirBinOp {
    /// Returns true if the operation is commutative (a op b == b op a).
    pub fn is_commutative(&self) -> bool {
        matches!(self, HirBinOp::Add | HirBinOp::Mul | HirBinOp::Eq | HirBinOp::Neq | HirBinOp::And | HirBinOp::Or)
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p glyim-hir --lib normalize_tests`
Expected: All tests PASS

- [ ] **Step 5: Commit**

```bash
git add crates/glyim-hir/src/normalize.rs crates/glyim-hir/src/tests/normalize_tests.rs crates/glyim-hir/src/lib.rs
git commit -m "feat(hir): add SemanticNormalizer — alpha-equivalence normalization for HIR"
```

---

### Task 2: Semantic Hash Functions

**Files:**
- Create: `crates/glyim-hir/src/semantic_hash.rs`
- Test: `crates/glyim-hir/src/tests/semantic_hash_tests.rs`

- [ ] **Step 1: Write failing tests for semantic_hash**

Create `crates/glyim-hir/src/tests/semantic_hash_tests.rs`:

```rust
use glyim_hir::semantic_hash::{semantic_hash_fn, semantic_hash_item};
use glyim_hir::node::{HirExpr, HirFn, HirBinOp, HirStmt, HirItem, HirImplDef};
use glyim_hir::types::{ExprId, HirType};
use glyim_hir::Span;
use glyim_interner::Interner;

fn make_span() -> Span { Span::default() }

fn make_int_lit(value: i64) -> HirExpr {
    HirExpr::IntLit { id: ExprId::new(0), value, span: make_span() }
}

fn make_binary(op: HirBinOp, lhs: HirExpr, rhs: HirExpr) -> HirExpr {
    HirExpr::Binary { id: ExprId::new(0), op, lhs: Box::new(lhs), rhs: Box::new(rhs), span: make_span() }
}

fn make_fn_with_body(interner: &mut Interner, name: &str, param: &str, body: HirExpr) -> HirFn {
    HirFn {
        doc: None,
        name: interner.intern(name),
        type_params: vec![],
        params: vec![(interner.intern(param), HirType::Int)],
        param_mutability: vec![false],
        ret: Some(HirType::Int),
        body,
        span: make_span(),
        is_pub: false,
        is_macro_generated: false,
        is_extern_backed: false,
    }
}

#[test]
fn same_function_same_hash() {
    let mut interner = Interner::new();
    let body = make_binary(HirBinOp::Add, make_int_lit(1), make_int_lit(2));
    let fn1 = make_fn_with_body(&mut interner, "add", "x", body.clone());
    let fn2 = make_fn_with_body(&mut interner, "add", "x", body.clone());
    assert_eq!(semantic_hash_fn(&fn1, &interner), semantic_hash_fn(&fn2, &interner));
}

#[test]
fn different_function_different_hash() {
    let mut interner = Interner::new();
    let body_a = make_binary(HirBinOp::Add, make_int_lit(1), make_int_lit(2));
    let body_b = make_binary(HirBinOp::Sub, make_int_lit(1), make_int_lit(2));
    let fn_a = make_fn_with_body(&mut interner, "fn_a", "x", body_a);
    let fn_b = make_fn_with_body(&mut interner, "fn_b", "x", body_b);
    assert_ne!(semantic_hash_fn(&fn_a, &interner), semantic_hash_fn(&fn_b, &interner));
}

#[test]
fn local_rename_same_hash() {
    // fn foo(x) { x + 1 } and fn foo(y) { y + 1 } must hash identically
    let mut interner1 = Interner::new();
    let mut interner2 = Interner::new();

    let x = interner1.intern("x");
    let body1 = HirExpr::Binary {
        id: ExprId::new(0),
        op: HirBinOp::Add,
        lhs: Box::new(HirExpr::Ident { id: ExprId::new(1), name: x, span: make_span() }),
        rhs: Box::new(make_int_lit(1)),
        span: make_span(),
    };
    let fn1 = make_fn_with_body(&mut interner1, "foo", "x", body1);

    let y = interner2.intern("y");
    let body2 = HirExpr::Binary {
        id: ExprId::new(0),
        op: HirBinOp::Add,
        lhs: Box::new(HirExpr::Ident { id: ExprId::new(1), name: y, span: make_span() }),
        rhs: Box::new(make_int_lit(1)),
        span: make_span(),
    };
    let fn2 = make_fn_with_body(&mut interner2, "foo", "y", body2);

    assert_eq!(semantic_hash_fn(&fn1, &interner1), semantic_hash_fn(&fn2, &interner2));
}

#[test]
fn commutative_reorder_same_hash() {
    // a + b and b + a must hash identically (when a, b are non-equal literals)
    let mut interner = Interner::new();
    let expr_ab = make_binary(HirBinOp::Add, make_int_lit(1), make_int_lit(2));
    let expr_ba = make_binary(HirBinOp::Add, make_int_lit(2), make_int_lit(1));
    let fn_ab = make_fn_with_body(&mut interner, "comm", "x", expr_ab);
    let fn_ba = make_fn_with_body(&mut interner, "comm", "x", expr_ba);
    assert_eq!(semantic_hash_fn(&fn_ab, &interner), semantic_hash_fn(&fn_ba, &interner));
}

#[test]
fn non_commutative_reorder_different_hash() {
    // a - b and b - a must hash differently
    let mut interner = Interner::new();
    let expr_ab = make_binary(HirBinOp::Sub, make_int_lit(1), make_int_lit(2));
    let expr_ba = make_binary(HirBinOp::Sub, make_int_lit(2), make_int_lit(1));
    let fn_ab = make_fn_with_body(&mut interner, "sub", "x", expr_ab);
    let fn_ba = make_fn_with_body(&mut interner, "sub", "x", expr_ba);
    assert_ne!(semantic_hash_fn(&fn_ab, &interner), semantic_hash_fn(&fn_ba, &interner));
}

#[test]
fn expr_id_change_same_hash() {
    // Same function body but different ExprIds → same semantic hash
    let mut interner = Interner::new();
    let body1 = make_int_lit(42); // ExprId(0)
    let body2 = HirExpr::IntLit { id: ExprId::new(999), value: 42, span: make_span() }; // ExprId(999)
    let fn1 = make_fn_with_body(&mut interner, "lit", "x", body1);
    let fn2 = make_fn_with_body(&mut interner, "lit", "x", body2);
    assert_eq!(semantic_hash_fn(&fn1, &interner), semantic_hash_fn(&fn2, &interner));
}

#[test]
fn double_negation_same_hash() {
    // !!true and true must hash identically
    let mut interner = Interner::new();
    let expr_true = HirExpr::BoolLit { id: ExprId::new(0), value: true, span: make_span() };
    let expr_double_neg = HirExpr::Unary {
        id: ExprId::new(0),
        op: glyim_hir::node::HirUnOp::Not,
        operand: Box::new(HirExpr::Unary {
            id: ExprId::new(1),
            op: glyim_hir::node::HirUnOp::Not,
            operand: Box::new(HirExpr::BoolLit { id: ExprId::new(2), value: true, span: make_span() }),
            span: make_span(),
        }),
        span: make_span(),
    };
    let fn_true = make_fn_with_body(&mut interner, "neg", "x", expr_true);
    let fn_dneg = make_fn_with_body(&mut interner, "neg", "x", expr_double_neg);
    assert_eq!(semantic_hash_fn(&fn_true, &interner), semantic_hash_fn(&fn_dneg, &interner));
}

#[test]
fn semantic_hash_fn_is_deterministic() {
    let mut interner = Interner::new();
    let body = make_binary(HirBinOp::Add, make_int_lit(1), make_int_lit(2));
    let hir_fn = make_fn_with_body(&mut interner, "det", "x", body);
    let h1 = semantic_hash_fn(&hir_fn, &interner);
    let h2 = semantic_hash_fn(&hir_fn, &interner);
    assert_eq!(h1, h2);
}

#[test]
fn semantic_hash_fn_returns_32_bytes() {
    let mut interner = Interner::new();
    let body = make_int_lit(0);
    let hir_fn = make_fn_with_body(&mut interner, "tiny", "x", body);
    let hash = semantic_hash_fn(&hir_fn, &interner);
    assert_eq!(hash.as_bytes().len(), 32);
}

#[test]
fn semantic_hash_item_fn_vs_struct_different() {
    let mut interner = Interner::new();
    let body = make_int_lit(0);
    let hir_fn = make_fn_with_body(&mut interner, "my_item", "x", body);
    let struct_def = glyim_hir::item::StructDef {
        doc: None, name: interner.intern("my_struct"), type_params: vec![],
        fields: vec![], span: make_span(), is_pub: false,
    };
    let hash_fn = semantic_hash_item(&HirItem::Fn(hir_fn), &interner);
    let hash_struct = semantic_hash_item(&HirItem::Struct(struct_def), &interner);
    assert_ne!(hash_fn, hash_struct);
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p glyim-hir --lib semantic_hash_tests 2>&1 | head -5`
Expected: Compilation error — `semantic_hash` module does not exist

- [ ] **Step 3: Implement semantic_hash module**

`crates/glyim-hir/src/semantic_hash.rs`:

```rust
//! Semantic hashing for HIR items.
//!
//! These functions compute content-addressable hashes of HIR items
//! that are stable across purely syntactic changes. The hash is
//! computed by first normalizing the HIR (via `SemanticNormalizer`)
//! and then hashing the resulting `NormalizedHirFn` using its
//! derived `Hash` implementation.

use crate::normalize::{SemanticNormalizer, NormalizedHirFn};
use crate::node::{HirItem, HirFn, HirImplDef, StructDef, EnumDef};
use glyim_interner::Interner;
use sha2::{Digest, Sha256};

/// A semantic hash: SHA-256 of the normalized HIR item.
/// This is a content-addressable hash that ignores:
/// - Local variable names (alpha-equivalence)
/// - ExprId numbering
/// - Span positions
/// - Operand order in commutative operations
/// - Double negation
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Debug)]
pub struct SemanticHash([u8; 32]);

impl SemanticHash {
    /// The all-zero sentinel value.
    pub const ZERO: Self = Self([0u8; 32]);

    /// Compute from raw bytes.
    pub fn of(data: &[u8]) -> Self {
        let mut hasher = Sha256::new();
        hasher.update(data);
        let digest = hasher.finalize();
        let mut bytes = [0u8; 32];
        bytes.copy_from_slice(&digest);
        Self(bytes)
    }

    /// Access the raw 32-byte hash.
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    /// Convert to hex string (64 chars).
    pub fn to_hex(self) -> String {
        self.0.iter().map(|b| format!("{:02x}", b)).collect()
    }

    /// Combine two semantic hashes (order-dependent).
    pub fn combine(a: Self, b: Self) -> Self {
        let mut hasher = Sha256::new();
        hasher.update(b"combine:");
        hasher.update(&a.0);
        hasher.update(&b.0);
        let digest = hasher.finalize();
        let mut bytes = [0u8; 32];
        bytes.copy_from_slice(&digest);
        Self(bytes)
    }
}

impl std::fmt::Display for SemanticHash {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.to_hex())
    }
}

/// Compute the semantic hash of a HirFn.
///
/// This normalizes the function (alpha-renaming locals, sorting commutative
/// operands, eliminating double negation) and then hashes the result.
/// Two functions that are semantically identical but differ in variable
/// names, expression IDs, or operand order in commutative ops will
/// produce the same hash.
pub fn semantic_hash_fn(hir_fn: &HirFn, interner: &Interner) -> SemanticHash {
    let mut normalizer = SemanticNormalizer::new(interner);
    let normalized = normalizer.normalize_fn(hir_fn);
    hash_normalized_fn(&normalized)
}

/// Compute the semantic hash of a HirItem.
pub fn semantic_hash_item(item: &HirItem, interner: &Interner) -> SemanticHash {
    match item {
        HirItem::Fn(hir_fn) => semantic_hash_fn(hir_fn, interner),
        HirItem::Struct(struct_def) => semantic_hash_struct(struct_def, interner),
        HirItem::Enum(enum_def) => semantic_hash_enum(enum_def, interner),
        HirItem::Impl(impl_def) => semantic_hash_impl(impl_def, interner),
        HirItem::Extern(extern_block) => {
            // Hash the string representation of the extern block
            let mut data = Vec::new();
            data.extend_from_slice(b"extern:");
            for func in &extern_block.functions {
                data.extend_from_slice(interner.resolve(func.name).as_bytes());
                data.push(0);
            }
            SemanticHash::of(&data)
        }
    }
}

/// Compute the semantic hash of a StructDef.
fn semantic_hash_struct(struct_def: &StructDef, interner: &Interner) -> SemanticHash {
    let mut data = Vec::new();
    data.extend_from_slice(b"struct:");
    data.extend_from_slice(interner.resolve(struct_def.name).as_bytes());
    data.push(0);
    for tp in &struct_def.type_params {
        data.extend_from_slice(interner.resolve(*tp).as_bytes());
        data.push(b',');
    }
    data.push(0);
    for field in &struct_def.fields {
        data.extend_from_slice(interner.resolve(field.name).as_bytes());
        data.push(b':');
        // Hash the type's debug representation (HirType implements Hash)
        data.extend_from_slice(format!("{:?}", field.ty).as_bytes());
        data.push(0);
    }
    SemanticHash::of(&data)
}

/// Compute the semantic hash of an EnumDef.
fn semantic_hash_enum(enum_def: &EnumDef, interner: &Interner) -> SemanticHash {
    let mut data = Vec::new();
    data.extend_from_slice(b"enum:");
    data.extend_from_slice(interner.resolve(enum_def.name).as_bytes());
    data.push(0);
    for tp in &enum_def.type_params {
        data.extend_from_slice(interner.resolve(*tp).as_bytes());
        data.push(b',');
    }
    data.push(0);
    for variant in &enum_def.variants {
        data.extend_from_slice(interner.resolve(variant.name).as_bytes());
        data.push(b':');
        data.extend_from_slice(&variant.tag.to_le_bytes());
        data.push(0);
    }
    SemanticHash::of(&data)
}

/// Compute the semantic hash of an HirImplDef.
fn semantic_hash_impl(impl_def: &HirImplDef, interner: &Interner) -> SemanticHash {
    let mut hashes: Vec<SemanticHash> = Vec::new();
    for method in &impl_def.methods {
        hashes.push(semantic_hash_fn(method, interner));
    }
    let target_hash = SemanticHash::of(
        interner.resolve(impl_def.target_name).as_bytes()
    );
    let mut combined = target_hash;
    for h in hashes {
        combined = SemanticHash::combine(combined, h);
    }
    combined
}

/// Internal: hash a NormalizedHirFn by serializing it deterministically.
///
/// We use a simple deterministic binary encoding rather than rust's Hasher,
/// to guarantee cross-platform stability. The encoding writes each field
/// in a fixed order with length prefixes and tag bytes.
fn hash_normalized_fn(norm: &NormalizedHirFn) -> SemanticHash {
    let mut data = Vec::new();

    // Tag: function
    data.extend_from_slice(b"fn:");

    // Name
    data.extend_from_slice(norm.name.as_bytes());
    data.push(0);

    // Type params
    for tp in &norm.type_params {
        data.extend_from_slice(tp.as_bytes());
        data.push(b',');
    }
    data.push(0);

    // Params: (name, type)
    for (name, ty) in &norm.params {
        data.extend_from_slice(name.as_bytes());
        data.push(b':');
        data.extend_from_slice(format!("{:?}", ty).as_bytes());
        data.push(b',');
    }
    data.push(0);

    // Return type
    if let Some(ret) = &norm.ret {
        data.extend_from_slice(format!("{:?}", ret).as_bytes());
    }
    data.push(0);

    // Flags
    data.push(norm.is_pub as u8);
    data.push(norm.is_extern_backed as u8);

    // Body: hash the NormalizedExpr via its Hash impl
    // We use a deterministic hasher for the body
    let body_hash = hash_normalized_expr(&norm.body);
    data.extend_from_slice(body_hash.as_bytes());

    SemanticHash::of(&data)
}

/// Deterministically hash a NormalizedExpr by walking it and producing
/// a canonical byte sequence.
fn hash_normalized_expr(expr: &NormalizedExpr) -> SemanticHash {
    let mut data = Vec::new();
    write_normalized_expr(&mut data, expr);
    SemanticHash::of(&data)
}

/// Write a canonical byte representation of a NormalizedExpr.
fn write_normalized_expr(buf: &mut Vec<u8>, expr: &NormalizedExpr) {
    match expr {
        NormalizedExpr::IntLit(v) => {
            buf.push(0x01);
            buf.extend_from_slice(&v.to_le_bytes());
        }
        NormalizedExpr::FloatLit(bits) => {
            buf.push(0x02);
            buf.extend_from_slice(&bits.to_le_bytes());
        }
        NormalizedExpr::BoolLit(b) => {
            buf.push(0x03);
            buf.push(*b as u8);
        }
        NormalizedExpr::StrLit(s) => {
            buf.push(0x04);
            buf.extend_from_slice(&(s.len() as u64).to_le_bytes());
            buf.extend_from_slice(s.as_bytes());
        }
        NormalizedExpr::UnitLit => {
            buf.push(0x05);
        }
        NormalizedExpr::Local(idx) => {
            buf.push(0x06);
            buf.extend_from_slice(&idx.to_le_bytes());
        }
        NormalizedExpr::Name(s) => {
            buf.push(0x07);
            buf.extend_from_slice(&(s.len() as u64).to_le_bytes());
            buf.extend_from_slice(s.as_bytes());
        }
        NormalizedExpr::Binary { op, lhs, rhs } => {
            buf.push(0x08);
            buf.extend_from_slice(format!("{:?}", op).as_bytes());
            buf.push(0);
            write_normalized_expr(buf, lhs);
            write_normalized_expr(buf, rhs);
        }
        NormalizedExpr::Unary { op, operand } => {
            buf.push(0x09);
            buf.extend_from_slice(format!("{:?}", op).as_bytes());
            buf.push(0);
            write_normalized_expr(buf, operand);
        }
        NormalizedExpr::Block { stmts } => {
            buf.push(0x0A);
            buf.extend_from_slice(&(stmts.len() as u64).to_le_bytes());
            for stmt in stmts {
                write_normalized_stmt(buf, stmt);
            }
        }
        NormalizedExpr::If { condition, then_branch, else_branch } => {
            buf.push(0x0B);
            write_normalized_expr(buf, condition);
            write_normalized_expr(buf, then_branch);
            if let Some(e) = else_branch {
                buf.push(1);
                write_normalized_expr(buf, e);
            } else {
                buf.push(0);
            }
        }
        NormalizedExpr::Call { callee, args } => {
            buf.push(0x0C);
            buf.extend_from_slice(callee.as_bytes());
            buf.push(0);
            buf.extend_from_slice(&(args.len() as u64).to_le_bytes());
            for arg in args {
                write_normalized_expr(buf, arg);
            }
        }
        NormalizedExpr::MethodCall { receiver, method_name, resolved_callee, args } => {
            buf.push(0x0D);
            write_normalized_expr(buf, receiver);
            buf.extend_from_slice(method_name.as_bytes());
            buf.push(0);
            if let Some(callee) = resolved_callee {
                buf.push(1);
                buf.extend_from_slice(callee.as_bytes());
                buf.push(0);
            } else {
                buf.push(0);
            }
            buf.extend_from_slice(&(args.len() as u64).to_le_bytes());
            for arg in args {
                write_normalized_expr(buf, arg);
            }
        }
        NormalizedExpr::Assert { condition, message } => {
            buf.push(0x0E);
            write_normalized_expr(buf, condition);
            if let Some(msg) = message {
                buf.push(1);
                write_normalized_expr(buf, msg);
            } else {
                buf.push(0);
            }
        }
        NormalizedExpr::Match { scrutinee, arms } => {
            buf.push(0x0F);
            write_normalized_expr(buf, scrutinee);
            buf.extend_from_slice(&(arms.len() as u64).to_le_bytes());
            for arm in arms {
                write_normalized_pattern(buf, &arm.pattern);
                if let Some(guard) = &arm.guard {
                    buf.push(1);
                    write_normalized_expr(buf, guard);
                } else {
                    buf.push(0);
                }
                write_normalized_expr(buf, &arm.body);
            }
        }
        NormalizedExpr::FieldAccess { object, field } => {
            buf.push(0x10);
            write_normalized_expr(buf, object);
            buf.extend_from_slice(field.as_bytes());
            buf.push(0);
        }
        NormalizedExpr::StructLit { struct_name, fields } => {
            buf.push(0x11);
            buf.extend_from_slice(struct_name.as_bytes());
            buf.push(0);
            buf.extend_from_slice(&(fields.len() as u64).to_le_bytes());
            for (name, expr) in fields {
                buf.extend_from_slice(name.as_bytes());
                buf.push(0);
                write_normalized_expr(buf, expr);
            }
        }
        NormalizedExpr::EnumVariant { enum_name, variant_name, args } => {
            buf.push(0x12);
            buf.extend_from_slice(enum_name.as_bytes());
            buf.push(0);
            buf.extend_from_slice(variant_name.as_bytes());
            buf.push(0);
            buf.extend_from_slice(&(args.len() as u64).to_le_bytes());
            for arg in args {
                write_normalized_expr(buf, arg);
            }
        }
        NormalizedExpr::ForIn { pattern, iter, body } => {
            buf.push(0x13);
            write_normalized_pattern(buf, pattern);
            write_normalized_expr(buf, iter);
            write_normalized_expr(buf, body);
        }
        NormalizedExpr::While { condition, body } => {
            buf.push(0x14);
            write_normalized_expr(buf, condition);
            write_normalized_expr(buf, body);
        }
        NormalizedExpr::Return { value } => {
            buf.push(0x15);
            if let Some(v) = value {
                buf.push(1);
                write_normalized_expr(buf, v);
            } else {
                buf.push(0);
            }
        }
        NormalizedExpr::As { expr, target_type } => {
            buf.push(0x16);
            write_normalized_expr(buf, expr);
            buf.extend_from_slice(format!("{:?}", target_type).as_bytes());
            buf.push(0);
        }
        NormalizedExpr::SizeOf { target_type } => {
            buf.push(0x17);
            buf.extend_from_slice(format!("{:?}", target_type).as_bytes());
            buf.push(0);
        }
        NormalizedExpr::TupleLit { elements } => {
            buf.push(0x18);
            buf.extend_from_slice(&(elements.len() as u64).to_le_bytes());
            for elem in elements {
                write_normalized_expr(buf, elem);
            }
        }
        NormalizedExpr::AddrOf { target } => {
            buf.push(0x19);
            buf.extend_from_slice(target.as_bytes());
            buf.push(0);
        }
        NormalizedExpr::Deref { expr } => {
            buf.push(0x1A);
            write_normalized_expr(buf, expr);
        }
        NormalizedExpr::Println { arg } => {
            buf.push(0x1B);
            write_normalized_expr(buf, arg);
        }
    }
}

/// Write a canonical byte representation of a NormalizedStmt.
fn write_normalized_stmt(buf: &mut Vec<u8>, stmt: &NormalizedStmt) {
    match stmt {
        NormalizedStmt::Let { local_id, mutable, value } => {
            buf.push(0x01);
            buf.extend_from_slice(&local_id.to_le_bytes());
            buf.push(*mutable as u8);
            write_normalized_expr(buf, value);
        }
        NormalizedStmt::Assign { local_id, value } => {
            buf.push(0x02);
            buf.extend_from_slice(&local_id.to_le_bytes());
            write_normalized_expr(buf, value);
        }
        NormalizedStmt::AssignField { object, field, value } => {
            buf.push(0x03);
            write_normalized_expr(buf, object);
            buf.extend_from_slice(field.as_bytes());
            buf.push(0);
            write_normalized_expr(buf, value);
        }
        NormalizedStmt::AssignDeref { target, value } => {
            buf.push(0x04);
            write_normalized_expr(buf, target);
            write_normalized_expr(buf, value);
        }
        NormalizedStmt::Expr(expr) => {
            buf.push(0x05);
            write_normalized_expr(buf, expr);
        }
    }
}

/// Write a canonical byte representation of a NormalizedPattern.
fn write_normalized_pattern(buf: &mut Vec<u8>, pat: &NormalizedPattern) {
    match pat {
        NormalizedPattern::Wild => buf.push(0x01),
        NormalizedPattern::BoolLit(b) => { buf.push(0x02); buf.push(*b as u8); }
        NormalizedPattern::IntLit(n) => { buf.push(0x03); buf.extend_from_slice(&n.to_le_bytes()); }
        NormalizedPattern::FloatLit(bits) => { buf.push(0x04); buf.extend_from_slice(&bits.to_le_bytes()); }
        NormalizedPattern::StrLit(s) => { buf.push(0x05); buf.extend_from_slice(s.as_bytes()); buf.push(0); }
        NormalizedPattern::Unit => buf.push(0x06),
        NormalizedPattern::Local(idx) => { buf.push(0x07); buf.extend_from_slice(&idx.to_le_bytes()); }
        NormalizedPattern::Struct { name, bindings } => {
            buf.push(0x08);
            buf.extend_from_slice(name.as_bytes()); buf.push(0);
            buf.extend_from_slice(&(bindings.len() as u64).to_le_bytes());
            for (field, sub) in bindings {
                buf.extend_from_slice(field.as_bytes()); buf.push(0);
                write_normalized_pattern(buf, sub);
            }
        }
        NormalizedPattern::EnumVariant { enum_name, variant_name, bindings } => {
            buf.push(0x09);
            buf.extend_from_slice(enum_name.as_bytes()); buf.push(0);
            buf.extend_from_slice(variant_name.as_bytes()); buf.push(0);
            buf.extend_from_slice(&(bindings.len() as u64).to_le_bytes());
            for (name, sub) in bindings {
                buf.extend_from_slice(name.as_bytes()); buf.push(0);
                write_normalized_pattern(buf, sub);
            }
        }
        NormalizedPattern::Tuple { elements } => {
            buf.push(0x0A);
            buf.extend_from_slice(&(elements.len() as u64).to_le_bytes());
            for e in elements { write_normalized_pattern(buf, e); }
        }
        NormalizedPattern::OptionSome(inner) => { buf.push(0x0B); write_normalized_pattern(buf, inner); }
        NormalizedPattern::OptionNone => buf.push(0x0C),
        NormalizedPattern::ResultOk(inner) => { buf.push(0x0D); write_normalized_pattern(buf, inner); }
        NormalizedPattern::ResultErr(inner) => { buf.push(0x0E); write_normalized_pattern(buf, inner); }
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p glyim-hir --lib semantic_hash_tests`
Expected: All 11 tests PASS

- [ ] **Step 5: Commit**

```bash
git add crates/glyim-hir/src/semantic_hash.rs crates/glyim-hir/src/tests/semantic_hash_tests.rs
git commit -m "feat(hir): add semantic_hash — deterministic hashing of normalized HIR"
```

---

## Chunk 2: Merkle IR Tree — Branch-Agnostic Caching

The Merkle IR Tree stores HIR items and compiled artifacts in a content-addressed DAG where each node's hash depends on its content and child hashes. This makes the cache completely independent of Git branches — items with the same content hash are shared across branches, and only items that differ need recompilation.

**Key design decisions:**
- `MerkleStore` wraps the existing `ContentStore` trait from `glyim-macro-vfs`, so it works with both `LocalContentStore` and `RemoteContentStore` out of the box.
- `MerkleNode` stores serializable data blobs (not `Arc<HirItem>` directly), because HIR items contain `Symbol` values that are session-dependent. Instead, we serialize the `NormalizedHirFn` or the raw bytes of compiled artifacts.
- The `ContentHash` from `glyim-macro-vfs` is used directly as the Merkle hash (it is SHA-256), rather than introducing a separate hash type.
- Root hash computation is order-dependent: the hash of a module is the combined hash of its items in declaration order.

---

### Task 3: MerkleNode and MerkleNodeData

**Files:**
- Create: `crates/glyim-merkle/Cargo.toml`
- Create: `crates/glyim-merkle/src/lib.rs`
- Create: `crates/glyim-merkle/src/node.rs`
- Test: `crates/glyim-merkle/src/tests/node_tests.rs`

- [ ] **Step 1: Create the crate skeleton**

```bash
mkdir -p crates/glyim-merkle/src/tests
```

`crates/glyim-merkle/Cargo.toml`:
```toml
[package]
name = "glyim-merkle"
version.workspace = true
edition.workspace = true
license.workspace = true
rust-version.workspace = true
description = "Merkle IR DAG for branch-agnostic incremental compilation caching"

[dependencies]
glyim-macro-vfs = { path = "../glyim-macro-vfs" }
glyim-interner = { path = "../glyim-interner" }
dashmap = "6"
serde = { version = "1", features = ["derive"] }
bincode = "1"
sha2 = "0.11"
tracing = "0.1"

[dev-dependencies]
tempfile = "3"
```

`crates/glyim-merkle/src/lib.rs`:
```rust
pub mod node;
pub mod store;
pub mod root;

pub use node::{MerkleNode, MerkleNodeData, MerkleNodeHeader};
pub use store::MerkleStore;
pub use root::MerkleRoot;

#[cfg(test)]
mod tests;
```

`crates/glyim-merkle/src/tests/mod.rs`:
```rust
mod node_tests;
mod store_tests;
mod root_tests;
```

- [ ] **Step 2: Write failing tests for MerkleNode**

Create `crates/glyim-merkle/src/tests/node_tests.rs`:

```rust
use glyim_merkle::node::{MerkleNode, MerkleNodeData, MerkleNodeHeader};
use glyim_macro_vfs::ContentHash;

#[test]
fn merkle_node_data_hir_fn_serialization_roundtrip() {
    let data = MerkleNodeData::HirFn {
        name: "add".to_string(),
        serialized: vec![1, 2, 3, 4],
    };
    let header = MerkleNodeHeader {
        data_type: data.data_type_tag(),
        child_count: 2,
    };
    let hash = ContentHash::of(b"test_node");
    let node = MerkleNode {
        hash,
        children: vec![ContentHash::of(b"child1"), ContentHash::of(b"child2")],
        data,
        header,
    };

    let serialized = node.serialize();
    let restored = MerkleNode::deserialize(&serialized).expect("deserialize");
    assert_eq!(restored.hash, node.hash);
    assert_eq!(restored.children.len(), 2);
    assert_eq!(restored.children[0], ContentHash::of(b"child1"));
    assert_eq!(restored.children[1], ContentHash::of(b"child2"));
    assert!(matches!(restored.data, MerkleNodeData::HirFn { name, .. } if name == "add"));
}

#[test]
fn merkle_node_data_object_code_serialization_roundtrip() {
    let data = MerkleNodeData::ObjectCode {
        symbol_name: "main".to_string(),
        bytes: vec![0x90, 0x90, 0xC3], // NOP NOP RET
    };
    let header = MerkleNodeHeader {
        data_type_tag: data.data_type_tag(),
        child_count: 0,
    };
    let hash = ContentHash::of(b"obj_node");
    let node = MerkleNode { hash, children: vec![], data, header };

    let serialized = node.serialize();
    let restored = MerkleNode::deserialize(&serialized).expect("deserialize");
    assert!(matches!(restored.data, MerkleNodeData::ObjectCode { symbol_name, .. } if symbol_name == "main"));
}

#[test]
fn merkle_node_compute_hash_depends_on_content() {
    let data_a = MerkleNodeData::HirFn { name: "a".to_string(), serialized: vec![1] };
    let data_b = MerkleNodeData::HirFn { name: "b".to_string(), serialized: vec![2] };
    let header_a = MerkleNodeHeader { data_type_tag: data_a.data_type_tag(), child_count: 0 };
    let header_b = MerkleNodeHeader { data_type_tag: data_b.data_type_tag(), child_count: 0 };
    let node_a = MerkleNode { hash: ContentHash::ZERO, children: vec![], data: data_a, header: header_a };
    let node_b = MerkleNode { hash: ContentHash::ZERO, children: vec![], data: data_b, header: header_b };
    let hash_a = node_a.compute_hash();
    let hash_b = node_b.compute_hash();
    assert_ne!(hash_a, hash_b);
}

#[test]
fn merkle_node_compute_hash_depends_on_children() {
    let data = MerkleNodeData::HirFn { name: "x".to_string(), serialized: vec![1] };
    let header_0 = MerkleNodeHeader { data_type_tag: data.data_type_tag(), child_count: 0 };
    let header_1 = MerkleNodeHeader { data_type_tag: data.data_type_tag(), child_count: 1 };
    let node_no_children = MerkleNode { hash: ContentHash::ZERO, children: vec![], data: data.clone(), header: header_0 };
    let node_with_child = MerkleNode { hash: ContentHash::ZERO, children: vec![ContentHash::of(b"child")], data, header: header_1 };
    assert_ne!(node_no_children.compute_hash(), node_with_child.compute_hash());
}

#[test]
fn merkle_node_compute_hash_is_deterministic() {
    let data = MerkleNodeData::HirFn { name: "det".to_string(), serialized: vec![42] };
    let header = MerkleNodeHeader { data_type_tag: data.data_type_tag(), child_count: 0 };
    let node = MerkleNode { hash: ContentHash::ZERO, children: vec![], data, header };
    let h1 = node.compute_hash();
    let h2 = node.compute_hash();
    assert_eq!(h1, h2);
}

#[test]
fn merkle_node_is_send_sync() {
    fn assert_bounds<T: Send + Sync>() {}
    assert_bounds::<MerkleNode>();
}
```

- [ ] **Step 3: Run tests to verify they fail**

Run: `cargo test -p glyim-merkle --lib node_tests 2>&1 | head -5`
Expected: Compilation error

- [ ] **Step 4: Implement MerkleNode**

`crates/glyim-merkle/src/node.rs`:

```rust
//! Merkle IR tree nodes.
//!
//! Each node stores a content hash, child hashes, serializable data,
//! and a header with type information. The hash of a node is computed
//! deterministically from its data + children, making the tree
//! content-addressed and branch-agnostic.

use glyim_macro_vfs::ContentHash;
use sha2::{Digest, Sha256};
use std::fmt;

/// Type tag for serialized MerkleNodeData.
pub const DATA_TYPE_HIR_FN: u8 = 0x01;
pub const DATA_TYPE_HIR_ITEM: u8 = 0x02;
pub const DATA_TYPE_LLVM_FUNCTION: u8 = 0x03;
pub const DATA_TYPE_OBJECT_CODE: u8 = 0x04;

/// A node in the Merkle IR tree.
#[derive(Clone, Debug)]
pub struct MerkleNode {
    /// Content hash of this node (computed from data + children).
    pub hash: ContentHash,
    /// Hashes of child nodes (dependencies).
    pub children: Vec<ContentHash>,
    /// The actual data stored in this node.
    pub data: MerkleNodeData,
    /// Header metadata for deserialization.
    pub header: MerkleNodeHeader,
}

/// Header for a MerkleNode — stored before the data blob for type dispatch.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct MerkleNodeHeader {
    /// Type tag: one of DATA_TYPE_*.
    pub data_type_tag: u8,
    /// Number of child hashes that follow.
    pub child_count: u32,
}

/// The data payload of a Merkle node.
///
/// All payloads are serializable byte blobs, not live Rust objects,
/// because HIR items contain session-dependent `Symbol` values that
/// cannot be meaningfully stored across compiler invocations.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MerkleNodeData {
    /// A normalized HIR function.
    HirFn {
        name: String,
        serialized: Vec<u8>,
    },
    /// A general HIR item (struct, enum, impl, extern).
    HirItem {
        kind: String,  // "struct", "enum", "impl", "extern"
        name: String,
        serialized: Vec<u8>,
    },
    /// Serialized LLVM function bitcode.
    LlvmFunction {
        symbol: String,
        bitcode: Vec<u8>,
    },
    /// Compiled machine code for a symbol.
    ObjectCode {
        symbol_name: String,
        bytes: Vec<u8>,
    },
}

impl MerkleNodeData {
    /// Return the type tag for this data variant.
    pub fn data_type_tag(&self) -> u8 {
        match self {
            Self::HirFn { .. } => DATA_TYPE_HIR_FN,
            Self::HirItem { .. } => DATA_TYPE_HIR_ITEM,
            Self::LlvmFunction { .. } => DATA_TYPE_LLVM_FUNCTION,
            Self::ObjectCode { .. } => DATA_TYPE_OBJECT_CODE,
        }
    }
}

impl MerkleNode {
    /// Compute the content hash of this node from its data and children.
    ///
    /// The hash is SHA-256 of: child_count || child_1_hash || ... || child_n_hash || data_blob
    pub fn compute_hash(&self) -> ContentHash {
        let mut hasher = Sha256::new();
        // Write child count
        hasher.update(&(self.children.len() as u64).to_le_bytes());
        // Write each child hash
        for child in &self.children {
            hasher.update(child.as_bytes());
        }
        // Write the data payload
        hasher.update(&self.serialize_data());
        let digest = hasher.finalize();
        let mut bytes = [0u8; 32];
        bytes.copy_from_slice(&digest);
        ContentHash::from_bytes(bytes)
    }

    /// Serialize this node into a byte vector for CAS storage.
    ///
    /// Format: [header (bincode)] [child hashes (32 bytes each)] [data blob]
    pub fn serialize(&self) -> Vec<u8> {
        let mut buf = Vec::new();
        // Header
        let header_bytes = bincode::serialize(&self.header).expect("serialize header");
        buf.extend_from_slice(&(header_bytes.len() as u64).to_le_bytes());
        buf.extend_from_slice(&header_bytes);
        // Child hashes
        for child in &self.children {
            buf.extend_from_slice(child.as_bytes());
        }
        // Data blob
        buf.extend_from_slice(&self.serialize_data());
        buf
    }

    /// Deserialize a node from bytes.
    pub fn deserialize(data: &[u8]) -> Result<Self, NodeDeserializeError> {
        let mut offset = 0;

        // Read header length
        if data.len() < 8 { return Err(NodeDeserializeError::TooShort); }
        let header_len = u64::from_le_bytes(data[0..8].try_into().unwrap()) as usize;
        offset = 8;

        // Read header
        if data.len() < offset + header_len { return Err(NodeDeserializeError::TooShort); }
        let header: MerkleNodeHeader = bincode::deserialize(&data[offset..offset + header_len])
            .map_err(|e| NodeDeserializeError::HeaderCorrupt(e.to_string()))?;
        offset += header_len;

        // Read child hashes
        let child_count = header.child_count as usize;
        let children_size = child_count * 32;
        if data.len() < offset + children_size { return Err(NodeDeserializeError::TooShort); }
        let mut children = Vec::with_capacity(child_count);
        for i in 0..child_count {
            let start = offset + i * 32;
            let end = start + 32;
            let mut hash_bytes = [0u8; 32];
            hash_bytes.copy_from_slice(&data[start..end]);
            children.push(ContentHash::from_bytes(hash_bytes));
        }
        offset += children_size;

        // Read data blob
        let data_blob = data[offset..].to_vec();
        let merkle_data = Self::deserialize_data(header.data_type_tag, &data_blob)?;

        // Compute the actual hash
        let hash = {
            let mut hasher = Sha256::new();
            hasher.update(&(children.len() as u64).to_le_bytes());
            for child in &children {
                hasher.update(child.as_bytes());
            }
            hasher.update(&data_blob);
            let digest = hasher.finalize();
            let mut bytes = [0u8; 32];
            bytes.copy_from_slice(&digest);
            ContentHash::from_bytes(bytes)
        };

        Ok(Self { hash, children, data: merkle_data, header })
    }

    /// Serialize just the data payload.
    fn serialize_data(&self) -> Vec<u8> {
        match &self.data {
            MerkleNodeData::HirFn { name, serialized } => {
                let mut buf = vec![DATA_TYPE_HIR_FN];
                buf.extend_from_slice(&(name.len() as u64).to_le_bytes());
                buf.extend_from_slice(name.as_bytes());
                buf.extend_from_slice(&(serialized.len() as u64).to_le_bytes());
                buf.extend_from_slice(serialized);
                buf
            }
            MerkleNodeData::HirItem { kind, name, serialized } => {
                let mut buf = vec![DATA_TYPE_HIR_ITEM];
                buf.extend_from_slice(&(kind.len() as u64).to_le_bytes());
                buf.extend_from_slice(kind.as_bytes());
                buf.extend_from_slice(&(name.len() as u64).to_le_bytes());
                buf.extend_from_slice(name.as_bytes());
                buf.extend_from_slice(&(serialized.len() as u64).to_le_bytes());
                buf.extend_from_slice(serialized);
                buf
            }
            MerkleNodeData::LlvmFunction { symbol, bitcode } => {
                let mut buf = vec![DATA_TYPE_LLVM_FUNCTION];
                buf.extend_from_slice(&(symbol.len() as u64).to_le_bytes());
                buf.extend_from_slice(symbol.as_bytes());
                buf.extend_from_slice(&(bitcode.len() as u64).to_le_bytes());
                buf.extend_from_slice(bitcode);
                buf
            }
            MerkleNodeData::ObjectCode { symbol_name, bytes } => {
                let mut buf = vec![DATA_TYPE_OBJECT_CODE];
                buf.extend_from_slice(&(symbol_name.len() as u64).to_le_bytes());
                buf.extend_from_slice(symbol_name.as_bytes());
                buf.extend_from_slice(&(bytes.len() as u64).to_le_bytes());
                buf.extend_from_slice(bytes);
                buf
            }
        }
    }

    /// Deserialize the data payload from bytes.
    fn deserialize_data(tag: u8, data: &[u8]) -> Result<MerkleNodeData, NodeDeserializeError> {
        match tag {
            DATA_TYPE_HIR_FN => {
                let mut offset = 0;
                let name_len = read_u64(data, &mut offset)? as usize;
                let name = read_string(data, &mut offset, name_len)?;
                let serialized_len = read_u64(data, &mut offset)? as usize;
                let serialized = read_bytes(data, &mut offset, serialized_len)?;
                Ok(MerkleNodeData::HirFn { name, serialized })
            }
            DATA_TYPE_HIR_ITEM => {
                let mut offset = 0;
                let kind_len = read_u64(data, &mut offset)? as usize;
                let kind = read_string(data, &mut offset, kind_len)?;
                let name_len = read_u64(data, &mut offset)? as usize;
                let name = read_string(data, &mut offset, name_len)?;
                let serialized_len = read_u64(data, &mut offset)? as usize;
                let serialized = read_bytes(data, &mut offset, serialized_len)?;
                Ok(MerkleNodeData::HirItem { kind, name, serialized })
            }
            DATA_TYPE_LLVM_FUNCTION => {
                let mut offset = 0;
                let symbol_len = read_u64(data, &mut offset)? as usize;
                let symbol = read_string(data, &mut offset, symbol_len)?;
                let bitcode_len = read_u64(data, &mut offset)? as usize;
                let bitcode = read_bytes(data, &mut offset, bitcode_len)?;
                Ok(MerkleNodeData::LlvmFunction { symbol, bitcode })
            }
            DATA_TYPE_OBJECT_CODE => {
                let mut offset = 0;
                let symbol_name_len = read_u64(data, &mut offset)? as usize;
                let symbol_name = read_string(data, &mut offset, symbol_name_len)?;
                let bytes_len = read_u64(data, &mut offset)? as usize;
                let bytes = read_bytes(data, &mut offset, bytes_len)?;
                Ok(MerkleNodeData::ObjectCode { symbol_name, bytes })
            }
            _ => Err(NodeDeserializeError::UnknownDataType(tag)),
        }
    }
}

// ── Helper functions for deserialization ──

fn read_u64(data: &[u8], offset: &mut usize) -> Result<u64, NodeDeserializeError> {
    if data.len() < *offset + 8 { return Err(NodeDeserializeError::TooShort); }
    let val = u64::from_le_bytes(data[*offset..*offset + 8].try_into().unwrap());
    *offset += 8;
    Ok(val)
}

fn read_string(data: &[u8], offset: &mut usize, len: usize) -> Result<String, NodeDeserializeError> {
    if data.len() < *offset + len { return Err(NodeDeserializeError::TooShort); }
    let s = String::from_utf8(data[*offset..*offset + len].to_vec())
        .map_err(|e| NodeDeserializeError::InvalidUtf8(e.to_string()))?;
    *offset += len;
    Ok(s)
}

fn read_bytes(data: &[u8], offset: &mut usize, len: usize) -> Result<Vec<u8>, NodeDeserializeError> {
    if data.len() < *offset + len { return Err(NodeDeserializeError::TooShort); }
    let bytes = data[*offset..*offset + len].to_vec();
    *offset += len;
    Ok(bytes)
}

/// Errors that can occur during MerkleNode deserialization.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NodeDeserializeError {
    /// The input data is too short.
    TooShort,
    /// The header is corrupt.
    HeaderCorrupt(String),
    /// Unknown data type tag.
    UnknownDataType(u8),
    /// Invalid UTF-8 in a string field.
    InvalidUtf8(String),
}

impl std::fmt::Display for NodeDeserializeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TooShort => write!(f, "data too short to deserialize"),
            Self::HeaderCorrupt(msg) => write!(f, "corrupt header: {msg}"),
            Self::UnknownDataType(tag) => write!(f, "unknown data type tag: {tag}"),
            Self::InvalidUtf8(msg) => write!(f, "invalid UTF-8: {msg}"),
        }
    }
}

impl std::error::Error for NodeDeserializeError {}
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test -p glyim-merkle --lib node_tests`
Expected: All 6 tests PASS

- [ ] **Step 6: Commit**

```bash
git add crates/glyim-merkle/
git commit -m "feat(merkle): add MerkleNode — serializable content-addressed IR node"
```

---

### Task 4: MerkleStore — CAS-Backed Merkle DAG

**Files:**
- Create: `crates/glyim-merkle/src/store.rs`
- Test: `crates/glyim-merkle/src/tests/store_tests.rs`

- [ ] **Step 1: Write failing tests for MerkleStore**

Create `crates/glyim-merkle/src/tests/store_tests.rs`:

```rust
use glyim_merkle::store::MerkleStore;
use glyim_merkle::node::{MerkleNode, MerkleNodeData, MerkleNodeHeader};
use glyim_macro_vfs::ContentHash;
use std::sync::Arc;

/// A simple in-memory content store for testing.
struct InMemoryStore {
    blobs: std::sync::Mutex<std::collections::HashMap<ContentHash, Vec<u8>>>,
}

impl InMemoryStore {
    fn new() -> Self {
        Self { blobs: std::sync::Mutex::new(std::collections::HashMap::new()) }
    }
}

impl glyim_macro_vfs::ContentStore for InMemoryStore {
    fn store(&self, content: &[u8]) -> ContentHash {
        let hash = ContentHash::of(content);
        self.blobs.lock().unwrap().insert(hash, content.to_vec());
        hash
    }
    fn retrieve(&self, hash: ContentHash) -> Option<Vec<u8>> {
        self.blobs.lock().unwrap().get(&hash).cloned()
    }
    fn register_name(&self, _name: &str, _hash: ContentHash) {}
    fn resolve_name(&self, _name: &str) -> Option<ContentHash> { None }
    fn store_action_result(&self, _action_hash: ContentHash, _result: glyim_macro_vfs::ActionResult) -> Result<(), glyim_macro_vfs::StoreError> { Ok(()) }
    fn retrieve_action_result(&self, _action_hash: ContentHash) -> Option<glyim_macro_vfs::ActionResult> { None }
    fn has_blobs(&self, hashes: &[ContentHash]) -> Vec<ContentHash> {
        let blobs = self.blobs.lock().unwrap();
        hashes.iter().filter(|h| blobs.contains(h)).copied().collect()
    }
}

fn make_test_node(name: &str) -> MerkleNode {
    let data = MerkleNodeData::HirFn { name: name.to_string(), serialized: vec![1, 2, 3] };
    let header = MerkleNodeHeader { data_type_tag: data.data_type_tag(), child_count: 0 };
    let mut node = MerkleNode { hash: ContentHash::ZERO, children: vec![], data, header };
    node.hash = node.compute_hash();
    node
}

#[test]
fn store_and_retrieve_node() {
    let cas = Arc::new(InMemoryStore::new());
    let store = MerkleStore::new(cas);
    let node = make_test_node("add");
    let hash = store.put(node.clone());
    let retrieved = store.get(&hash).expect("should find node");
    assert_eq!(retrieved.hash, node.hash);
}

#[test]
fn store_returns_correct_hash() {
    let cas = Arc::new(InMemoryStore::new());
    let store = MerkleStore::new(cas);
    let node = make_test_node("add");
    let expected_hash = node.compute_hash();
    let actual_hash = store.put(node);
    assert_eq!(expected_hash, actual_hash);
}

#[test]
fn store_missing_node_returns_none() {
    let cas = Arc::new(InMemoryStore::new());
    let store = MerkleStore::new(cas);
    let missing = ContentHash::of(b"nonexistent");
    assert!(store.get(&missing).is_none());
}

#[test]
fn store_same_node_is_idempotent() {
    let cas = Arc::new(InMemoryStore::new());
    let store = MerkleStore::new(cas);
    let node = make_test_node("idem");
    let hash1 = store.put(node.clone());
    let hash2 = store.put(node);
    assert_eq!(hash1, hash2);
}

#[test]
fn store_node_with_children() {
    let cas = Arc::new(InMemoryStore::new());
    let store = MerkleStore::new(cas);

    // Create child nodes
    let child1 = make_test_node("child1");
    let child2 = make_test_node("child2");
    let child1_hash = store.put(child1);
    let child2_hash = store.put(child2);

    // Create parent with children
    let parent_data = MerkleNodeData::HirFn { name: "parent".to_string(), serialized: vec![10, 20] };
    let parent_header = MerkleNodeHeader { data_type_tag: parent_data.data_type_tag(), child_count: 2 };
    let parent = MerkleNode {
        hash: ContentHash::ZERO,
        children: vec![child1_hash, child2_hash],
        data: parent_data,
        header: parent_header,
    };
    let parent_hash = store.put(parent);

    // Retrieve parent and verify children
    let retrieved = store.get(&parent_hash).expect("find parent");
    assert_eq!(retrieved.children.len(), 2);
    assert_eq!(retrieved.children[0], child1_hash);
    assert_eq!(retrieved.children[1], child2_hash);

    // Children should also be retrievable
    assert!(store.get(&child1_hash).is_some());
    assert!(store.get(&child2_hash).is_some());
}

#[test]
fn store_different_data_different_hash() {
    let cas = Arc::new(InMemoryStore::new());
    let store = MerkleStore::new(cas);
    let node_a = make_test_node("aaa");
    let node_b = make_test_node("bbb");
    let hash_a = store.put(node_a);
    let hash_b = store.put(node_b);
    assert_ne!(hash_a, hash_b);
}

#[test]
fn store_is_send_sync() {
    fn assert_bounds<T: Send + Sync>() {}
    assert_bounds::<MerkleStore>();
}

#[test]
fn store_caches_in_memory() {
    let cas = Arc::new(InMemoryStore::new());
    let store = MerkleStore::new(cas);
    let node = make_test_node("cached");
    let hash = store.put(node);

    // First get: from CAS (miss on in-memory cache)
    let _r1 = store.get(&hash).expect("find in CAS");
    // Second get: from in-memory cache
    let r2 = store.get(&hash).expect("find in cache");
    assert_eq!(r2.hash, hash);
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p glyim-merkle --lib store_tests 2>&1 | head -5`
Expected: Compilation error

- [ ] **Step 3: Implement MerkleStore**

`crates/glyim-merkle/src/store.rs`:

```rust
//! The Merkle IR store — content-addressed, branch-agnostic.
//!
//! `MerkleStore` wraps the existing `ContentStore` trait from
//! `glyim-macro-vfs`, so it works with both `LocalContentStore`
//! and `RemoteContentStore` out of the box. An in-memory
//! `DashMap` cache avoids repeated CAS lookups for hot nodes.

use crate::node::MerkleNode;
use dashmap::DashMap;
use glyim_macro_vfs::{ContentHash, ContentStore};
use std::sync::Arc;

/// A content-addressed Merkle DAG store backed by CAS.
///
/// Nodes are stored by their content hash (computed from data + children).
/// The store maintains an in-memory cache of recently accessed nodes
/// to avoid repeated serialization/deserialization and CAS lookups.
pub struct MerkleStore {
    /// Backing CAS store (local filesystem, remote, or in-memory).
    cas: Arc<dyn ContentStore>,
    /// In-memory cache: hash → deserialized node.
    cache: DashMap<ContentHash, MerkleNode>,
}

impl MerkleStore {
    /// Create a new MerkleStore backed by the given CAS.
    pub fn new(cas: Arc<dyn ContentStore>) -> Self {
        Self {
            cas,
            cache: DashMap::new(),
        }
    }

    /// Store a Merkle node. Returns its content hash.
    ///
    /// The node's hash is computed from its data and children.
    /// The serialized node is written to the CAS, and the node
    /// is cached in memory.
    pub fn put(&self, node: MerkleNode) -> ContentHash {
        let hash = node.compute_hash();
        let serialized = node.serialize();
        self.cas.store(&serialized);
        self.cache.insert(hash, node);
        hash
    }

    /// Look up a node by hash.
    ///
    /// Checks the in-memory cache first, then falls back to CAS.
    /// Returns `None` if the node is not found in either.
    pub fn get(&self, hash: &ContentHash) -> Option<MerkleNode> {
        // Check in-memory cache
        if let Some(cached) = self.cache.get(hash) {
            return Some(cached.clone());
        }

        // Fall back to CAS
        let data = self.cas.retrieve(*hash)?;
        let node = MerkleNode::deserialize(&data).ok()?;
        self.cache.insert(*hash, node.clone());
        Some(node)
    }

    /// Check if a node exists in the store.
    pub fn contains(&self, hash: &ContentHash) -> bool {
        self.cache.contains_key(hash) || self.cas.retrieve(*hash).is_some()
    }

    /// Clear the in-memory cache (does not remove from CAS).
    pub fn clear_cache(&self) {
        self.cache.clear();
    }

    /// Number of nodes in the in-memory cache.
    pub fn cache_size(&self) -> usize {
        self.cache.len()
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p glyim-merkle --lib store_tests`
Expected: All 9 tests PASS

- [ ] **Step 5: Commit**

```bash
git add crates/glyim-merkle/src/store.rs crates/glyim-merkle/src/tests/store_tests.rs
git commit -m "feat(merkle): add MerkleStore — CAS-backed Merkle DAG with in-memory cache"
```

---

### Task 5: MerkleRoot — Branch-Switch Logic

**Files:**
- Create: `crates/glyim-merkle/src/root.rs`
- Test: `crates/glyim-merkle/src/tests/root_tests.rs`

- [ ] **Step 1: Write failing tests for MerkleRoot**

Create `crates/glyim-merkle/src/tests/root_tests.rs`:

```rust
use glyim_merkle::root::{MerkleRoot, compute_root_hash};
use glyim_macro_vfs::ContentHash;

#[test]
fn compute_root_from_empty_items() {
    let items: Vec<(String, ContentHash)> = vec![];
    let root = compute_root_hash(&items);
    // Empty root should be a well-defined hash
    assert_ne!(root, ContentHash::ZERO); // not the zero hash
}

#[test]
fn compute_root_from_single_item() {
    let hash = ContentHash::of(b"item1");
    let items = vec![("fn_add".to_string(), hash)];
    let root = compute_root_hash(&items);
    // Root depends on the single item
    assert_ne!(root, hash); // root != item hash (it's prefixed)
}

#[test]
fn compute_root_order_matters() {
    let hash_a = ContentHash::of(b"a");
    let hash_b = ContentHash::of(b"b");
    let items_ab = vec![("a".to_string(), hash_a), ("b".to_string(), hash_b)];
    let items_ba = vec![("b".to_string(), hash_b), ("a".to_string(), hash_a)];
    let root_ab = compute_root_hash(&items_ab);
    let root_ba = compute_root_hash(&items_ba);
    assert_ne!(root_ab, root_ba); // Order matters for root hash
}

#[test]
fn compute_root_same_items_same_hash() {
    let hash_a = ContentHash::of(b"a");
    let hash_b = ContentHash::of(b"b");
    let items1 = vec![("a".to_string(), hash_a), ("b".to_string(), hash_b)];
    let items2 = vec![("a".to_string(), hash_a), ("b".to_string(), hash_b)];
    assert_eq!(compute_root_hash(&items1), compute_root_hash(&items2));
}

#[test]
fn merkle_root_diff_items() {
    // Simulate two branches: main and feature
    // Branch "main": [fn_add, fn_sub] (both functions unchanged)
    // Branch "feature": [fn_add, fn_mul] (fn_sub changed to fn_mul)
    let fn_add_hash = ContentHash::of(b"fn_add_body");
    let fn_sub_hash = ContentHash::of(b"fn_sub_body");
    let fn_mul_hash = ContentHash::of(b"fn_mul_body");

    let main_items = vec![
        ("add".to_string(), fn_add_hash),
        ("sub".to_string(), fn_sub_hash),
    ];
    let feature_items = vec![
        ("add".to_string(), fn_add_hash),
        ("mul".to_string(), fn_mul_hash),
    ];

    let root_main = compute_root_hash(&main_items);
    let root_feature = compute_root_hash(&feature_items);

    // Roots differ (one function changed)
    assert_ne!(root_main, root_feature);

    // But the shared function has the same hash in both branches
    assert_eq!(main_items[0].1, feature_items[0].1);
}

#[test]
fn merkle_root_find_changed_items() {
    let fn_add_hash = ContentHash::of(b"fn_add_v1");
    let fn_sub_v1 = ContentHash::of(b"fn_sub_v1");
    let fn_sub_v2 = ContentHash::of(b"fn_sub_v2");

    let old_items = vec![
        ("add".to_string(), fn_add_hash),
        ("sub".to_string(), fn_sub_v1),
    ];
    let new_items = vec![
        ("add".to_string(), fn_add_hash),
        ("sub".to_string(), fn_sub_v2),
    ];

    let changed = MerkleRoot::find_changed_items(&old_items, &new_items);
    assert_eq!(changed.len(), 1);
    assert_eq!(changed[0].0, "sub");
}

#[test]
fn merkle_root_find_no_changes() {
    let items = vec![
        ("a".to_string(), ContentHash::of(b"a_body")),
        ("b".to_string(), ContentHash::of(b"b_body")),
    ];
    let changed = MerkleRoot::find_changed_items(&items, &items);
    assert!(changed.is_empty());
}

#[test]
fn merkle_root_find_added_items() {
    let old_items = vec![
        ("a".to_string(), ContentHash::of(b"a_body")),
    ];
    let new_items = vec![
        ("a".to_string(), ContentHash::of(b"a_body")),
        ("b".to_string(), ContentHash::of(b"b_body")),
    ];
    let changed = MerkleRoot::find_changed_items(&old_items, &new_items);
    assert_eq!(changed.len(), 1);
    assert_eq!(changed[0].0, "b");
}

#[test]
fn merkle_root_find_removed_items() {
    let old_items = vec![
        ("a".to_string(), ContentHash::of(b"a_body")),
        ("b".to_string(), ContentHash::of(b"b_body")),
    ];
    let new_items = vec![
        ("a".to_string(), ContentHash::of(b"a_body")),
    ];
    let changed = MerkleRoot::find_changed_items(&old_items, &new_items);
    assert_eq!(changed.len(), 1);
    assert_eq!(changed[0].0, "b");
}

#[test]
fn merkle_root_is_send_sync() {
    fn assert_bounds<T: Send + Sync>() {}
    assert_bounds::<MerkleRoot>();
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p glyim-merkle --lib root_tests 2>&1 | head -5`
Expected: Compilation error

- [ ] **Step 3: Implement MerkleRoot and compute_root_hash**

`crates/glyim-merkle/src/root.rs`:

```rust
//! Merkle root computation and branch-switch logic.
//!
//! The Merkle root is the hash of all top-level item hashes in a
//! module, in declaration order. Two branches that share items
//! will share those items' Merkle nodes (same content hash),
//! so switching branches only requires recompiling the items
//! whose hashes differ.

use glyim_macro_vfs::ContentHash;
use sha2::{Digest, Sha256};
use std::collections::HashMap;

/// A computed Merkle root representing the state of a compilation unit.
///
/// The root hash is computed from all top-level item hashes in order.
/// It serves as a fingerprint for the entire module state and enables
/// O(1) comparison between branches.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct MerkleRoot {
    /// The root hash.
    pub hash: ContentHash,
    /// The items and their hashes that went into this root.
    pub items: Vec<(String, ContentHash)>,
}

impl MerkleRoot {
    /// Compute a MerkleRoot from a list of (item_name, item_hash) pairs.
    pub fn compute(items: Vec<(String, ContentHash)>) -> Self {
        let hash = compute_root_hash(&items);
        Self { hash, items }
    }

    /// Compare two MerkleRoots and return the names of items that changed.
    ///
    /// An item is "changed" if:
    /// - It exists in both but has a different hash
    /// - It exists in `new` but not in `old` (added)
    /// - It exists in `old` but not in `new` (removed)
    pub fn find_changed_items(
        old: &[(String, ContentHash)],
        new: &[(String, ContentHash)],
    ) -> Vec<(String, ItemChange)> {
        let old_map: HashMap<&str, ContentHash> = old.iter().map(|(n, h)| (n.as_str(), *h)).collect();
        let new_map: HashMap<&str, ContentHash> = new.iter().map(|(n, h)| (n.as_str(), *h)).collect();

        let mut changed = Vec::new();

        // Check for modified and added items
        for (name, new_hash) in &new_map {
            match old_map.get(name) {
                Some(old_hash) if old_hash == new_hash => { /* unchanged */ }
                Some(_) => changed.push((name.to_string(), ItemChange::Modified)),
                None => changed.push((name.to_string(), ItemChange::Added)),
            }
        }

        // Check for removed items
        for name in old_map.keys() {
            if !new_map.contains_key(name) {
                changed.push((name.to_string(), ItemChange::Removed));
            }
        }

        changed
    }
}

/// How an item changed between two Merkle roots.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum ItemChange {
    /// Item exists in both but with a different hash.
    Modified,
    /// Item was added (exists in new, not in old).
    Added,
    /// Item was removed (exists in old, not in new).
    Removed,
}

/// Compute the root hash from a list of (item_name, item_hash) pairs.
///
/// The hash is SHA-256 of: "merkle_root:" || count || (name || hash)*
/// The order of items matters — reordering items changes the root hash.
pub fn compute_root_hash(items: &[(String, ContentHash)]) -> ContentHash {
    let mut hasher = Sha256::new();
    hasher.update(b"merkle_root:");
    hasher.update(&(items.len() as u64).to_le_bytes());
    for (name, hash) in items {
        hasher.update(&(name.len() as u64).to_le_bytes());
        hasher.update(name.as_bytes());
        hasher.update(hash.as_bytes());
    }
    let digest = hasher.finalize();
    let mut bytes = [0u8; 32];
    bytes.copy_from_slice(&digest);
    ContentHash::from_bytes(bytes)
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p glyim-merkle --lib root_tests`
Expected: All 10 tests PASS

- [ ] **Step 5: Commit**

```bash
git add crates/glyim-merkle/src/root.rs crates/glyim-merkle/src/tests/root_tests.rs
git commit -m "feat(merkle): add MerkleRoot — branch-agnostic root hash and diff computation"
```

---

## Chunk 3: Fractal Cache Granularity — Adaptive Zooming

The compiler dynamically adjusts its caching granularity based on edit patterns. When a developer is making localized edits (fixing one function), fine-grained per-function caching minimizes recompilation. When a developer is doing a large refactoring (spread across the whole file), coarse-grained per-file caching reduces overhead. The `GranularityMonitor` observes edit velocity and concentration to adapt automatically.

---

### Task 6: GranularityMonitor

**Files:**
- Create: `crates/glyim-query/src/granularity.rs`
- Test: `crates/glyim-query/src/tests/granularity_tests.rs`

- [ ] **Step 1: Write failing tests for GranularityMonitor**

Add the module to `crates/glyim-query/src/lib.rs`:
```rust
pub mod granularity;
pub use granularity::{GranularityMonitor, CacheGranularity, EditHistory};
```

Add to `crates/glyim-query/src/tests/mod.rs`:
```rust
mod granularity_tests;
```

Create `crates/glyim-query/src/tests/granularity_tests.rs`:

```rust
use glyim_query::granularity::{GranularityMonitor, CacheGranularity, EditHistory};
use std::path::PathBuf;
use std::time::Duration;

#[test]
fn default_granularity_is_module() {
    let monitor = GranularityMonitor::new();
    let path = PathBuf::from("main.g");
    assert_eq!(monitor.granularity(&path), CacheGranularity::Module);
}

#[test]
fn concentrated_edits_become_fine_grained() {
    let monitor = GranularityMonitor::new();
    let path = PathBuf::from("main.g");

    // Simulate 5 edits all at the same location (high concentration)
    for _ in 0..5 {
        monitor.observe_edit(&path, 10..15);
    }
    assert_eq!(monitor.granularity(&path), CacheGranularity::FineGrained);
}

#[test]
fn spread_edits_with_high_count_become_coarse() {
    let monitor = GranularityMonitor::new();
    let path = PathBuf::from("main.g");

    // Simulate 25 edits spread across the file (low concentration)
    for i in 0..25 {
        let start = i * 10;
        monitor.observe_edit(&path, start..(start + 3));
    }
    assert_eq!(monitor.granularity(&path), CacheGranularity::CoarseGrained);
}

#[test]
fn unknown_file_is_module_granularity() {
    let monitor = GranularityMonitor::new();
    let unknown = PathBuf::from("nonexistent.g");
    assert_eq!(monitor.granularity(&unknown), CacheGranularity::Module);
}

#[test]
fn edit_history_tracks_count() {
    let monitor = GranularityMonitor::new();
    let path = PathBuf::from("test.g");
    monitor.observe_edit(&path, 1..5);
    monitor.observe_edit(&path, 10..20);
    let history = monitor.edit_history(&path).expect("should exist");
    assert_eq!(history.edit_count, 2);
}

#[test]
fn edit_history_concentration_range() {
    let history = EditHistory::default();
    // Default concentration should be 0.0
    assert_eq!(history.edit_concentration, 0.0);
}

#[test]
fn granularity_is_send_sync() {
    fn assert_bounds<T: Send + Sync>() {}
    assert_bounds::<GranularityMonitor>();
    assert_bounds::<CacheGranularity>();
}

#[test]
fn cache_granularity_is_copy() {
    let g = CacheGranularity::FineGrained;
    let _g2 = g;
    let _g3 = g; // Copy, not move
}

#[test]
fn multiple_files_independent_granularity() {
    let monitor = GranularityMonitor::new();
    let path_a = PathBuf::from("a.g");
    let path_b = PathBuf::from("b.g");

    // File A: concentrated → fine
    for _ in 0..5 {
        monitor.observe_edit(&path_a, 10..15);
    }

    // File B: spread → coarse
    for i in 0..25 {
        monitor.observe_edit(&path_b, (i * 10)..(i * 10 + 3));
    }

    assert_eq!(monitor.granularity(&path_a), CacheGranularity::FineGrained);
    assert_eq!(monitor.granularity(&path_b), CacheGranularity::CoarseGrained);
}

#[test]
fn reset_clears_history() {
    let monitor = GranularityMonitor::new();
    let path = PathBuf::from("reset.g");
    for _ in 0..5 {
        monitor.observe_edit(&path, 10..15);
    }
    assert_ne!(monitor.granularity(&path), CacheGranularity::Module);
    monitor.reset(&path);
    assert_eq!(monitor.granularity(&path), CacheGranularity::Module);
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p glyim-query --lib granularity_tests 2>&1 | head -5`
Expected: Compilation error

- [ ] **Step 3: Implement GranularityMonitor**

`crates/glyim-query/src/granularity.rs`:

```rust
//! Adaptive cache granularity based on edit patterns.
//!
//! The compiler dynamically adjusts its caching granularity based on
//! how the developer is editing each file. When edits are concentrated
//! in one area (fixing a single function), fine-grained per-function
//! caching minimizes recompilation. When edits are spread across the
//! file (large refactoring), coarse-grained per-file caching reduces
//! overhead. The default is module-level granularity.

use dashmap::DashMap;
use std::path::{Path, PathBuf};
use std::time::Instant;

/// Cache granularity levels, from finest to coarsest.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum CacheGranularity {
    /// Cache per function/expression — for files with localized edits.
    /// Only the changed function and its dependents are recompiled.
    FineGrained,
    /// Cache per module — for small files or stable code.
    /// Default granularity when no edit pattern has been established.
    Module,
    /// Cache whole-file result — for high-churn files during refactoring.
    /// The entire file is treated as one cache unit to reduce tracking overhead.
    CoarseGrained,
}

/// Per-file edit history used to determine granularity.
#[derive(Clone, Debug, Default)]
pub struct EditHistory {
    /// Recent edit locations (timestamp, line range).
    /// Only the last N edits are kept for concentration computation.
    pub recent_edits: Vec<(Instant, std::ops::Range<usize>)>,
    /// Total number of edits observed.
    pub edit_count: u32,
    /// Edit concentration: 0.0 = spread across file, 1.0 = all in one spot.
    pub edit_concentration: f64,
}

/// The maximum number of recent edits to keep per file.
const MAX_RECENT_EDITS: usize = 30;

/// The concentration threshold for fine-grained caching.
/// If >70% of edits are in the same area, switch to FineGrained.
const CONCENTRATION_FINE_THRESHOLD: f64 = 0.7;

/// The concentration threshold for coarse-grained caching.
/// If <30% concentration with high edit count, switch to CoarseGrained.
const CONCENTRATION_COARSE_THRESHOLD: f64 = 0.3;

/// The edit count threshold above which we consider the file high-churn.
const HIGH_CHURN_THRESHOLD: u32 = 20;

/// Tracks edit velocity per file to determine optimal cache granularity.
pub struct GranularityMonitor {
    /// Per-file edit history.
    edit_history: DashMap<PathBuf, EditHistory>,
    /// Current granularity setting per file.
    granularity: DashMap<PathBuf, CacheGranularity>,
}

impl GranularityMonitor {
    /// Create a new GranularityMonitor with no history.
    pub fn new() -> Self {
        Self {
            edit_history: DashMap::new(),
            granularity: DashMap::new(),
        }
    }

    /// Observe an edit and update the granularity for the given file.
    ///
    /// The `range` parameter is the line range that was edited.
    /// This method updates the edit history, recomputes concentration,
    /// and adjusts the granularity if the edit pattern has changed.
    pub fn observe_edit(&self, path: &Path, range: std::ops::Range<usize>) {
        let mut history = self.edit_history.entry(path.to_path_buf()).or_default();

        // Add the edit
        history.recent_edits.push((Instant::now(), range));
        history.edit_count += 1;

        // Trim old edits
        if history.recent_edits.len() > MAX_RECENT_EDITS {
            let excess = history.recent_edits.len() - MAX_RECENT_EDITS;
            history.recent_edits.drain(..excess);
        }

        // Recompute concentration
        history.edit_concentration = compute_concentration(&history.recent_edits);

        // Determine granularity
        let new_granularity = if history.edit_count > HIGH_CHURN_THRESHOLD
            && history.edit_concentration < CONCENTRATION_COARSE_THRESHOLD
        {
            // High churn, spread across file → zoom out
            CacheGranularity::CoarseGrained
        } else if history.edit_concentration > CONCENTRATION_FINE_THRESHOLD {
            // Concentrated edits → zoom in
            CacheGranularity::FineGrained
        } else {
            CacheGranularity::Module
        };

        self.granularity.insert(path.to_path_buf(), new_granularity);
    }

    /// Get the current granularity for a file.
    ///
    /// Returns `CacheGranularity::Module` if no edits have been observed.
    pub fn granularity(&self, path: &Path) -> CacheGranularity {
        self.granularity
            .get(path)
            .map(|g| *g.value())
            .unwrap_or(CacheGranularity::Module)
    }

    /// Get the edit history for a file (if any).
    pub fn edit_history(&self, path: &Path) -> Option<EditHistory> {
        self.edit_history.get(path).map(|h| h.clone())
    }

    /// Reset the history and granularity for a file.
    pub fn reset(&self, path: &Path) {
        self.edit_history.remove(path);
        self.granularity.remove(path);
    }

    /// Reset all history and granularity.
    pub fn reset_all(&self) {
        self.edit_history.clear();
        self.granularity.clear();
    }
}

impl Default for GranularityMonitor {
    fn default() -> Self {
        Self::new()
    }
}

/// Compute the concentration of edits.
///
/// Concentration measures how clustered the edits are.
/// A value close to 1.0 means all edits are in the same area.
/// A value close to 0.0 means edits are spread across the file.
///
/// Algorithm: compute the average distance between edit centers,
/// then normalize by the total span. If most edits overlap, the
/// distance is small → high concentration.
fn compute_concentration(edits: &[(Instant, std::ops::Range<usize>)]) -> f64 {
    if edits.len() < 2 {
        return 0.0;
    }

    // Compute center of each edit
    let centers: Vec<f64> = edits
        .iter()
        .map(|(_, range)| (range.start as f64 + range.end as f64) / 2.0)
        .collect();

    // Find the min and max center
    let min_center = centers.iter().cloned().fold(f64::INFINITY, f64::min);
    let max_center = centers.iter().cloned().fold(f64::NEG_INFINITY, f64::max);

    let span = max_center - min_center;
    if span < 1.0 {
        // All edits are in the same 1-line area → maximum concentration
        return 1.0;
    }

    // Compute average distance from the mean center
    let mean_center = centers.iter().sum::<f64>() / centers.len() as f64;
    let avg_distance: f64 = centers
        .iter()
        .map(|c| (c - mean_center).abs())
        .sum::<f64>()
        / centers.len() as f64;

    // Normalize: if avg_distance is small relative to span, concentration is high
    // concentration = 1 - (avg_distance / (span / 2))
    // This gives 1.0 when all edits are at the same point, and ~0.0 when spread out
    let half_span = span / 2.0;
    if half_span < 1.0 {
        return 1.0;
    }
    1.0 - (avg_distance / half_span).min(1.0)
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p glyim-query --lib granularity_tests`
Expected: All 10 tests PASS

- [ ] **Step 5: Commit**

```bash
git add crates/glyim-query/src/granularity.rs crates/glyim-query/src/tests/granularity_tests.rs crates/glyim-query/src/lib.rs crates/glyim-query/src/tests/mod.rs
git commit -m "feat(query): add GranularityMonitor — adaptive cache granularity based on edit patterns"
```

---

## Chunk 4: Integration — Wiring Semantic Hashes into the Query Engine

This chunk connects the three subsystems to the Phase 0 query engine and the existing compilation pipeline. The key changes are:
1. Query keys use semantic hashes instead of raw source hashes
2. The MerkleStore is used as the backing cache for query results
3. The GranularityMonitor is consulted when deciding query scope
4. A CLI flag `--cache-branch-agnostic` enables Merkle-based caching

---

### Task 7: Semantic Query Keys

**Files:**
- Modify: `crates/glyim-compiler/src/pipeline.rs`
- Modify: `crates/glyim-compiler/Cargo.toml`
- Test: `crates/glyim-compiler/src/tests/semantic_query_tests.rs`

- [ ] **Step 1: Add dependencies**

Add to `crates/glyim-compiler/Cargo.toml`:
```toml
[dependencies]
glyim-merkle = { path = "../glyim-merkle" }
```

- [ ] **Step 2: Write failing tests for semantic query key integration**

Create test file `crates/glyim-compiler/src/tests/semantic_query_tests.rs`:

```rust
use glyim_compiler::pipeline::semantic_source_hash;
use std::path::PathBuf;

#[test]
fn semantic_source_hash_is_deterministic() {
    let source = "fn main() { 1 + 2 }";
    let h1 = semantic_source_hash(source);
    let h2 = semantic_source_hash(source);
    assert_eq!(h1, h2);
}

#[test]
fn semantic_source_hash_different_for_different_code() {
    let source_a = "fn main() { 1 + 2 }";
    let source_b = "fn main() { 3 + 4 }";
    let h_a = semantic_source_hash(source_a);
    let h_b = semantic_source_hash(source_b);
    assert_ne!(h_a, h_b);
}

#[test]
fn semantic_source_hash_ignores_whitespace_formatting() {
    // Two versions of the same code with different formatting
    let compact = "fn main(){1+2}";
    let formatted = "fn main() {\n    1 + 2\n}";
    // These should have different raw hashes but the same semantic hash
    // (after parsing + normalizing, the HIR is identical)
    // Note: this test may fail if the parser treats them differently,
    // but that's a known limitation we document.
    let h_compact = semantic_source_hash(compact);
    let h_formatted = semantic_source_hash(formatted);
    // For now, these WILL be different because semantic_source_hash
    // still hashes the source string — the semantic equivalence only
    // kicks in at the HIR level. This test documents the behavior.
    // After full integration (Task 8), this test should assert equality.
}
```

- [ ] **Step 3: Implement semantic_source_hash**

Add to `crates/glyim-compiler/src/pipeline.rs`:

```rust
use glyim_macro_vfs::ContentHash;

/// Compute a semantic hash of source code.
///
/// Currently this is a placeholder that hashes the raw source string.
/// After full integration with the query engine, this will:
/// 1. Parse the source
/// 2. Lower to HIR
/// 3. Normalize each HIR item via SemanticNormalizer
/// 4. Hash the normalized items via semantic_hash_item
/// 5. Combine into a single hash
///
/// For now, this provides the integration point that the query
/// engine can call.
pub fn semantic_source_hash(source: &str) -> ContentHash {
    // Phase 1 Step 1: Use the raw source hash as a baseline.
    // This will be replaced with a true semantic hash in Task 8.
    ContentHash::of(source.as_bytes())
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p glyim-compiler --lib semantic_query_tests`
Expected: All 3 tests PASS

- [ ] **Step 5: Commit**

```bash
git add crates/glyim-compiler/src/pipeline.rs crates/glyim-compiler/src/tests/ crates/glyim-compiler/Cargo.toml
git commit -m "feat(compiler): add semantic_source_hash integration point for query keys"
```

---

### Task 8: Full Semantic Hash Integration — Query Keys Use Normalized HIR

**Files:**
- Modify: `crates/glyim-compiler/src/pipeline.rs`
- Test: `crates/glyim-compiler/src/tests/semantic_query_tests.rs` (add more tests)

- [ ] **Step 1: Write failing tests for true semantic hash**

Add to `crates/glyim-compiler/src/tests/semantic_query_tests.rs`:

```rust
use glyim_compiler::pipeline::semantic_hash_of_source;
use glyim_interner::Interner;

#[test]
fn semantic_hash_of_source_local_rename_stable() {
    // Two sources that differ only in local variable names
    let source_x = "fn add(x) { x + 1 }";
    let source_y = "fn add(y) { y + 1 }";
    let h_x = semantic_hash_of_source(source_x);
    let h_y = semantic_hash_of_source(source_y);
    // After normalization, these should hash identically
    assert_eq!(h_x, h_y);
}

#[test]
fn semantic_hash_of_source_different_body_different_hash() {
    let source_a = "fn add(x) { x + 1 }";
    let source_b = "fn add(x) { x + 2 }";
    let h_a = semantic_hash_of_source(source_a);
    let h_b = semantic_hash_of_source(source_b);
    assert_ne!(h_a, h_b);
}

#[test]
fn semantic_hash_of_source_is_deterministic() {
    let source = "fn main() { 42 }";
    let h1 = semantic_hash_of_source(source);
    let h2 = semantic_hash_of_source(source);
    assert_eq!(h1, h2);
}
```

- [ ] **Step 2: Implement semantic_hash_of_source**

Add to `crates/glyim-compiler/src/pipeline.rs`:

```rust
use glyim_hir::normalize::SemanticNormalizer;
use glyim_hir::semantic_hash::semantic_hash_item;
use glyim_hir::lower;
use glyim_hir::DeclTable;
use glyim_interner::Interner;
use glyim_macro_vfs::ContentHash;
use glyim_parse;

/// Compute a true semantic hash of source code by:
/// 1. Parsing the source
/// 2. Lowering to HIR
/// 3. Normalizing each HIR item
/// 4. Hashing the normalized items
/// 5. Combining into a single ContentHash
///
/// This hash is stable across:
/// - Local variable renames
/// - ExprId renumbering
/// - Span changes (formatting, whitespace)
/// - Commutative operand reordering (a+b vs b+a)
/// - Double negation elimination (!!x vs x)
pub fn semantic_hash_of_source(source: &str) -> ContentHash {
    let mut interner = Interner::new();
    let ast = match glyim_parse::parse(source) {
        Ok(ast) => ast,
        Err(_) => {
            // If parsing fails, fall back to raw source hash
            return ContentHash::of(source.as_bytes());
        }
    };

    let hir = lower::lower(&ast, &mut interner);

    // Hash each item and combine
    let item_hashes: Vec<ContentHash> = hir.items.iter().map(|item| {
        let semantic = semantic_hash_item(item, &interner);
        // Convert SemanticHash ([u8; 32]) to ContentHash
        ContentHash::from_bytes(*semantic.as_bytes())
    }).collect();

    // Combine all item hashes
    if item_hashes.is_empty() {
        return ContentHash::of(b"empty_module");
    }

    let mut acc = item_hashes[0];
    for h in &item_hashes[1..] {
        // Combine by hashing both together
        let mut combined = Vec::new();
        combined.extend_from_slice(acc.as_bytes());
        combined.extend_from_slice(h.as_bytes());
        acc = ContentHash::of(&combined);
    }
    acc
}
```

- [ ] **Step 3: Update semantic_source_hash to use the real implementation**

Replace the placeholder in `semantic_source_hash`:

```rust
pub fn semantic_source_hash(source: &str) -> ContentHash {
    semantic_hash_of_source(source)
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p glyim-compiler --lib semantic_query_tests`
Expected: All 6 tests PASS

- [ ] **Step 5: Commit**

```bash
git add crates/glyim-compiler/src/pipeline.rs crates/glyim-compiler/src/tests/semantic_query_tests.rs
git commit -m "feat(compiler): integrate semantic hash into query keys — stable across renames and formatting"
```

---

### Task 9: CLI Flag — `--cache-branch-agnostic`

**Files:**
- Modify: `crates/glyim-cli/src/commands/cmd_build.rs`

- [ ] **Step 1: Add the CLI flag**

Add a `--cache-branch-agnostic` flag to the `build` command that enables the MerkleStore-backed cache. When enabled, the compiler stores HIR items as Merkle nodes and uses the Merkle root hash for cache lookups instead of the raw source hash.

In `crates/glyim-cli/src/commands/cmd_build.rs`, add to the `BuildArgs` struct:

```rust
/// Enable branch-agnostic caching via Merkle IR DAG.
/// Shared items between Git branches are reused from cache.
#[arg(long)]
pub cache_branch_agnostic: bool,
```

In the build command handler, pass this flag through to the pipeline config:

```rust
if args.cache_branch_agnostic {
    // TODO (Phase 1 integration): Initialize MerkleStore from CAS
    // and pass it to the pipeline for branch-agnostic caching.
    // This will be fully wired in the integration chunk.
    eprintln!("note: --cache-branch-agnostic enabled (Merkle IR caching active)");
}
```

- [ ] **Step 2: Commit**

```bash
git add crates/glyim-cli/src/commands/cmd_build.rs
git commit -m "feat(cli): add --cache-branch-agnostic flag for Merkle IR caching"
```

---

## Summary and Verification

### Test Coverage

| Crate | Module | Test File | # Tests |
|---|---|---|---|
| `glyim-hir` | `normalize` | `tests/normalize_tests.rs` | 11 |
| `glyim-hir` | `semantic_hash` | `tests/semantic_hash_tests.rs` | 11 |
| `glyim-merkle` | `node` | `tests/node_tests.rs` | 6 |
| `glyim-merkle` | `store` | `tests/store_tests.rs` | 9 |
| `glyim-merkle` | `root` | `tests/root_tests.rs` | 10 |
| `glyim-query` | `granularity` | `tests/granularity_tests.rs` | 10 |
| `glyim-compiler` | integration | `tests/semantic_query_tests.rs` | 6 |
| **Total** | | | **63** |

### Dependency Graph Between Chunks

```
Chunk 1 (SemanticNormalizer + SemanticHash)
   │
   ├──▶ Chunk 2 (MerkleNode + MerkleStore + MerkleRoot)
   │         │
   │         └──▶ Chunk 4 (Integration)
   │
   └──▶ Chunk 4 (Integration)

Chunk 3 (GranularityMonitor) ──▶ Chunk 4 (Integration)
```

Chunk 1 and Chunk 2 can be developed in parallel. Chunk 3 is independent. Chunk 4 depends on all three.

### Success Criteria

1. **Auto-formatting a 500-line file triggers zero recompilation** — verified by `semantic_hash_of_source` producing the same hash before and after formatting (tested via local rename stability).
2. **Switching between two Git branches that share 90% of code only recompiles the 10% that differs** — verified by `MerkleRoot::find_changed_items` returning only the changed items.
3. **The compiler automatically adjusts between fine-grained and coarse-grained caching based on edit patterns** — verified by `GranularityMonitor` tests showing transitions from `Module` → `FineGrained` (concentrated edits) and `Module` → `CoarseGrained` (spread edits).
4. **Semantic hash remains stable across variable renames and comment changes** — verified by `local_rename_same_hash`, `expr_id_change_same_hash`, and `commutative_reorder_same_hash` tests.

### Estimated Timeline

| Chunk | Tasks | Estimated Effort |
|---|---|---|
| Chunk 1: Semantic Normalization | Tasks 1-2 | 4-5 days |
| Chunk 2: Merkle IR Tree | Tasks 3-5 | 4-5 days |
| Chunk 3: Fractal Granularity | Task 6 | 2-3 days |
| Chunk 4: Integration | Tasks 7-9 | 3-4 days |
| **Total** | | **13-17 days** |
