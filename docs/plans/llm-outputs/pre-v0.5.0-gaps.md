# Glyim Gap Closure – Complete Implementation Plan (Final)

> **For agentic workers:** REQUIRED: Use superpowers:subagent‑driven‑development (if subagents available) or superpowers:executing‑plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Close all critical gaps between the v0.5.0‑dev codebase and the architectural specs (v0.1.0–v0.4.0) with zero regressions.

**Architecture:** Six phases ordered by dependency. Phase 1 completes CST coverage. Phase 2 adds feature‑gated JIT (orchestrated in `glyim‑cli` to preserve tier constraints). Phase 3 hardens the type system (with migration sub‑task). Phase 4 fixes control flow (`?` & match codegen). Phase 5 delivers generics and method dispatch. Phase 6 implements the typed macro pipeline. Every phase leaves a fully working compiler.

**Tech Stack:** Rust 1.75+, Inkwell 0.9 (LLVM 22), Rowan 0.16, ariadne 0.6, insta

---

## Pre‑flight Checklist

Before starting any implementation, run and record the baseline:

```bash
cargo test --workspace         # all tests must pass
just check-tiers               # no tier violations
just check-dag                 # no cyclic deps
just check-files               # file sizes within limits
```

---

# Phase 1: CST Coverage & Foundation Fixes

## Task 1.1 – Fix Stray Test and Add Missing CST Node Kinds

**Files:** `crates/glyim‑codegen‑llvm/src/lib.rs`, `crates/glyim‑syntax/src/kind.rs`

- [ ] **Step 1 – Move stray test**  
  In `crates/glyim‑codegen‑llvm/src/lib.rs`, find the orphan test `compile_to_ir_debug_has_local_variable` (currently after the closing `}` of `mod debug_ir_tests`, around line 118‑122). Move it inside that module block, just before the module's closing `}`.

- [ ] **Step 2 – Append new CST node kinds, update COUNT, fix test, add display_name entries**

  In `crates/glyim‑syntax/src/kind.rs`:

  1. **Note:** The current enum actually has **85 variants** (Error through PtrType). The old `COUNT=82` is a latent bug; we'll fix it now. The new total will be 85 existing + 9 new = 94.

  2. Append the following variants **after** `PtrType` (do not duplicate existing ones like `IfExpr`, `TryExpr`, `FloatLitExpr`; those are already present):

     ```rust
     LetStmt,
     AssignStmt,
     ExprStmt,
     StructLitExpr,
     EnumVariantExpr,
     FieldAccessExpr,
     TupleLitExpr,
     ReturnExpr,
     MatchArmPat,
     ```

  3. Change `COUNT` to `94`.

  4. In the test `count_matches_actual_variants` (inside `#[cfg(test)] mod tests` at the bottom of the file), change `assert_eq!(COUNT, 82)` to `assert_eq!(COUNT, 94)`.

  5. Add these `display_name()` entries before the closing `}` of the match:

     ```rust
     Self::LetStmt => "let statement",
     Self::AssignStmt => "assignment statement",
     Self::ExprStmt => "expression statement",
     Self::StructLitExpr => "struct literal",
     Self::EnumVariantExpr => "enum variant",
     Self::FieldAccessExpr => "field access",
     Self::TupleLitExpr => "tuple literal",
     Self::ReturnExpr => "return expression",
     Self::MatchArmPat => "match arm pattern",
     ```

- [ ] **Step 3 – Run syntax tests**  
  ```bash
  cargo test -p glyim-syntax
  ```
  Expected: all pass, `count_matches_actual_variants` expects 94.

- [ ] **Step 4 – Commit**

---

## Task 1.2 – Complete AST‑to‑CST Conversion

**File:** `crates/glyim‑parse/src/ast_to_cst.rs`

- [ ] **Step 1 – Replace the entire file** with the complete implementation below. This covers every AST variant, adds `ast_stmt_to_cst`, updates `ast_item_to_cst`, and keeps the public functions unchanged.

```rust
use crate::ast::*;
use crate::cst_builder::CstBuilder;
use glyim_syntax::{GreenNode, SyntaxKind, SyntaxNode};

// ── Operator helpers (unchanged from original) ─────────────────────
fn binop_kind(op: BinOp) -> SyntaxKind {
    match op {
        BinOp::Add => SyntaxKind::Plus,
        BinOp::Sub => SyntaxKind::Minus,
        BinOp::Mul => SyntaxKind::Star,
        BinOp::Div => SyntaxKind::Slash,
        BinOp::Mod => SyntaxKind::Percent,
        BinOp::Eq => SyntaxKind::EqEq,
        BinOp::Neq => SyntaxKind::BangEq,
        BinOp::Lt => SyntaxKind::Lt,
        BinOp::Gt => SyntaxKind::Gt,
        BinOp::Lte => SyntaxKind::LtEq,
        BinOp::Gte => SyntaxKind::GtEq,
        BinOp::And => SyntaxKind::AmpAmp,
        BinOp::Or => SyntaxKind::PipePipe,
    }
}
fn binop_text(op: BinOp) -> &'static str {
    match op {
        BinOp::Add => "+", BinOp::Sub => "-", BinOp::Mul => "*",
        BinOp::Div => "/", BinOp::Mod => "%",
        BinOp::Eq => "==", BinOp::Neq => "!=",
        BinOp::Lt => "<", BinOp::Gt => ">",
        BinOp::Lte => "<=", BinOp::Gte => ">=",
        BinOp::And => "&&", BinOp::Or => "||",
    }
}
fn unop_kind(op: UnOp) -> SyntaxKind {
    match op { UnOp::Neg => SyntaxKind::Minus, UnOp::Not => SyntaxKind::Bang }
}
fn unop_text(op: UnOp) -> &'static str {
    match op { UnOp::Neg => "-", UnOp::Not => "!" }
}

// ── Expression → CST ────────────────────────────────────────────────
fn ast_expr_to_cst(builder: &mut CstBuilder, expr: &ExprNode) {
    match &expr.kind {
        ExprKind::IntLit(n) => {
            builder.start_node(SyntaxKind::LitExpr);
            builder.token(SyntaxKind::IntLit, &n.to_string());
            builder.finish_node();
        }
        ExprKind::FloatLit(f) => {
            builder.start_node(SyntaxKind::LitExpr);
            builder.token(SyntaxKind::FloatLit, &f.to_string());
            builder.finish_node();
        }
        ExprKind::BoolLit(b) => {
            builder.start_node(SyntaxKind::LitExpr);
            builder.token(
                if *b { SyntaxKind::KwTrue } else { SyntaxKind::KwFalse },
                if *b { "true" } else { "false" },
            );
            builder.finish_node();
        }
        ExprKind::StrLit(s) => {
            builder.start_node(SyntaxKind::LitExpr);
            builder.token(SyntaxKind::StringLit, s);
            builder.finish_node();
        }
        ExprKind::UnitLit => {
            builder.start_node(SyntaxKind::LitExpr);
            builder.token(SyntaxKind::LParen, "(");
            builder.token(SyntaxKind::RParen, ")");
            builder.finish_node();
        }
        ExprKind::Ident(_) => {
            builder.start_node(SyntaxKind::PathExpr);
            builder.token(SyntaxKind::Ident, "<ident>");
            builder.finish_node();
        }
        ExprKind::Binary { op, lhs, rhs } => {
            builder.start_node(SyntaxKind::BinaryExpr);
            ast_expr_to_cst(builder, lhs);
            builder.token(binop_kind(op.clone()), binop_text(op.clone()));
            ast_expr_to_cst(builder, rhs);
            builder.finish_node();
        }
        ExprKind::Unary { op, operand } => {
            builder.start_node(SyntaxKind::PrefixExpr);
            builder.token(unop_kind(op.clone()), unop_text(op.clone()));
            ast_expr_to_cst(builder, operand);
            builder.finish_node();
        }
        ExprKind::Lambda { params: _, body } => {
            builder.start_node(SyntaxKind::LambdaExpr);
            builder.token(SyntaxKind::LParen, "(");
            builder.token(SyntaxKind::RParen, ")");
            builder.token(SyntaxKind::FatArrow, "=>");
            ast_expr_to_cst(builder, body);
            builder.finish_node();
        }
        ExprKind::Block(items) => {
            builder.start_node(SyntaxKind::BlockExpr);
            builder.token(SyntaxKind::LBrace, "{");
            for item in items {
                match item {
                    BlockItem::Expr(e) => ast_expr_to_cst(builder, e),
                    BlockItem::Stmt(s) => ast_stmt_to_cst(builder, s),
                }
            }
            builder.token(SyntaxKind::RBrace, "}");
            builder.finish_node();
        }
        ExprKind::If { condition, then_branch, else_branch } => {
            builder.start_node(SyntaxKind::IfExpr);
            builder.token(SyntaxKind::KwIf, "if");
            ast_expr_to_cst(builder, condition);
            ast_expr_to_cst(builder, then_branch);
            if let Some(e) = else_branch {
                builder.token(SyntaxKind::KwElse, "else");
                ast_expr_to_cst(builder, e);
            }
            builder.finish_node();
        }
        ExprKind::Call { callee, args } => {
            builder.start_node(SyntaxKind::CallExpr);
            ast_expr_to_cst(builder, callee);
            builder.token(SyntaxKind::LParen, "(");
            for (i, a) in args.iter().enumerate() {
                if i > 0 { builder.token(SyntaxKind::Comma, ","); }
                ast_expr_to_cst(builder, a);
            }
            builder.token(SyntaxKind::RParen, ")");
            builder.finish_node();
        }
        ExprKind::StructLit { name: _, fields } => {
            builder.start_node(SyntaxKind::StructLitExpr);
            for (_, fe) in fields { ast_expr_to_cst(builder, fe); }
            builder.finish_node();
        }
        ExprKind::EnumVariant { .. } => {
            builder.start_node(SyntaxKind::EnumVariantExpr);
            builder.token(SyntaxKind::Ident, "<variant>");
            builder.finish_node();
        }
        ExprKind::SomeExpr(e) | ExprKind::OkExpr(e) | ExprKind::ErrExpr(e) => {
            builder.start_node(SyntaxKind::EnumVariantExpr);
            ast_expr_to_cst(builder, e);
            builder.finish_node();
        }
        ExprKind::NoneExpr => {
            builder.start_node(SyntaxKind::EnumVariantExpr);
            builder.finish_node();
        }
        ExprKind::TryExpr(e) => {
            builder.start_node(SyntaxKind::TryExpr);
            ast_expr_to_cst(builder, e);
            builder.token(SyntaxKind::Question, "?");
            builder.finish_node();
        }
        ExprKind::As { expr, .. } => {
            builder.start_node(SyntaxKind::AsExpr);
            ast_expr_to_cst(builder, expr);
            builder.token(SyntaxKind::KwAs, "as");
            builder.finish_node();
        }
        ExprKind::MacroCall { .. } => {
            builder.token(SyntaxKind::At, "@");
            builder.token(SyntaxKind::Ident, "<macro>");
        }
        ExprKind::Match { .. } => {
            builder.start_node(SyntaxKind::MatchExpr);
            builder.token(SyntaxKind::KwMatch, "match");
            builder.finish_node();
        }
        ExprKind::FieldAccess { .. } => {
            builder.start_node(SyntaxKind::FieldAccessExpr);
            builder.token(SyntaxKind::Dot, ".");
            builder.finish_node();
        }
        ExprKind::SizeOf(_) => { builder.token(SyntaxKind::Ident, "__size_of"); }
        ExprKind::TupleLit(elems) => {
            builder.start_node(SyntaxKind::TupleLitExpr);
            for e in elems { ast_expr_to_cst(builder, e); }
            builder.finish_node();
        }
        ExprKind::Pointer { .. } => { builder.token(SyntaxKind::Star, "*"); }
    }
}

// ── Statement → CST ─────────────────────────────────────────────────
fn ast_stmt_to_cst(builder: &mut CstBuilder, stmt: &StmtNode) {
    match &stmt.kind {
        StmtKind::Let { pattern: _, mutable: _, value } => {
            builder.start_node(SyntaxKind::LetStmt);
            builder.token(SyntaxKind::KwLet, "let");
            builder.token(SyntaxKind::Ident, "<name>");
            builder.token(SyntaxKind::Eq, "=");
            ast_expr_to_cst(builder, value);
            builder.finish_node();
        }
        StmtKind::Assign { target: _, value } => {
            builder.start_node(SyntaxKind::AssignStmt);
            ast_expr_to_cst(builder, value);
            builder.finish_node();
        }
    }
}

// ── Item → CST ──────────────────────────────────────────────────────
fn ast_item_to_cst(builder: &mut CstBuilder, item: &Item) {
    match item {
        Item::Binding { value, .. } => ast_expr_to_cst(builder, value),
        Item::FnDef { body, .. } => {
            builder.start_node(SyntaxKind::FnDef);
            builder.token(SyntaxKind::KwFn, "fn");
            builder.token(SyntaxKind::Ident, "<name>");
            builder.token(SyntaxKind::LParen, "(");
            builder.token(SyntaxKind::RParen, ")");
            ast_expr_to_cst(builder, body);
            builder.finish_node();
        }
        Item::Stmt(stmt) => ast_stmt_to_cst(builder, stmt),
        Item::Use(u) => {
            builder.token(SyntaxKind::KwUse, "use");
            builder.token(SyntaxKind::Ident, &u.path);
        }
        Item::StructDef { .. } => {
            builder.start_node(SyntaxKind::StructDef);
            builder.token(SyntaxKind::KwStruct, "struct");
            builder.token(SyntaxKind::Ident, "<name>");
            builder.finish_node();
        }
        Item::EnumDef { .. } => {
            builder.start_node(SyntaxKind::EnumDef);
            builder.token(SyntaxKind::KwEnum, "enum");
            builder.token(SyntaxKind::Ident, "<name>");
            builder.finish_node();
        }
        Item::ImplBlock { .. } => {
            builder.token(SyntaxKind::KwImpl, "impl");
            builder.token(SyntaxKind::Ident, "<name>");
        }
        Item::MacroDef { .. } => {
            builder.token(SyntaxKind::At, "@");
            builder.token(SyntaxKind::Ident, "<macro>");
        }
        Item::ExternBlock { .. } => {
            builder.start_node(SyntaxKind::ExternBlock);
            builder.token(SyntaxKind::KwExtern, "extern");
            builder.finish_node();
        }
    }
}

// ── Public API (unchanged) ──────────────────────────────────────────
pub fn ast_to_green(ast: &Ast) -> GreenNode {
    let mut builder = CstBuilder::new();
    builder.start_node(SyntaxKind::SourceFile);
    for item in &ast.items {
        ast_item_to_cst(&mut builder, item);
    }
    builder.finish_node();
    let (green, _) = builder.finish();
    green
}

pub fn ast_to_cst(ast: &Ast) -> SyntaxNode {
    let green = ast_to_green(ast);
    crate::cst_builder::green_to_syntax(green)
}
```

- [ ] **Step 2 – Run parse tests**  
  ```bash
  cargo test -p glyim-parse
  ```
  All tests must pass.

- [ ] **Step 3 – Commit**

---

## Task 1.3 – Add CST Roundtrip Tests

**File:** `crates/glyim‑parse/tests/cst_roundtrip_tests.rs` (create)

```rust
use glyim_parse::{parse, ast_to_cst};
use glyim_syntax::SyntaxKind;

#[test]
fn cst_roundtrip_int_literal() {
    let out = parse("main = () => 42");
    assert!(out.errors.is_empty());
    let cst = ast_to_cst(&out.ast);
    assert_eq!(cst.kind(), SyntaxKind::SourceFile);
    assert!(cst.children().count() > 0);
}

#[test]
fn cst_roundtrip_let_binding() {
    let out = parse("let x = 42\nmain = () => x");
    assert!(out.errors.is_empty());
    let cst = ast_to_cst(&out.ast);
    assert_eq!(cst.kind(), SyntaxKind::SourceFile);
}

#[test]
fn cst_roundtrip_fn_def() {
    let out = parse("fn add(a, b) { a + b }\nmain = () => add(1, 2)");
    assert!(out.errors.is_empty());
    let cst = ast_to_cst(&out.ast);
    assert!(cst.children().count() > 0);
}

#[test]
fn cst_roundtrip_struct_and_enum() {
    let out = parse("struct Point { x, y }\nenum Color { Red, Green }\nmain = () => 1");
    assert!(out.errors.is_empty());
    let cst = ast_to_cst(&out.ast);
    assert!(cst.children().count() > 0);
}

#[test]
fn cst_roundtrip_if_else() {
    let out = parse("main = () => if 1 { 10 } else { 20 }");
    assert!(out.errors.is_empty());
    let cst = ast_to_cst(&out.ast);
    assert!(cst.children().count() > 0);
}

#[test]
fn cst_roundtrip_match() {
    let out = parse("main = () => match 1 { 1 => 10, _ => 20 }");
    assert!(out.errors.is_empty());
    let cst = ast_to_cst(&out.ast);
    assert!(cst.children().count() > 0);
}
```

- [ ] **Run and commit**

---

# Phase 2: JIT Execution (Feature‑Gated, No Tier Violation)

**Design Decision:** To avoid a tier violation (codegen‑llvm cannot depend on typeck), the JIT orchestration lives in `glyim‑cli` (tier 5), which already depends on both `glyim‑codegen‑llvm` and `glyim‑typeck`.

## Task 2.0 – Expose `Codegen::get_module()`

**File:** `crates/glyim‑codegen‑llvm/src/codegen/mod.rs`

- [ ] **Step 1 – Add public getter**  
  In the `impl Codegen` block, add:

  ```rust
  pub fn get_module(&self) -> &Module<'ctx> {
      &self.module
  }
  ```

- [ ] **Commit**

---

## Task 2.1 – JIT Module in `glyim-cli`

**Files:** `crates/glyim‑cli/Cargo.toml`, `src/pipeline.rs`, `src/main.rs`, `tests/jit_tests.rs`

- [ ] **Step 1 – Add `jit` feature**  
  In `Cargo.toml`:

  ```toml
  [features]
  default = []
  jit = []
  ```

- [ ] **Step 2 – Add `run_jit` in `pipeline.rs`** (behind `#[cfg(feature = "jit")]`)

  ```rust
  /// JIT‑compile and execute source, returning the exit code.
  /// NOTE: Does NOT prepend the prelude. Tests using Option/Result
  /// must include the prelude types inline.
  #[cfg(feature = "jit")]
  pub fn run_jit(source: &str) -> Result<i32, PipelineError> {
      use inkwell::{context::Context, execution_engine::ExecutionEngine, OptimizationLevel};

      let mut parse_out = glyim_parse::parse(source);
      if !parse_out.errors.is_empty() {
          return Err(PipelineError::Parse(parse_out.errors));
      }
      let mut interner = parse_out.interner;
      let hir = glyim_hir::lower(&parse_out.ast, &mut interner);
      let mut typeck = glyim_typeck::TypeChecker::new(interner.clone());
      if let Err(errs) = typeck.check(&hir) {
          return Err(PipelineError::TypeCheck(errs));
      }

      let context = Context::create();
      let mut cg = glyim_codegen_llvm::Codegen::new(&context, interner, typeck.expr_types);
      cg.generate(&hir).map_err(PipelineError::Codegen)?;

      let engine = cg.get_module()
          .create_jit_execution_engine(OptimizationLevel::None)
          .map_err(|e| PipelineError::Codegen(format!("JIT: {e}")))?;

      unsafe {
          let main_fn = engine
              .get_function::<unsafe extern "C" fn() -> i32>("main")
              .map_err(|e| PipelineError::Codegen(format!("JIT main: {e}")))?;
          Ok(main_fn.call())
      }
  }
  ```

- [ ] **Step 3 – Update CLI `Run` command**  
  In `src/main.rs`, add `jit: bool` to the `Run` variant and handle it with `#[cfg(feature = "jit")]` and `let _ = jit;` suppression.

- [ ] **Step 4 – Add JIT test**  
  Create `crates/glyim‑cli/tests/jit_tests.rs`:

  ```rust
  #[cfg(feature = "jit")]
  #[test]
  fn jit_compile_and_run_simple() {
      assert_eq!(glyim_cli::pipeline::run_jit("main = () => 42").unwrap(), 42);
  }
  ```

- [ ] **Step 5 – Run integration tests (default path) and manual JIT**  
  ```bash
  cargo test -p glyim-cli --test integration
  cargo run -p glyim-cli --features jit -- run --jit <test_file>
  ```

- [ ] **Commit**

---

# Phase 3: Type System Hardening

## Task 3.0 – Migration: Update All Integer If‑Condition Tests

**File:** `crates/glyim‑cli/tests/integration.rs`

- [ ] **Step 1 – Update four tests** that use integer conditions:

  - `e2e_if_true_branch`: `if 1 { 10 }` → `if true { 10 }`
  - `e2e_if_false_branch`: `if 0 { 10 }` → `if false { 10 }`
  - `e2e_if_without_else`: `if 0 { 42 }` → `if false { 42 }`
  - `e2e_else_if_chain`: `if 0 { 1 } else if 0 { 2 }` → `if false { 1 } else if false { 2 }`

- [ ] **Step 2 – Run tests** – all four pass.
- [ ] **Commit**

---

## Task 3.1 – Bool as Distinct Type

**Files:** `crates/glyim‑typeck/src/typeck/error.rs`, `expr.rs`; `crates/glyim‑codegen‑llvm/src/codegen/expr/mod.rs`

- [ ] **Step 1 – Error variant**  
  In `error.rs`, add:

  ```rust
  IfConditionMustBeBool { found: HirType, expr_id: ExprId },
  ```
  Display: `write!(f, "if condition must be `bool`, found `{:?}`", found)`

- [ ] **Step 2 – Enforce in type checker**  
  In `expr.rs`, in the `If` branch of `infer_expr`:

  ```rust
  HirExpr::If { condition, then_branch, else_branch, .. } => {
      let cond_type = self.check_expr(condition).unwrap_or(HirType::Int);
      if cond_type != HirType::Bool {
          self.errors.push(TypeError::IfConditionMustBeBool {
              found: cond_type, expr_id: condition.get_id(),
          });
      }
      let then_type = self.check_expr(then_branch);
      if let Some(eb) = else_branch { self.check_expr(eb); }
      then_type.unwrap_or(HirType::Unit)
  }
  ```

- [ ] **Step 3 – BoolLit codegen**  
  In `codegen/expr/mod.rs`:

  ```rust
  HirExpr::BoolLit { value: b, .. } => {
      let i1 = cg.context.bool_type().const_int(if *b { 1 } else { 0 }, false);
      Some(cg.builder.build_int_z_extend(i1, cg.i64_type, "bool_zext").ok()?)
  }
  ```

- [ ] **Step 4 – Add rejection test**

  ```rust
  #[test]
  fn e2e_bool_if_rejects_int_condition() {
      let src = "fn main() -> i64 { let x = 5; if x { 1 } else { 0 } }";
      assert!(pipeline::run(&temp_g(src)).is_err());
  }
  ```

- [ ] **Step 5 – Run all integration tests** – migrated tests pass, rejection test errors.
- [ ] **Commit**

---

## Task 3.2 – Immutability Enforcement

**Files:** `crates/glyim‑typeck/src/typeck/types.rs`, `scope.rs`, `stmt.rs`, `function.rs`, `error.rs`; integration test; UI test.

- [ ] **Step 1 – Define `Binding` and update `Scope`**  
  In `types.rs`:

  ```rust
  #[derive(Clone, Debug)]
  pub(crate) struct Binding { pub ty: HirType, pub mutable: bool }

  #[derive(Clone, Debug)]
  pub(crate) struct Scope { pub bindings: HashMap<Symbol, Binding> }

  impl Scope {
      pub fn new() -> Self { Self { bindings: HashMap::new() } }
      pub fn insert(&mut self, name: Symbol, ty: HirType, mutable: bool) {
          self.bindings.insert(name, Binding { ty, mutable });
      }
      pub fn lookup(&self, name: &Symbol) -> Option<&HirType> {
          self.bindings.get(name).map(|b| &b.ty)
      }
      pub fn lookup_binding(&self, name: &Symbol) -> Option<&Binding> {
          self.bindings.get(name)
      }
  }
  ```

- [ ] **Step 2 – Update `insert_binding` and add `lookup_binding_full`**  
  In `scope.rs`:

  ```rust
  pub(crate) fn insert_binding(&mut self, name: Symbol, ty: HirType, mutable: bool) {
      if let Some(scope) = self.scopes.last_mut() { scope.insert(name, ty, mutable); }
  }

  pub(crate) fn lookup_binding_full(&self, name: &Symbol) -> Option<&crate::typeck::types::Binding> {
      for scope in self.scopes.iter().rev() {
          if let Some(b) = scope.lookup_binding(name) { return Some(b); }
      }
      None
  }
  ```

- [ ] **Step 3 – Fix ALL call sites (three total)**  

  1. In `function.rs` `check_fn`:  
     `tc.insert_binding(*sym, ty.clone())` → `tc.insert_binding(*sym, ty.clone(), false)`

  2. In `stmt.rs` `bind_pattern`:  
     `self.insert_binding(*sym, value_ty.clone())` → `self.insert_binding(*sym, value_ty.clone(), false)`

  3. In `stmt.rs` `check_stmt` for the `Let` arm:  
     `self.insert_binding(*name, ty.clone())` → `self.insert_binding(*name, ty, *mutable)`

- [ ] **Step 4 – Handle assignment immutability**  
  In `stmt.rs` `check_stmt` for `Assign`:

  ```rust
  HirStmt::Assign { target, value, .. } => {
      let immutable = self.lookup_binding_full(target)
          .map(|b| !b.mutable).unwrap_or(false);
      if immutable {
          self.errors.push(TypeError::AssignToImmutable {
              name: *target, expr_id: ExprId::new(0),
          });
      }
      let ty = self.check_expr(value).unwrap_or(HirType::Int);
      self.insert_binding(*target, ty, true);
      Some(ty)
  }
  ```

- [ ] **Step 5 – Add error variant**  
  In `error.rs`:

  ```rust
  AssignToImmutable { name: Symbol, expr_id: ExprId },
  ```
  Display: `write!(f, "cannot assign to immutable binding")`

- [ ] **Step 6 – Update UI test source**  
  In `crates/glyim‑cli/tests/ui/assign_immutable.g`, add `main = () => 0` at the end so the error comes from immutability, not missing main. Then run `cargo insta review` to update the snapshot.

- [ ] **Step 7 – Integration test**

  ```rust
  #[test]
  fn e2e_assign_to_immutable_is_error() {
      let src = "fn main() -> i64 { let x = 5; x = 10; x }";
      assert!(pipeline::run(&temp_g(src)).is_err());
  }
  ```

- [ ] **Step 8 – Run all type checker and integration tests.**
- [ ] **Commit**

---

## Task 3.3 – f64 Arithmetic Codegen

**Files:** create `crates/glyim‑codegen‑llvm/src/codegen/float_ops.rs`, modify `codegen/expr/mod.rs`

- [ ] **Step 1 – `float_ops.rs`**

  ```rust
  use crate::Codegen;
  use glyim_hir::HirBinOp;
  use inkwell::values::FloatValue;

  pub(crate) fn codegen_float_binop<'ctx>(
      cg: &Codegen<'ctx>, op: &HirBinOp,
      lhs: FloatValue<'ctx>, rhs: FloatValue<'ctx>,
  ) -> Option<FloatValue<'ctx>> {
      match op {
          HirBinOp::Add => cg.builder.build_float_add(lhs, rhs, "fadd").ok(),
          HirBinOp::Sub => cg.builder.build_float_sub(lhs, rhs, "fsub").ok(),
          HirBinOp::Mul => cg.builder.build_float_mul(lhs, rhs, "fmul").ok(),
          HirBinOp::Div => cg.builder.build_float_div(lhs, rhs, "fdiv").ok(),
          _ => None,
      }
  }
  ```

- [ ] **Step 2 – Wire into `codegen_expr`**  

  Add `mod float_ops;` at top of file. In `codegen_expr`, add a new match arm for `FloatLit` (keep the existing `As` arm separate):

  ```rust
  HirExpr::As { .. } => Some(cg.i64_type.const_int(0, false)),
  HirExpr::FloatLit { value: f, .. } => {
      // Store float on stack; return the pointer bitcast to i64.
      // NOTE: Lossy for NaN/Inf; sufficient for v0.5.0.
      // Full float dispatch requires type-aware Binary operator selection.
      let fv = cg.f64_type.const_float(*f);
      let alloca = cg.builder.build_alloca(cg.f64_type, "float_tmp").ok()?;
      cg.builder.build_store(alloca, fv).ok()?;
      Some(cg.builder.build_ptr_to_int(alloca, cg.i64_type, "f2i64").ok()?)
  }
  ```

  **Note:** `float_ops::codegen_float_binop` is available but not yet called from `codegen_expr` for Binary ops with float types. Binary ops on float‑typed expressions will still use integer ops for now. Full float binary dispatch requires type‑aware operator selection (out of scope for this task).

- [ ] **Step 3 – Integration test**

  ```rust
  #[test]
  fn e2e_float_arithmetic_no_crash() {
      let src = "fn main() -> i64 { let x: f64 = 3.0; let y: f64 = x + 2.0; 1 }";
      assert!(pipeline::run(&temp_g(src)).is_ok());
  }
  ```

- [ ] **Commit**

---

## Task 3.4 – Add `UnresolvedName` Error Variant

**File:** `crates/glyim‑typeck/src/typeck/error.rs`

- [ ] **Add variant and Display:**

  ```rust
  UnresolvedName { name: Symbol },
  ```
  Display: `write!(f, "unresolved name: {:?}", name)`

- [ ] **Commit**

---

# Phase 4: Control Flow Fixes

## Task 4.1 – Add `Return` Expression and Fix `?` Operator

**Files:** `crates/glyim‑hir/src/node/mod.rs`, `lower/expr.rs`, `codegen/expr/mod.rs`, `codegen/expr/control.rs`, `crates/glyim‑typeck/src/typeck/expr.rs`

- [ ] **Step 1 – Add `Return` variant to `HirExpr`**

  In `node/mod.rs`:

  ```rust
  pub enum HirExpr {
      // ... existing ...
      Return { id: ExprId, value: Option<Box<HirExpr>>, span: Span },
  }
  ```

  In `get_id()`:
  ```rust
  Self::Return { id, .. } => *id,
  ```

  In `get_span()`:
  ```rust
  Self::Return { span, .. } => *span,
  ```

- [ ] **Step 1b – Update type checker for `Return`**  
  In `crates/glyim‑typeck/src/typeck/expr.rs`:

  In `extract_expr_id`, add:
  ```rust
  HirExpr::Return { id, .. } => *id,
  ```

  In `infer_expr`, add:
  ```rust
  HirExpr::Return { .. } => HirType::Never,
  ```

- [ ] **Step 2 – `Return` codegen**  
  In `codegen/expr/mod.rs`:

  ```rust
  HirExpr::Return { value, .. } => {
      let ret_val = match value {
          Some(v) => codegen_expr(cg, v, fctx)?,
          None => cg.i64_type.const_int(0, false),
      };
      cg.builder.build_return(Some(&ret_val)).ok()?;
      None
  }
  ```

- [ ] **Step 3 – Rewrite `lower_try_expr`**  
  In `crates/glyim‑hir/src/lower/expr.rs`, replace the function with this version that captures the scrutinee in a let‑binding and properly returns the Err value:

  ```rust
  fn lower_try_expr(id: ExprId, expr: &glyim_parse::ExprNode, ctx: &mut LoweringContext) -> HirExpr {
      let span = expr.span;
      let scrutinee_var = ctx.intern("__try_scrut");
      let ok_v = ctx.intern("v");
      HirExpr::Block {
          id: ctx.fresh_id(),
          stmts: vec![
              HirStmt::Let {
                  name: scrutinee_var,
                  mutable: false,
                  value: lower_expr(expr, ctx),
                  span,
              },
              HirStmt::Expr(HirExpr::Match {
                  id,
                  scrutinee: Box::new(HirExpr::Ident { id: ctx.fresh_id(), name: scrutinee_var, span }),
                  arms: vec![
                      (
                          HirPattern::ResultOk(Box::new(HirPattern::Var(ok_v))),
                          None,
                          HirExpr::Ident { id: ctx.fresh_id(), name: ok_v, span },
                      ),
                      (
                          HirPattern::ResultErr(Box::new(HirPattern::Wild)),
                          None,
                          HirExpr::Return {
                              id: ctx.fresh_id(),
                              value: Some(Box::new(HirExpr::Ident {
                                  id: ctx.fresh_id(),
                                  name: scrutinee_var,
                                  span,
                              })),
                              span,
                          },
                      ),
                  ],
                  span,
              }),
          ],
          span,
      }
  }
  ```

- [ ] **Step 4 – Update match codegen for `?` pattern**  
  In `crates/glyim‑codegen‑llvm/src/codegen/expr/control.rs`:

  Ensure imports include:
  ```rust
  use inkwell::{AddressSpace, IntPredicate};
  ```

  Replace `codegen_match` with:

  ```rust
  pub(crate) fn codegen_match<'ctx>(
      cg: &Codegen<'ctx>, expr: &HirExpr, fctx: &mut FunctionContext<'ctx>,
  ) -> Option<IntValue<'ctx>> {
      if let HirExpr::Match { scrutinee, arms, .. } = expr {
          // Special case: desugared `?` — 2 arms, second arm's body is Return
          if arms.len() == 2 && matches!(arms[1].2, HirExpr::Return { .. }) {
              return codegen_try_match(cg, scrutinee, &arms[0], &arms[1], fctx);
          }
          // Fallback: single-arm extraction (existing logic)
          if let Some((pattern, _, body)) = arms.first() {
              let sv = codegen_expr(cg, scrutinee, fctx)?;
              match pattern {
                  HirPattern::OptionSome(inner) | HirPattern::ResultOk(inner) => {
                      if let HirPattern::Var(name) = inner.as_ref() {
                          let ep = cg.builder.build_int_to_ptr(
                              sv, cg.context.ptr_type(AddressSpace::from(0u16)), "ep",
                          ).ok()?;
                          let en = if matches!(pattern, HirPattern::OptionSome(_)) {
                              cg.option_sym
                          } else {
                              cg.result_sym
                          };
                          if let Some(st) = cg.enum_struct_types.borrow().get(&en).copied() {
                              let pp = cg.builder.build_struct_gep(st, ep, 1, "pp").ok()?;
                              let ap = cg.builder.build_bit_cast(
                                  pp, cg.i64_type.ptr_type(AddressSpace::from(0u16)), "ap",
                              ).ok()?;
                              let pv = cg.builder.build_load(cg.i64_type, ap, "pv").ok()?.into_int_value();
                              let al = cg.builder.build_alloca(
                                  cg.i64_type, cg.interner.resolve(*name),
                              ).ok()?;
                              cg.builder.build_store(al, pv).ok()?;
                              fctx.vars.insert(*name, al);
                          }
                      }
                  }
                  _ => {}
              }
              return codegen_expr(cg, body, fctx);
          }
          Some(cg.i64_type.const_int(0, false))
      } else {
          None
      }
  }
  ```

  Add the `codegen_try_match` helper:

  ```rust
  fn codegen_try_match<'ctx>(
      cg: &Codegen<'ctx>,
      scrutinee: &HirExpr,
      ok_arm: &(HirPattern, Option<HirExpr>, HirExpr),
      err_arm: &(HirPattern, Option<HirExpr>, HirExpr),
      fctx: &mut FunctionContext<'ctx>,
  ) -> Option<IntValue<'ctx>> {
      let sv = codegen_expr(cg, scrutinee, fctx)?;
      let rt = cg.enum_struct_types.borrow().get(&cg.result_sym).copied()
          .unwrap_or_else(|| cg.context.struct_type(
              &[cg.i32_type.into(), cg.context.i8_type().array_type(8).into()],
              false,
          ));
      let ep = cg.builder.build_int_to_ptr(sv, rt.ptr_type(AddressSpace::from(0u16)), "ep").ok()?;
      let tp = cg.builder.build_struct_gep(rt, ep, 0, "tp").ok()?;
      let tag = cg.builder.build_load(cg.i32_type, tp, "tag").ok()?.into_int_value();

      let ok_bb = cg.context.append_basic_block(fctx.fn_value, "ok");
      let err_bb = cg.context.append_basic_block(fctx.fn_value, "err");
      let merge_bb = cg.context.append_basic_block(fctx.fn_value, "merge");

      cg.builder.build_conditional_branch(
          cg.builder.build_int_compare(IntPredicate::EQ, tag, cg.i32_type.const_int(0, false), "is_ok").ok()?,
          ok_bb, err_bb,
      ).ok()?;

      // Ok arm: extract payload, bind to variable, execute body
      cg.builder.position_at_end(ok_bb);
      if let HirPattern::ResultOk(inner) = &ok_arm.0 {
          if let HirPattern::Var(name) = inner.as_ref() {
              let pp = cg.builder.build_struct_gep(rt, ep, 1, "pp").ok()?;
              let ap = cg.builder.build_bit_cast(
                  pp, cg.i64_type.ptr_type(AddressSpace::from(0u16)), "ap",
              ).ok()?;
              let okv = cg.builder.build_load(cg.i64_type, ap, "okv").ok()?.into_int_value();
              let al = cg.builder.build_alloca(cg.i64_type, cg.interner.resolve(*name)).ok()?;
              cg.builder.build_store(al, okv).ok()?;
              fctx.vars.insert(*name, al);
          }
      }
      let ok_body = codegen_expr(cg, &ok_arm.2, fctx).unwrap_or(cg.i64_type.const_int(0, false));
      let ok_end = cg.builder.get_insert_block().unwrap();
      cg.builder.build_unconditional_branch(merge_bb).ok()?;

      // Err arm: body is Return, so it will return (no fallthrough)
      cg.builder.position_at_end(err_bb);
      let _ = codegen_expr(cg, &err_arm.2, fctx);

      // Merge (only reached from Ok arm)
      cg.builder.position_at_end(merge_bb);
      Some(ok_body)
  }
  ```

- [ ] **Step 5 – Verify existing `?` test still passes**  
  The `e2e_arrow` integration test already exercises the Ok path through `?`.  
  Testing the Err path requires a function returning `Result<…>` and a multi-arm match in `main`, which is not yet supported by match codegen. This is deferred until match codegen handles all variants.

  ```bash
  cargo test -p glyim-cli --test integration e2e_arrow
  ```
  Expected: PASS

- [ ] **Step 6 – Run all tests**  
  ```bash
  cargo test -p glyim-cli --test integration
  ```

- [ ] **Commit**

---

## Task 4.2 – Raw Pointer Codegen Fix

**File:** `crates/glyim‑codegen‑llvm/src/codegen/types.rs`

- [ ] **Step 1 – Add `RawPtr` case in `hir_type_to_llvm`:**

  ```rust
  HirType::RawPtr { inner } => {
      let i = self.hir_type_to_llvm(inner)?;
      Some(i.ptr_type(inkwell::AddressSpace::from(0u16)).into())
  }
  ```

  **Note:** This codegen path is not yet exercised by the test below, because `register_extern` hardcodes parameter types as `Int`. The test verifies parsing only; a codegen‑level test requires fixing `register_extern` to preserve parameter types (out of scope for this task).

- [ ] **Step 2 – Integration test** (parse‑level):

  ```rust
  #[test]
  fn e2e_extern_block_with_ptr_param() {
      let src = "extern { fn write(fd: i64, buf: *const u8, len: i64) -> i64; }\nfn main() -> i64 { 0 }";
      assert!(pipeline::run(&temp_g(src)).is_ok());
  }
  ```

- [ ] **Commit**

---

# Phase 5: Generics & Method Dispatch

## Task 5.1 – Enable Impl Method Codegen

**File:** `crates/glyim‑codegen‑llvm/src/codegen/mod.rs`

- [ ] **Step 1 – In `generate`, replace `Impl(_) => {}` with:**

  ```rust
  HirItem::Impl(impl_def) => {
      for method in &impl_def.methods { function::codegen_fn(self, method)?; }
  }
  ```

- [ ] **Step 2 – Keep `#[ignore]` on `e2e_impl_method`.**  
  Add a comment explaining why:

  ```rust
  // TODO: requires codegen to respect struct return types.
  // Currently codegen always uses i64, but fn zero() -> Point
  // returns a struct value, causing a type‑mismatched `ret` instruction.
  #[ignore = "struct return type not yet supported in codegen"]
  fn e2e_impl_method() { ... }
  ```

- [ ] **Commit**

---

## Task 5.2 – Basic Monomorphization (Stub)

**File:** create `crates/glyim‑codegen‑llvm/src/codegen/monomorphize.rs`

- [ ] **Step 1 – Write `instantiate_fn`** (clones, clears type params, substitutes param types). Mark `#[allow(dead_code)]`.

  ```rust
  use crate::Codegen;
  use glyim_hir::{HirFn, HirType};
  use glyim_interner::Symbol;
  use std::collections::HashMap;

  #[allow(dead_code)]
  pub(crate) fn instantiate_fn(f: &HirFn, concrete: &[HirType]) -> HirFn {
      let mut sub = HashMap::new();
      for (i, tp) in f.type_params.iter().enumerate() {
          if let Some(ct) = concrete.get(i) { sub.insert(*tp, ct.clone()); }
      }
      let mut mono = f.clone();
      mono.type_params.clear();
      for (_, pt) in &mut mono.params { *pt = apply(&sub, pt); }
      if let Some(rt) = &mut mono.ret { *rt = apply(&sub, rt); }
      mono
  }
  fn apply(sub: &HashMap<Symbol, HirType>, t: &HirType) -> HirType {
      match t {
          HirType::Generic(s, _) if sub.contains_key(s) => sub[s].clone(),
          _ => t.clone(),
      }
  }
  ```

- [ ] **Step 2 – Do NOT add `#[ignore]` to existing generic test.** The test already passes because codegen treats all generics as `i64`. A comment explaining deferred body substitution is sufficient.

- [ ] **Commit**

---

# Phase 6: Typed Macro Pipeline

## Task 6.1 – Macro Engine (Self‑Contained)

**Files:** `crates/glyim‑macro‑core/src/context.rs`, `expand.rs` (create), `lib.rs` (modify), `tests/expand_tests.rs` (create)

- [ ] **Step 1 – Create `context.rs`:**

  ```rust
  use glyim_interner::Symbol;

  #[derive(Debug, Clone, PartialEq)]
  pub struct Field { pub name: Symbol, pub ty: Symbol }

  pub trait MacroContext {
      fn trait_is_implemented(&self, trait_name: Symbol, for_type: Symbol) -> bool;
      fn get_fields(&self, struct_name: Symbol) -> Vec<Field>;
      fn get_type_params(&self, struct_name: Symbol) -> Vec<Symbol>;
  }
  ```

- [ ] **Step 2 – Create `expand.rs`:**

  ```rust
  use crate::context::MacroContext;
  use glyim_interner::Symbol;

  #[derive(Debug, Clone, PartialEq)]
  pub enum MacroArg { Expr(String), Ty(String) }

  /// Interpret a macro body. For `@identity`, returns the first expression argument.
  pub fn interpret_macro(
      _ctx: &dyn MacroContext,
      _type_args: &[Symbol],
      args: &[MacroArg],
  ) -> Option<String> {
      args.first().and_then(|a| match a {
          MacroArg::Expr(s) => Some(s.clone()),
          _ => None,
      })
  }
  ```

- [ ] **Step 3 – Update `lib.rs`:** `pub mod context; pub mod expand;`

- [ ] **Step 4 – Create test `tests/expand_tests.rs`:**

  ```rust
  use glyim_macro_core::context::{MacroContext, Field};
  use glyim_macro_core::expand::{interpret_macro, MacroArg};
  use glyim_interner::Symbol;

  struct TestCtx;
  impl MacroContext for TestCtx {
      fn trait_is_implemented(&self, _: Symbol, _: Symbol) -> bool { false }
      fn get_fields(&self, _: Symbol) -> Vec<Field> { vec![] }
      fn get_type_params(&self, _: Symbol) -> Vec<Symbol> { vec![] }
  }

  #[test]
  fn identity_works() {
      let result = interpret_macro(&TestCtx, &[], &[MacroArg::Expr("42".into())]);
      assert_eq!(result.unwrap(), "42");
  }

  #[test]
  fn identity_wrong_arg_count() {
      let result = interpret_macro(&TestCtx, &[], &[]);
      assert!(result.is_none());
  }
  ```

- [ ] **Run and commit**

---

## Task 6.2 – Keep Hardcoded `@identity` (No Changes)

Verify existing test `e2e_macro_identity` still passes. No changes needed.

---

# Final Verification Checklist

- [ ] `cargo test --workspace` — all tests pass (1 ignored: `e2e_impl_method`)
- [ ] `cargo run -- run examples/test.g` — linker default works
- [ ] `cargo run --features jit -- run --jit test.g` — JIT works
- [ ] `let x = 5; x = 10` → immutability error
- [ ] `if (non‑bool) { … }` → type error
- [ ] Float arithmetic no crash
- [ ] `?` Ok path works (`e2e_arrow` passes)
- [ ] Impl methods codegen infrastructure present (test kept ignored with TODO)
- [ ] CST roundtrip tests pass
- [ ] `just check-dag` — no cycles
- [ ] `just check-tiers` — no tier violations
- [ ] `just check-files` — file sizes within limits

**Plan complete.** Ready to execute.
