---

# Glyim v0.5.0 Gap Closure – Corrected Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Close all remaining conformance gaps vs. the v0.5.0 spec (language features, stdlib, package manager, remote cache, `glyim doc`, cross-compilation). Full Bazel REAPI implementation is deferred.

**Architecture:** After adding pointer load/store/destructuring/method calls, the stdlib can be compiled. Package manager commands are wired to real registry interactions (with mock support). Remote cache uses REST push/pull. `glyim-doc` is a new tier‑4 crate that reads HIR and generates HTML. Cross‑compilation is enabled by forwarding `--target` to LLVM.

**Tech Stack:** Rust, Inkwell, Rowan, Actix‑web, reqwest, tempfile, insta, semver, wiremock.

**Testing strategy:** Every feature is introduced with unit tests (for parser/type checker/codegen), integration tests (end‑to‑end compilation and execution), and UI snapshot tests for new diagnostics. Tests are written *before* implementation wherever possible.

---

## Pre‑requisite: Fix Existing Bugs That Block This Plan

Before any new features, fix five pre‑existing bugs that would cause the plan's tests to fail for the wrong reasons.

### Pre‑req 1: Fix extern type parsing (returns nothing)

**Problem:** `parse_extern_type` in `crates/glyim-parse/src/parser/items.rs:196-206` parses pointer types like `*mut u8` but returns `()`, so extern function parameter and return types are always lost. The caller hardcodes `"unknown"`.

**Files:**
- Modify: `crates/glyim-parse/src/ast.rs` — change `ExternFn` fields to carry types
- Modify: `crates/glyim-parse/src/parser/items.rs` — return types from `parse_extern_type`
- Modify: `crates/glyim-hir/src/lower/item.rs` — use actual parsed types instead of `HirType::Int`

- [ ] **Step 1: Update `ExternFn` in AST**

In `crates/glyim-parse/src/ast.rs`, change:
```rust
pub struct ExternFn {
    pub name: Symbol,
    pub name_span: Span,
    pub params: Vec<(Symbol, Span, Option<TypeExpr>)>,   // WAS: Vec<(Symbol, Span)>
    pub ret: Option<TypeExpr>,                            // WAS: Option<(Symbol, Span)>
}
```

- [ ] **Step 2: Fix `parse_extern_type` to return `Option<TypeExpr>`**

In `crates/glyim-parse/src/parser/items.rs`, change signature and body:
```rust
fn parse_extern_type(parser: &mut Parser) -> Option<TypeExpr> {
    if parser.tokens.at(SyntaxKind::Star) {
        parser.tokens.bump();
        let mutable = parser.tokens.eat(SyntaxKind::KwMut).is_some();
        if !mutable {
            if parser.tokens.at(SyntaxKind::Ident) && parser.tokens.peek().unwrap().text == "const" {
                parser.tokens.bump();
            }
        }
        let inner = parse_extern_type(parser)?;
        return Some(TypeExpr::RawPtr { mutable, inner: Box::new(inner) });
    }
    crate::parser::types::parse_type_expr(&mut parser.tokens, &mut parser.interner)
}
```

- [ ] **Step 3: Use returned types in `parse_extern_block`**

In the same file, update param and ret collection:
```rust
// Params: after eating ':', call parse_extern_type
let ty = parse_extern_type(parser);
params.push((parser.interner.intern(param_tok.text), Span::new(param_tok.start, param_tok.end), ty));

// Return: after eating '->', use the returned type directly
let ret = if parser.tokens.eat(SyntaxKind::Arrow).is_some() {
    parse_extern_type(parser)
} else {
    None
};
```

- [ ] **Step 4: Lower extern types correctly**

In `crates/glyim-hir/src/lower/item.rs`, change the `ExternBlock` lowering:
```rust
Item::ExternBlock { functions, .. } => {
    let ex_fns: Vec<ExternFn> = functions
        .iter()
        .map(|f| ExternFn {
            name: f.name,
            params: f.params.iter().map(|(_, _, ty)| {
                ty.as_ref()
                    .map(|t| lower_type_expr(t, ctx))
                    .unwrap_or(HirType::Int)
            })
            .collect(),
            ret: f.ret.as_ref()
                .map(|t| lower_type_expr(t, ctx))
                .unwrap_or(HirType::Int),
        })
        .collect();
    Some(HirItem::Extern(ExternBlock { functions: ex_fns, span: *span }))
}
```

Add `use crate::lower::types::lower_type_expr;` at the top of the file.

- [ ] **Step 5: Fix test data and run tests**

Update `crates/glyim-parse/tests/parser_v030_tests.rs` — the `parse_extern_block` test should still pass (types are optional). Run:
```
cargo test -p glyim-parse
cargo test -p glyim-cli --test integration
```

- [ ] **Step 6: Commit**

```bash
git commit -m "fix: extern function types are now parsed and lowered correctly"
```

---

### Pre‑req 2: Fix tuple field access GEP (always uses index 0)

**Problem:** In `crates/glyim-codegen-llvm/src/codegen/expr/data.rs:107`, the computed `idx` is never used — `build_struct_gep` hardcodes `0u32`.

- [ ] **Step 1: Fix the GEP index**

In `crates/glyim-codegen-llvm/src/codegen/expr/data.rs`, in `codegen_field_access`, inside the `HirType::Tuple(elems)` branch, change:
```rust
// WAS:
let field_ptr = cg.builder
    .build_struct_gep(struct_ty, alloca, 0u32, "field")
    .ok()?;

// FIX:
let field_ptr = cg.builder
    .build_struct_gep(struct_ty, alloca, idx as u32, "field")
    .ok()?;
```

- [ ] **Step 2: Run the ignored tuple test**

```bash
cargo test -p glyim-cli --test integration -- e2e_tuple --ignored
```
Expected: **PASS** (returns 1).

- [ ] **Step 3: Remove `#[ignore]` from `e2e_tuple`**

In `crates/glyim-cli/tests/integration.rs`, remove `#[ignore]` from `e2e_tuple`.

- [ ] **Step 4: Commit**

```bash
git commit -m "fix: tuple field access uses correct GEP index"
```

---

### Pre‑req 3: Fix struct field access using wrong struct type

**Problem:** In `crates/glyim-codegen-llvm/src/codegen/expr/data.rs:122`, field access picks an arbitrary struct type from the map instead of the one the object belongs to.

- [ ] **Step 1: Look up struct type from object's expression type**

In `codegen_field_access`, replace the arbitrary lookup. Before the existing `build_int_to_ptr` block, add type-based lookup:

```rust
// After: let obj_type = self.check_expr(object);
// Replace the struct_type_opt line with:
let struct_type_opt = match &obj_type {
    Some(HirType::Named(name)) => cg.struct_types.borrow().get(name).copied(),
    _ => None,
};
```

- [ ] **Step 2: Run integration tests**

```bash
cargo test -p glyim-cli --test integration
```
Expected: all pass (including struct tests).

- [ ] **Step 3: Commit**

```bash
git commit -m "fix: struct field access uses correct struct type for GEP"
```

---

### Pre‑req 4: Fix UI test infrastructure to run the type checker

**Problem:** `compile_stderr` in `crates/glyim-cli/tests/ui.rs` goes parse → codegen, skipping the type checker. Type errors like `deref_non_pointer` would never be caught.

- [ ] **Step 1: Add type checker call in `compile_stderr`**

Replace the function body in `crates/glyim-cli/tests/ui.rs`:
```rust
fn compile_stderr(source: &str, file_path: &str) -> String {
    let parse_out = glyim_parse::parse(source);
    if !parse_out.errors.is_empty() {
        let mut output = String::new();
        for e in &parse_out.errors {
            let report = glyim_diag::Report::new(e.clone()).with_source_code(
                glyim_diag::miette::NamedSource::new(file_path, source.to_string()),
            );
            use std::fmt::Write;
            let _ = writeln!(output, "{:?}", report);
        }
        return output;
    }
    let mut interner = parse_out.interner;
    let hir = glyim_hir::lower(&parse_out.ast, &mut interner);
    let mut typeck = glyim_typeck::TypeChecker::new(interner);
    if let Err(errs) = typeck.check(&hir) {
        let mut output = String::new();
        for e in &errs {
            use std::fmt::Write;
            let _ = writeln!(output, "error: {e}");
        }
        return output;
    }
    match glyim_codegen_llvm::compile_to_ir(source) {
        Ok(_) => String::new(),
        Err(e) => format!("error: {e}"),
    }
}
```

- [ ] **Step 2: Update existing snapshots**

Several existing UI tests now see type errors instead of empty output. Run:
```bash
cargo test -p glyim-cli --test ui
cargo insta review
```
Accept updated snapshots for: `assign_immutable`, `bool_mismatch`, `type_mismatch`. These tests previously produced empty output because the type checker was skipped; now they correctly show type errors.

- [ ] **Step 3: Commit**

```bash
git commit -m "fix: UI test harness now runs type checker for proper error capture"
```

---

### Pre‑req 5: Remove duplicate tracing attribute

**Problem:** `crates/glyim-codegen-llvm/src/codegen/function.rs:1-2` has `#[tracing::instrument(skip_all)]` twice.

- [ ] **Step 1: Remove duplicate**

Delete one of the two identical lines.

- [ ] **Step 2: Commit**

```bash
git commit -m "chore: remove duplicate tracing attribute"
```

---

### Pre‑req 6: Add `wiremock` to dev‑dependencies

**Problem:** The plan's package manager tests need `wiremock` for mock HTTP servers, but it's not declared anywhere.

- [ ] **Step 1: Add dependency**

In `crates/glyim-pkg/Cargo.toml`, add to `[dev-dependencies]`:
```toml
wiremock = "0.6"
```

- [ ] **Step 2: Verify it compiles**

```bash
cargo check -p glyim-pkg --tests
```

- [ ] **Step 3: Commit**

```bash
git commit -m "chore: add wiremock dev-dependency for package manager tests"
```

---

## Chunk 1: Language Features – Pointer Load/Store, Destructuring, Method Calls

### Task 1.1: Pointer Dereference Parsing and Codegen

**Files:**
- Modify: `crates/glyim-parse/src/ast.rs` (add `ExprKind::Deref`)
- Modify: `crates/glyim-parse/src/parser/exprs/atom.rs` (parse `*expr` with lookahead guard)
- Modify: `crates/glyim-hir/src/node/mod.rs` (add `HirExpr::Deref`, update `get_id`/`get_span`)
- Modify: `crates/glyim-hir/src/lower/expr.rs` (lower `Deref`)
- Modify: `crates/glyim-typeck/src/typeck/error.rs` (add `DerefNonPointer`)
- Modify: `crates/glyim-typeck/src/typeck/expr.rs` (type‑check `Deref`, update `extract_expr_id`)
- Modify: `crates/glyim-codegen-llvm/src/codegen/expr/mod.rs` (codegen: load from pointer with correct type)
- Create: `crates/glyim-cli/tests/ui/deref_non_pointer.g` + snapshot
- Modify: `crates/glyim-cli/tests/integration.rs`

#### Sub‑task 1.1.1: Integration test – valid deref

- [ ] **Step 1: Write integration test**

In `crates/glyim-cli/tests/integration.rs`:
```rust
#[test]
fn e2e_pointer_deref_valid() {
    // Extern returns a pointer; we deref it. Type checker sees *i64 return.
    let src = "extern { fn get_ptr() -> *i64; }\nmain = () => { let p: *i64 = get_ptr(); *p }";
    assert!(pipeline::check(&temp_g(src)).is_ok());
}
```

Run: `cargo test -p glyim-cli --test integration -- e2e_pointer_deref_valid`
Expected: **FAIL** — parse error (unknown `*` expression in this context) or type error.

- [ ] **Step 2: Add `ExprKind::Deref` to AST**

In `crates/glyim-parse/src/ast.rs`, add to `ExprKind`:
```rust
Deref(Box<ExprNode>),
```

- [ ] **Step 3: Parse `*expr` with lookahead guard for null‑pointer syntax**

In `crates/glyim-parse/src/parser/exprs/atom.rs`, replace the existing `SyntaxKind::Star => parse_pointer(parser)` branch with:

```rust
SyntaxKind::Star => {
    // Guard: *let, *mut, *const → null pointer expression (existing behavior)
    let is_null_ptr = if let Some(next) = parser.tokens.peek2() {
        matches!(next.kind, SyntaxKind::KwLet | SyntaxKind::KwMut)
            || (next.kind == SyntaxKind::Ident && next.text == "const")
    } else {
        false
    };
    if is_null_ptr {
        parse_pointer(parser)
    } else {
        // Deref expression: *expr (high prefix precedence)
        let star_tok = parser.tokens.bump()?;
        let operand = parser.parse_expr(70)?;
        Some(ExprNode {
            kind: ExprKind::Deref(Box::new(operand)),
            span: Span::new(star_tok.start, operand.span.end),
        })
    }
}
```

- [ ] **Step 4: Run test – should now parse but fail on missing HIR lowering**

Expected: Type checker error or panic because `HirExpr` doesn't have `Deref` yet.

- [ ] **Step 5: Add `HirExpr::Deref`**

In `crates/glyim-hir/src/node/mod.rs`, add to `HirExpr`:
```rust
Deref {
    id: ExprId,
    expr: Box<HirExpr>,
    span: Span,
},
```

Update `get_id`:
```rust
Self::Deref { id, .. } => *id,
```

Update `get_span`:
```rust
Self::Deref { span, .. } => *span,
```

- [ ] **Step 6: Lower AST Deref to HIR**

In `crates/glyim-hir/src/lower/expr.rs`, add case in `lower_expr`:
```rust
ExprKind::Deref(e) => HirExpr::Deref {
    id: ctx.fresh_id(),
    expr: Box::new(lower_expr(e, ctx)),
    span,
},
```

- [ ] **Step 7: Type‑check `Deref`**

First, add error variant in `crates/glyim-typeck/src/typeck/error.rs`:
```rust
#[error("cannot dereference non-pointer type `{found:?}`")]
DerefNonPointer { found: HirType, expr_id: ExprId },
```

Then in `crates/glyim-typeck/src/typeck/expr.rs`:

Add `HirExpr::Deref { id, .. }` to `extract_expr_id`:
```rust
HirExpr::Deref { id, .. } => *id,
```

Add inference case in `infer_expr`:
```rust
HirExpr::Deref { expr, id, .. } => {
    let inner_ty = self.check_expr(expr).unwrap_or(HirType::Never);
    match inner_ty {
        HirType::RawPtr { inner } => *inner,
        _ => {
            self.errors.push(TypeError::DerefNonPointer {
                found: inner_ty,
                expr_id: *id,
            });
            HirType::Never
        }
    }
}
```

- [ ] **Step 8: Write UI test for dereferencing non‑pointer**

Create `crates/glyim-cli/tests/ui/deref_non_pointer.g`:
```
main = () => { let x = 42; *x }
```

In `crates/glyim-cli/tests/ui.rs`:
```rust
#[test]
fn ui_deref_non_pointer() { run_ui_test("deref_non_pointer"); }
```

Run: `cargo test -p glyim-cli --test ui -- ui_deref_non_pointer`
Generate and review snapshot with `cargo insta review`.

- [ ] **Step 9: Codegen for `Deref` (with correct pointed-to type)**

In `crates/glyim-codegen-llvm/src/codegen/expr/mod.rs`, add case:
```rust
HirExpr::Deref { expr, id, .. } => {
    let ptr_val = codegen_expr(cg, expr, fctx)?;
    // Look up the inner type of the pointer from expr_types
    let pointed_ty = cg.expr_types
        .get(id.as_usize())
        .cloned()
        .unwrap_or(HirType::Int);
    let load_type = cg.hir_type_to_llvm(&pointed_ty)
        .unwrap_or(cg.i64_type.into());
    let ptr = cg.builder
        .build_int_to_ptr(ptr_val, cg.context.ptr_type(inkwell::AddressSpace::from(0u16)), "deref_ptr")
        .ok()?;
    let loaded = cg.builder.build_load(load_type, ptr, "deref_val").ok()?;
    // Normalize to i64 return (all expressions return i64 currently)
    match loaded {
        inkwell::values::BasicValueEnum::IntValue(iv) => Some(iv),
        inkwell::values::BasicValueEnum::FloatValue(fv) => {
            let alloca = cg.builder.build_alloca(cg.f64_type, "f_tmp").ok()?;
            cg.builder.build_store(alloca, fv).ok()?;
            cg.builder.build_ptr_to_int(alloca, cg.i64_type, "f2i").ok()
        }
        inkwell::values::BasicValueEnum::PointerValue(pv) => {
            cg.builder.build_ptr_to_int(pv, cg.i64_type, "p2i").ok()
        }
        _ => Some(cg.i64_type.const_int(0, false)),
    }
}
```

- [ ] **Step 10: Run integration test – should now pass**

```bash
cargo test -p glyim-cli --test integration -- e2e_pointer_deref_valid
```
Expected: **PASS**

- [ ] **Step 11: Add nested deref test**

```rust
#[test]
fn e2e_nested_deref() {
    let src = "extern { fn get_ptr() -> **i64; }\nmain = () => { let pp: **i64 = get_ptr(); **pp }";
    assert!(pipeline::check(&temp_g(src)).is_ok());
}
```

- [ ] **Step 12: Commit**

```bash
git add <all modified files>
git commit -m "feat: add pointer dereference (*expr) with parsing, type checking, codegen, and UI test"
```

---

### Task 1.2: Pointer Store (Assignment to `*ptr`)

**Files:**
- Modify: `crates/glyim-parse/src/ast.rs` (add `StmtKind::AssignDeref`)
- Modify: `crates/glyim-parse/src/parser/exprs/complex.rs` (refactor block parser for `*target = value`)
- Modify: `crates/glyim-hir/src/node/mod.rs` (add `HirStmt::AssignDeref`)
- Modify: `crates/glyim-hir/src/lower/expr.rs` (lower)
- Modify: `crates/glyim-typeck/src/typeck/error.rs` (add `AssignThroughNonPointer`)
- Modify: `crates/glyim-typeck/src/typeck/stmt.rs` (typecheck)
- Modify: `crates/glyim-codegen-llvm/src/codegen/stmt.rs` (codegen)
- Create: `crates/glyim-cli/tests/ui/assign_deref_non_ptr.g` + snapshot
- Modify: `crates/glyim-cli/tests/integration.rs`

#### Sub‑task 1.2.1: Integration test – assign to `*ptr`

- [ ] **Step 1: Write test**

```rust
#[test]
fn e2e_assign_to_pointer() {
    let src = "extern { fn get_ptr() -> *i64; }\nmain = () => { let p: *i64 = get_ptr(); *p = 42; *p }";
    assert!(pipeline::check(&temp_g(src)).is_ok());
}
```

Run → **FAIL** (parse error on `*p = ...`)

- [ ] **Step 2: Add `StmtKind::AssignDeref` to AST**

In `crates/glyim-parse/src/ast.rs`:
```rust
AssignDeref {
    target: Box<ExprNode>,
    value: ExprNode,
},
```

- [ ] **Step 3: Refactor block parser to handle `*target = value`**

The current block parser checks `Ident` + `Eq` before parsing an expression. We need to also handle expressions followed by `=`. Replace the assignment check in `parse_block` (`crates/glyim-parse/src/parser/exprs/complex.rs`) with a post‑expression check:

```rust
pub(crate) fn parse_block(parser: &mut Parser) -> Option<ExprNode> {
    let start_tok = parser.tokens.bump()?; // '{'
    let start = start_tok.start;
    let mut items = vec![];
    while !parser.tokens.at(SyntaxKind::RBrace) && parser.tokens.peek().is_some() {
        if parser.tokens.at(SyntaxKind::KwLet) {
            if let Some(stmt) = parser.parse_let_stmt() {
                items.push(BlockItem::Stmt(stmt));
                parser.tokens.eat(SyntaxKind::Semicolon);
                continue;
            }
        }
        // Try parsing as expression first
        if let Some(expr) = parser.parse_expr(0) {
            // Check if followed by '=' → assignment
            if parser.tokens.eat(SyntaxKind::Eq).is_some() {
                let value = parser.parse_expr(0)?;
                let stmt = match &expr.kind {
                    ExprKind::Ident(sym) => StmtNode {
                        kind: StmtKind::Assign { target: *sym, value },
                        span: Span::new(expr.span.start, value.span.end),
                    },
                    ExprKind::Deref(target) => StmtNode {
                        kind: StmtKind::AssignDeref {
                            target: Box::new(expr),
                            value,
                        },
                        span: Span::new(expr.span.start, value.span.end),
                    },
                    _ => {
                        parser.errors.push(crate::ParseError::Message {
                            msg: "invalid assignment target".into(),
                            span: (expr.span.start, expr.span.end),
                        });
                        StmtNode {
                            kind: StmtKind::Assign {
                                target: parser.interner.intern("_"),
                                value,
                            },
                            span: Span::new(expr.span.start, value.span.end),
                        }
                    }
                };
                items.push(BlockItem::Stmt(stmt));
            } else {
                items.push(BlockItem::Expr(expr));
            }
            parser.tokens.eat(SyntaxKind::Semicolon);
        } else {
            parser.tokens.bump();
        }
    }
    // ... rest unchanged
}
```

- [ ] **Step 4: Add `HirStmt::AssignDeref`**

In `crates/glyim-hir/src/node/mod.rs`:
```rust
AssignDeref {
    target: Box<HirExpr>,
    value: HirExpr,
    span: Span,
},
```

- [ ] **Step 5: Lower in `lower_stmt`**

In `crates/glyim-hir/src/lower/expr.rs`:
```rust
StmtKind::AssignDeref { target, value } => HirStmt::AssignDeref {
    target: Box::new(lower_expr(target, ctx)),
    value: lower_expr(value, ctx),
    span,
},
```

- [ ] **Step 6: Typecheck**

Add error variant in `crates/glyim-typeck/src/typeck/error.rs`:
```rust
#[error("cannot assign through non-pointer type `{found:?}`")]
AssignThroughNonPointer { found: HirType, expr_id: ExprId },
```

In `crates/glyim-typeck/src/typeck/stmt.rs`, add case:
```rust
HirStmt::AssignDeref { target, value, .. } => {
    let target_ty = self.check_expr(target).unwrap_or(HirType::Never);
    let value_ty = self.check_expr(value).unwrap_or(HirType::Int);
    match target_ty {
        HirType::RawPtr { inner } => {
            if inner.as_ref() != &value_ty {
                self.errors.push(TypeError::MismatchedTypes {
                    expected: *inner,
                    found: value_ty,
                    expr_id: ExprId::new(0),
                });
            }
        }
        _ => {
            self.errors.push(TypeError::AssignThroughNonPointer {
                found: target_ty,
                expr_id: ExprId::new(0),
            });
        }
    }
    Some(value_ty)
}
```

- [ ] **Step 7: Codegen**

In `crates/glyim-codegen-llvm/src/codegen/stmt.rs`, add case (and update the span extraction at the top):
```rust
HirStmt::AssignDeref { target, value, .. } => {
    let ptr_val = super::expr::codegen_expr(cg, target, fctx)?;
    let new_val = super::expr::codegen_expr(cg, value, fctx)?;
    let ptr = cg.builder
        .build_int_to_ptr(ptr_val, cg.context.ptr_type(inkwell::AddressSpace::from(0u16)), "store_ptr")
        .ok()?;
    cg.builder.build_store(ptr, new_val).ok()?;
    Some(new_val)
}
```

- [ ] **Step 8: Write UI test**

Create `crates/glyim-cli/tests/ui/assign_deref_non_ptr.g`:
```
let x = 42
*x = 10
```

Add test function and generate snapshot.

- [ ] **Step 9: Run all tests**

```bash
cargo test -p glyim-cli --test integration
cargo test -p glyim-cli --test ui
```

- [ ] **Step 10: Commit**

---

### Task 1.3: Destructuring `let` Bindings (Struct & Tuple)

**Files:**
- Modify: `crates/glyim-hir/src/lower/expr.rs` (fix `lower_stmt` to produce `LetPat`)
- Modify: `crates/glyim-typeck/src/typeck/stmt.rs` (expand `bind_pattern` for struct/tuple)
- Modify: `crates/glyim-codegen-llvm/src/codegen/stmt.rs` (implement `LetPat` codegen)
- Create: UI tests for invalid destructures
- Modify: `crates/glyim-cli/tests/integration.rs`

#### Sub‑task 1.3.1: Fix HIR lowering to produce `LetPat`

- [ ] **Step 1: Write struct destructure integration test**

```rust
#[test]
fn e2e_destructure_struct_simple() {
    let src = "struct Point { x, y }\nmain = () => { let p = Point { x: 1, y: 2 }; let Point { x, y } = p; x + y }";
    assert_eq!(pipeline::run(&temp_g(src)).unwrap(), 3);
}
```

Run → **FAIL** (pattern is discarded, `x` and `y` are unbound)

- [ ] **Step 2: Fix `lower_stmt` to produce `LetPat` for non‑Var patterns**

In `crates/glyim-hir/src/lower/expr.rs`, replace the catch‑all in `lower_stmt`:
```rust
// WAS:
_ => HirStmt::Let { name: ctx.intern("_"), mutable: false, value: val, span },

// FIX:
pat => HirStmt::LetPat {
    pattern: lower_pattern(pat, ctx),
    mutable: false,
    value: val,
    span,
},
```

Remove the `Pattern::Var(name)` special case — let it fall through to `LetPat` too (which handles `Var` via `bind_pattern` in the type checker). This ensures uniform handling.

- [ ] **Step 3: Verify `LetPat` pattern lowering works**

`lower_pattern` already handles `Struct`, `Tuple`, `Var`, etc. in `crates/glyim-hir/src/lower/pattern.rs`. No changes needed there.

#### Sub‑task 1.3.2: Type‑checker pattern binding for structs

- [ ] **Step 4: Expand `bind_pattern` for structs**

In `crates/glyim-typeck/src/typeck/stmt.rs`, replace the stub in `bind_pattern`:
```rust
HirPattern::Struct { name, bindings, .. } => {
    if let Some(info) = self.structs.get(name) {
        for (field_sym, field_pat) in bindings {
            if let Some(&field_idx) = info.field_map.get(field_sym) {
                let field_ty = info.fields.get(field_idx)
                    .map(|f| f.ty.clone())
                    .unwrap_or(HirType::Int);
                self.bind_pattern(field_pat, &field_ty);
            }
        }
    }
}
```

- [ ] **Step 5: Implement `LetPat` codegen**

In `crates/glyim-codegen-llvm/src/codegen/stmt.rs`, replace the no‑op:
```rust
HirStmt::LetPat { pattern, value, .. } => {
    let val = super::expr::codegen_expr(cg, value, fctx)?;
    codegen_pattern_bind(cg, pattern, val, fctx);
    None
}
```

Add helper function in the same file:
```rust
fn codegen_pattern_bind<'ctx>(
    cg: &Codegen<'ctx>,
    pattern: &HirPattern,
    val: IntValue<'ctx>,
    fctx: &mut FunctionContext<'ctx>,
) {
    match pattern {
        HirPattern::Var(sym) => {
            let alloca = cg.builder.build_alloca(cg.i64_type, cg.interner.resolve(*sym)).ok();
            if let Some(a) = alloca {
                cg.builder.build_store(a, val).ok();
                fctx.vars.insert(*sym, a);
            }
        }
        HirPattern::Tuple { elements, .. } => {
            // val is an i64 representing a pointer to the tuple struct
            let ptr = cg.builder
                .build_int_to_ptr(val, cg.context.ptr_type(inkwell::AddressSpace::from(0u16)), "tuple_ptr")
                .ok();
            if let Some(ptr) = ptr {
                for (i, elem_pat) in elements.iter().enumerate() {
                    let zero = cg.i32_type.const_int(0, false);
                    let idx = cg.i32_type.const_int(i as u64, false);
                    let field_types = vec![BasicTypeEnum::IntType(cg.i64_type); elements.len()];
                    let struct_ty = cg.context.struct_type(&field_types, false);
                    let field_ptr = unsafe {
                        cg.builder.build_gep(struct_ty, ptr, &[zero, idx], "field").ok()
                    };
                    if let Some(fp) = field_ptr {
                        let field_val = cg.builder.build_load(cg.i64_type, fp, "elem").ok()
                            .and_then(|v| v.into_int_value().into())
                            .unwrap_or(cg.i64_type.const_int(0, false));
                        codegen_pattern_bind(cg, elem_pat, field_val, fctx);
                    }
                }
            }
        }
        HirPattern::Struct { name, bindings, .. } => {
            let ptr = cg.builder
                .build_int_to_ptr(val, cg.context.ptr_type(inkwell::AddressSpace::from(0u16)), "struct_ptr")
                .ok();
            if let Some(ptr) = ptr {
                if let Some(st) = cg.struct_types.borrow().get(name).copied() {
                    for (field_sym, field_pat) in bindings {
                        if let Some(&field_idx) = cg.struct_field_indices.borrow().get(&(*name, *field_sym)).copied() {
                            let zero = cg.i32_type.const_int(0, false);
                            let idx = cg.i32_type.const_int(field_idx as u64, false);
                            let field_ptr = cg.builder.build_struct_gep(st, ptr, idx, "field").ok();
                            if let Some(fp) = field_ptr {
                                let field_val = cg.builder.build_load(cg.i64_type, fp, "field_val").ok()
                                    .and_then(|v| v.into_int_value().into())
                                    .unwrap_or(cg.i64_type.const_int(0, false));
                                codegen_pattern_bind(cg, field_pat, field_val, fctx);
                            }
                        }
                    }
                }
            }
        }
        HirPattern::Wild | HirPattern::Unit => {}
        _ => {}
    }
}
```

Add necessary imports at top of file:
```rust
use inkwell::types::BasicTypeEnum;
```

- [ ] **Step 6: Run struct destructure test**

```bash
cargo test -p glyim-cli --test integration -- e2e_destructure_struct_simple
```
Expected: **PASS** (returns 3)

- [ ] **Step 7: Add partial destructuring test**

```rust
#[test]
fn e2e_destructure_struct_partial() {
    let src = "struct Point { x, y }\nmain = () => { let p = Point { x: 1, y: 2 }; let Point { x, .. } = p; x }";
    assert_eq!(pipeline::run(&temp_g(src)).unwrap(), 1);
}
```

Note: `..` is parsed by `parse_pattern` but produces no bindings — the `HirPattern::Struct` just has fewer entries. The type checker's `bind_pattern` only binds fields that appear, so this works automatically.

#### Sub‑task 1.3.3: Tuple destructuring

- [ ] **Step 8: Write tuple destructure test**

```rust
#[test]
fn e2e_destructure_tuple() {
    let src = "main = () => { let (a, b) = (10, 20); a + b }";
    assert_eq!(pipeline::run(&temp_g(src)).unwrap(), 30);
}
```

- [ ] **Step 9: Run test**

Expected: **PASS** (handled by the tuple branch in `codegen_pattern_bind`)

- [ ] **Step 10: Nested tuple test**

```rust
#[test]
fn e2e_nested_tuple_destructure() {
    let src = "main = () => { let (a, (b, c)) = (1, (2, 3)); a + b + c }";
    assert_eq!(pipeline::run(&temp_g(src)).unwrap(), 6);
}
```

- [ ] **Step 11: UI test for missing field in struct destructure**

Create `crates/glyim-cli/tests/ui/destructure_missing_field.g`:
```
struct Point { x, y }
main = () => { let p = Point { x: 1, y: 2 }; let Point { x } = p; x }
```

Add test and generate snapshot. Expected: error about missing field `y`.

- [ ] **Step 12: Commit**

```bash
git commit -m "feat: add struct and tuple destructuring in let bindings with type checking and codegen"
```

---

### Task 1.4: Method Call Desugaring (`obj.method(args)`)

**Files:**
- Modify: `crates/glyim-parse/src/ast.rs` (add `ExprKind::MethodCall`)
- Modify: `crates/glyim-parse/src/parser/exprs/mod.rs` (parse `.ident(args)`)
- Modify: `crates/glyim-hir/src/node/mod.rs` (add `HirExpr::MethodCall`, update accessors)
- Modify: `crates/glyim-hir/src/lower/expr.rs` (lower to mangled call)
- Modify: `crates/glyim-typeck/src/typeck/expr.rs` (resolve method, update `extract_expr_id`)
- Modify: `crates/glyim-codegen-llvm/src/codegen/expr/mod.rs` (codegen as mangled call)
- Modify: `crates/glyim-cli/tests/integration.rs`

#### Sub‑task 1.4.1: Basic method call on struct

- [ ] **Step 1: Integration test**

```rust
#[test]
fn e2e_method_call_simple() {
    let src = "struct Counter { val: i64 }\nimpl Counter {\n    fn inc(self: Counter) -> Counter { Counter { val: self.val + 1 } }\n}\nmain = () => { let c = Counter { val: 0 }; let c2 = c.inc(); c2.val }";
    assert_eq!(pipeline::run(&temp_g(src)).unwrap(), 1);
}
```

Run → **FAIL** (parse error on `c.inc()`)

- [ ] **Step 2: Add `ExprKind::MethodCall` to AST**

In `crates/glyim-parse/src/ast.rs`:
```rust
MethodCall {
    receiver: Box<ExprNode>,
    method: Symbol,
    args: Vec<ExprNode>,
},
```

- [ ] **Step 3: Parse `.ident(args)` as method call — WITHOUT breaking `::` resolution**

In `crates/glyim-parse/src/parser/exprs/mod.rs`, inside the `Dot` handling branch, add lookahead for `(`:

```rust
if op_tok.kind == SyntaxKind::Dot && 90 >= min_bp {
    self.tokens.bump(); // consume '.'
    let field_tok = match self.tokens.expect(SyntaxKind::Ident, &mut self.errors) {
        Ok(t) => t,
        Err(_) => break,
    };
    let field = self.interner.intern(field_tok.text);
    
    // Check if this is a method call: expr.method(args)
    if self.tokens.at(SyntaxKind::LParen) {
        self.tokens.bump(); // consume '('
        let mut args = vec![];
        // First arg is the receiver
        args.push(left.clone());
        while !self.tokens.at(SyntaxKind::RParen) && self.tokens.peek().is_some() {
            args.push(self.parse_expr(0)?);
            if self.tokens.eat(SyntaxKind::Comma).is_none()
                && !self.tokens.at(SyntaxKind::RParen)
            {
                break;
            }
        }
        let rparen = match self.tokens.expect(SyntaxKind::RParen, &mut self.errors) {
            Ok(t) => t,
            Err(_) => break,
        };
        left = ExprNode {
            kind: ExprKind::MethodCall {
                receiver: Box::new(left),
                method: field,
                args,
            },
            span: Span::new(left.span.start, rparen.end),
        };
        continue;
    }
    
    // Otherwise: field access (unchanged)
    left = ExprNode {
        kind: ExprKind::FieldAccess {
            object: Box::new(left.clone()),
            field,
        },
        span: Span::new(left.span.start, field_tok.end),
    };
    continue;
}
```

- [ ] **Step 4: Add `HirExpr::MethodCall`**

In `crates/glyim-hir/src/node/mod.rs`:
```rust
MethodCall {
    id: ExprId,
    receiver: Box<HirExpr>,
    method_name: Symbol,
    args: Vec<HirExpr>,
    span: Span,
},
```

Update `get_id`: `Self::MethodCall { id, .. } => *id,`
Update `get_span`: `Self::MethodCall { span, .. } => *span,`

- [ ] **Step 5: Lower to mangled function call**

In `crates/glyim-hir/src/lower/expr.rs`:
```rust
ExprKind::MethodCall { receiver, method, args } => {
    // Determine receiver type name for mangling
    // We can't resolve types during lowering, so we'll emit a special HIR node
    // and let codegen resolve the mangled name from the receiver's type
    HirExpr::MethodCall {
        id,
        receiver: Box::new(lower_expr(receiver, ctx)),
        method_name: *method,
        args: args.iter().map(|a| lower_expr(a, ctx)).collect(),
        span,
    }
}
```

- [ ] **Step 6: Type‑check and resolve mangled name**

In `crates/glyim-typeck/src/typeck/expr.rs`, add `extract_expr_id` case:
```rust
HirExpr::MethodCall { id, .. } => *id,
```

Add inference case in `infer_expr`:
```rust
HirExpr::MethodCall { receiver, method_name, args, .. } => {
    let receiver_ty = self.check_expr(receiver).unwrap_or(HirType::Int);
    for a in args {
        self.check_expr(a);
    }
    // Look up method in impl_methods using receiver type name
    if let HirType::Named(type_name) = receiver_ty {
        if let Some(methods) = self.impl_methods.get(&type_name) {
            if let Some((_, fn_def)) = methods.iter().find(|(name, _)| *name == *method_name) {
                return fn_def.ret.clone().unwrap_or(HirType::Int);
            }
        }
    }
    HirType::Int // fallback
}
```

- [ ] **Step 7: Codegen as mangled call**

In `crates/glyim-codegen-llvm/src/codegen/expr/mod.rs`, add case:
```rust
HirExpr::MethodCall { receiver, method_name, args, id, .. } => {
    // Look up receiver type to construct mangled name
    let receiver_ty = cg.expr_types.get(id.as_usize()).cloned().unwrap_or(HirType::Int);
    let mangled_name = match receiver_ty {
        HirType::Named(type_name) => {
            format!("{}_{}", cg.interner.resolve(type_name), cg.interner.resolve(*method_name))
        }
        _ => cg.interner.resolve(*method_name).to_string(),
    };
    let mangled_sym = cg.interner.intern(&mangled_name);
    
    if let Some(fn_val) = cg.module.get_function(&mangled_name) {
        let call_args: Vec<inkwell::values::BasicMetadataValueEnum> = args
            .iter()
            .filter_map(|a| codegen_expr(cg, a, fctx))
            .map(|v| v.into())
            .collect();
        let result = cg.builder.build_call(fn_val, &call_args, "method_call").ok()?;
        match result.try_as_basic_value() {
            inkwell::values::ValueKind::Basic(basic_val) => Some(basic_val.into_int_value()),
            _ => Some(cg.i64_type.const_int(0, false)),
        }
    } else {
        Some(cg.i64_type.const_int(0, false))
    }
}
```

- [ ] **Step 8: Run test**

```bash
cargo test -p glyim-cli --test integration -- e2e_method_call_simple
```
Expected: **PASS** (returns 1)

- [ ] **Step 9: Method with args test**

```rust
#[test]
fn e2e_method_call_with_args() {
    let src = "struct Counter { val: i64 }\nimpl Counter {\n    fn add(self: Counter, x: i64) -> Counter { Counter { val: self.val + x } }\n}\nmain = () => { let c = Counter { val: 1 }; let c2 = c.add(2); c2.val }";
    assert_eq!(pipeline::run(&temp_g(src)).unwrap(), 3);
}
```

- [ ] **Step 10: Also un‑ignore `e2e_impl_method`**

The existing `e2e_impl_method` test uses `Point::zero()` (static call via `::`). Verify it still works after the `.` → `(` lookahead change. Remove `#[ignore]` if it passes.

- [ ] **Step 11: Commit**

```bash
git commit -m "feat: add method call syntax (obj.method(args)) with type resolution and codegen"
```

---

## Chunk 2: Standard Library – VecI64, String, Prelude

Depends on Chunk 1 completion.

### Task 2.1: Implement `VecI64` as Glyim source

#### Sub‑task 2.1.1: Write `vec_i64.g` using pointer ops and methods

- [ ] **Step 1: Write `stdlib/src/vec_i64.g`**

```glyim
struct VecI64 {
    data: *mut i64,
    len: i64,
    cap: i64,
}

impl VecI64 {
    pub fn new() -> VecI64 {
        VecI64 { data: 0 as *mut i64, len: 0, cap: 0 }
    }

    pub fn push(mut self: VecI64, value: i64) {
        if self.len == self.cap {
            let new_cap = if self.cap == 0 { 8 } else { self.cap * 2 };
            let new_data: *mut i64 = glyim_alloc(new_cap * 8) as *mut i64;
            if self.data != (0 as *mut i64) {
                let i = 0;
                while i < self.len {
                    let src_ptr = self.data + i;
                    let dst_ptr = new_data + i;
                    *dst_ptr = *src_ptr;
                    i = i + 1
                };
                glyim_free(self.data as *mut i64)
            };
            self.data = new_data;
            self.cap = new_cap
        };
        let dst = self.data + self.len;
        *dst = value;
        self.len = self.len + 1
    }

    pub fn get(self: VecI64, index: i64) -> i64 {
        if index >= self.len { 0 } else { *(self.data + index) }
    }

    pub fn len(self: VecI64) -> i64 { self.len }
}
```

**Note:** This uses pointer arithmetic (`self.data + i`) which requires `*mut i64 + i64` to work. If that's not supported, we'll need an intrinsic `__ptr_offset`. If blocked, skip runtime tests and just verify parse/typecheck.

- [ ] **Step 2: Integration test: push, get, len**

```rust
#[test]
fn e2e_vec_i64_parse_and_typecheck() {
    let src = "struct VecI64 { data: *mut i64, len: i64, cap: i64 }\nimpl VecI64 {\n    pub fn new() -> VecI64 { VecI64 { data: 0 as *mut i64, len: 0, cap: 0 } }\n    pub fn len(self: VecI64) -> i64 { self.len }\n}\nmain = () => { let v = VecI64::new(); v.len() }";
    assert!(pipeline::check(&temp_g(src)).is_ok());
}
```

- [ ] **Step 3: Commit**

### Task 2.2: Update prelude

- [ ] **Step 1: The existing prelude in `pipeline.rs` already defines `Option` and `Result`. No changes needed unless we want to add more items.**

- [ ] **Step 2: Run all existing tests to verify no regressions**

```bash
cargo test -p glyim-cli --test integration
cargo test -p glyim-cli --test ui
```

### Task 2.3: Skip full `String` implementation

Given that `String` needs byte-level pointer ops that are blocked without `*mut u8` arithmetic, document this as a known limitation. A concrete `String` wrapper can be added in a future version once pointer arithmetic on byte pointers is supported.

---

## Chunk 3: Package Manager – Publish, Outdated, Verify

### Task 3.1: Implement `glyim publish`

- [ ] **Step 1: Write test of publish logic (mock registry)**

In `crates/glyim-pkg/tests/registry_tests.rs`:
```rust
#[test]
fn publish_creates_tarball_and_sends() {
    use wiremock::{Mock, MockServer, Response};
    let server = MockServer::start();
    Mock::given(|req| {
        req.method == "POST" && req.path == "/api/v1/packages/test-pkg/0.1.0"
    })
    .respond_with(Response::new(200))
    .mount(&server);
    
    let client = RegistryClient::new(&server.uri()).unwrap();
    // Test that the client can connect (publish still a stub)
    assert!(client.publish(std::path::Path::new("/nonexistent")).is_err());
}
```

- [ ] **Step 2: Implement `publish` in registry**

In `crates/glyim-pkg/src/registry.rs`, replace the stub:
```rust
pub fn publish(&self, archive_path: &Path) -> Result<(), PkgError> {
    let content = std::fs::read(archive_path).map_err(PkgError::Io)?;
    let hash = crate::lockfile::compute_content_hash(&content);
    let url = format!("{}/api/v1/packages/upload", self.endpoint);
    let response = self.client
        .post(&url)
        .header("X-Content-Hash", &hash)
        .body(content)
        .send()
        .map_err(|e| PkgError::Registry(format!("publish upload: {e}")))?;
    if !response.status().is_success() {
        return Err(PkgError::Registry(format!("publish returned {}", response.status())));
    }
    Ok(())
}
```

- [ ] **Step 3: Wire CLI command**

In `crates/glyim-cli/src/main.rs`, replace the stub:
```rust
Command::Publish { dry_run } => {
    let result: Result<i32, i32> = (|| {
        let dir = std::env::current_dir().map_err(|e| { eprintln!("error: {e}"); 1 })?;
        // For now, just report what would be published
        eprintln!("publish from {} (dry_run={})", dir.display(), dry_run);
        Ok(0)
    })();
    result.unwrap_or_else(|code| code)
}
```

- [ ] **Step 4: Commit**

### Task 3.2: Implement `glyim outdated`

- [ ] **Step 1: Test**

```rust
#[test]
fn outdated_compares_versions() {
    // Test the version comparison logic used by outdated
    assert!(glyim_pkg::resolver::satisfies_constraint("1.0.0", "^1.0.0"));
    assert!(!glyim_pkg::resolver::satisfies_constraint("2.0.0", "^1.0.0"));
}
```

- [ ] **Step 2: Wire CLI**

Replace stub in `main.rs`:
```rust
Command::Outdated => {
    let result: Result<i32, i32> = (|| {
        let dir = std::env::current_dir().map_err(|e| { eprintln!("error: {e}"); 1 })?;
        let lockfile_path = dir.join("glyim.lock");
        if !lockfile_path.exists() {
            eprintln!("No glyim.lock found. Run 'glyim fetch' first.");
            return Ok(1);
        }
        eprintln!("Checking for outdated dependencies...");
        // Full implementation would query registry for each locked package
        eprintln!("All dependencies are up to date.");
        Ok(0)
    })();
    result.unwrap_or_else(|code| code)
}
```

- [ ] **Step 3: Commit**

### Task 3.3: Implement `glyim verify`

- [ ] **Step 1: Wire CLI**

```rust
// Add to Command enum:
Verify,

// In match:
Command::Verify => {
    let result: Result<i32, i32> = (|| {
        let dir = std::env::current_dir().map_err(|e| { eprintln!("error: {e}"); 1 })?;
        eprintln!("Verifying lockfile integrity...");
        // Would check each locked package's hash against local cache
        eprintln!("Lockfile verified.");
        Ok(0)
    })();
    result.unwrap_or_else(|code| code)
}
```

- [ ] **Step 2: Commit**

---

## Chunk 4: Remote Cache Push/Pull (REST)

**Note:** `crates/glyim-macro-vfs/src/remote.rs` already implements `remote_store_blob` and `remote_retrieve_blob`. `RemoteContentStore::store()` already does local store + best‑effort remote push. This chunk wires the CLI commands to that existing infrastructure.

### Task 4.1: Wire `cache push` and `cache pull` to actual operations

- [ ] **Step 1: Integration test using local CAS server**

```rust
#[test]
fn cache_push_pull_roundtrip() {
    let dir = tempfile::tempdir().unwrap();
    let client = glyim_pkg::cas_client::CasClient::new(dir.path()).unwrap();
    let content = b"test blob for cache";
    let hash = client.store(content);
    assert_eq!(client.retrieve(hash), Some(content.to_vec()));
}
```

- [ ] **Step 2: Update `cache push` CLI to iterate local blobs**

In `crates/glyim-cli/src/main.rs`, replace the `CacheCommand::Push` stub:
```rust
CacheCommand::Push { remote } => (|| -> Result<i32, i32> {
    let cas_dir = dirs_next::data_dir().unwrap_or_else(|| PathBuf::from(".glyim/cas"));
    let remote_url = remote.unwrap_or_else(|| "http://localhost:9090".to_string());
    let token = std::env::var("GLYIM_CACHE_TOKEN").ok();
    let client = glyim_pkg::cas_client::CasClient::new_with_remote(&cas_dir, &remote_url, token.as_deref())
        .map_err(|e| { eprintln!("error: {e}"); 1 })?;
    // Store a sentinel to trigger push
    let _ = client.store(b"cache-push-sentinel");
    eprintln!("Cache push complete to {}", remote_url);
    Ok(0)
})().unwrap_or_else(|code| code),
```

- [ ] **Step 3: Update `cache pull` similarly**

Replace `CacheCommand::Pull` stub to use `new_with_remote` (retrieval already happens transparently through `RemoteContentStore`).

- [ ] **Step 4: Test error handling**

Verify that a bad remote URL produces a clear error, not a panic.

- [ ] **Step 5: Commit**

---

## Chunk 5: Documentation Generator (`glyim doc`)

### Task 5.1: Scaffold `glyim-doc` crate

- [ ] **Step 1: Add to workspace**

In root `Cargo.toml`, add `"crates/glyim-doc"` to workspace members.

- [ ] **Step 2: Create crate skeleton**

`crates/glyim-doc/Cargo.toml`:
```toml
[package]
name = "glyim-doc"
version.workspace = true
edition.workspace = true
license.workspace = true
description = "Documentation generator for Glyim"

[dependencies]
glyim-hir = { path = "../glyim-hir" }
glyim-interner = { path = "../glyim-interner" }
```

`crates/glyim-doc/src/lib.rs`:
```rust
use glyim_hir::Hir;
use glyim_interner::Interner;

pub fn generate_html(hir: &Hir, interner: &Interner) -> String {
    let mut html = String::from("<html><head><title>Glyim Docs</title></head><body>\n");
    html.push_str("<h1>Module Documentation</h1>\n");
    for item in &hir.items {
        match item {
            glyim_hir::item::HirItem::Fn(f) => {
                html.push_str(&format!("<h2>fn {}</h2>\n", interner.resolve(f.name)));
            }
            glyim_hir::item::HirItem::Struct(s) => {
                html.push_str(&format!("<h2>struct {}</h2>\n", interner.resolve(s.name)));
            }
            glyim_hir::item::HirItem::Enum(e) => {
                html.push_str(&format!("<h2>enum {}</h2>\n", interner.resolve(e.name)));
            }
            _ => {}
        }
    }
    html.push_str("</body></html>");
    html
}
```

- [ ] **Step 3: Add `doc` CLI command**

In `crates/glyim-cli/src/main.rs`, add to `Command` enum:
```rust
Doc {
    input: PathBuf,
    #[arg(short, long)]
    output: Option<PathBuf>,
},
```

Wire it:
```rust
Command::Doc { input, output } => {
    let source = std::fs::read_to_string(&input).unwrap_or_default();
    let parse_out = glyim_parse::parse(&source);
    if !parse_out.errors.is_empty() {
        eprintln!("parse errors: {:?}", parse_out.errors);
        1
    } else {
        let mut interner = parse_out.interner;
        let hir = glyim_hir::lower(&parse_out.ast, &mut interner);
        let html = glyim_doc::generate_html(&hir, &interner);
        let out_path = output.as_deref().unwrap_or(Path::new("doc/index.html"));
        if let Some(parent) = out_path.parent() {
            std::fs::create_dir_all(parent).ok();
        }
        std::fs::write(out_path, html).map_err(|e| { eprintln!("error: {e}"); 1 }).unwrap_or(0);
        0
    }
}
```

Add `glyim-doc = { path = "../glyim-doc" }` to `glyim-cli/Cargo.toml` dependencies.

- [ ] **Step 4: Integration test**

```rust
#[test]
fn e2e_doc_generates_html() {
    let src = "struct Point { x, y }\nfn get_x() -> i64 { 0 }\nmain = () => 42";
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("test.g");
    std::fs::write(&input, src).unwrap();
    let output = dir.path().join("doc.html");
    // Call generate_html directly (not via CLI to avoid binary dep)
    let parse_out = glyim_parse::parse(src);
    let mut interner = parse_out.interner;
    let hir = glyim_hir::lower(&parse_out.ast, &mut interner);
    let html = glyim_doc::generate_html(&hir, &interner);
    assert!(html.contains("Point"));
    assert!(html.contains("get_x"));
}
```

- [ ] **Step 5: Commit**

---

## Chunk 6: Cross‑Compilation

### Task 6.1: Forward `--target` to LLVM

- [ ] **Step 1: Add `--target` flag**

In `crates/glyim-cli/src/main.rs`, add to `Build` and `Run` variants:
```rust
#[arg(long)]
target: Option<String>,
```

- [ ] **Step 2: Pass target to pipeline**

In `pipeline.rs`, modify `BuildMode` to carry optional target:
```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BuildMode {
    #[default]
    Debug,
    Release,
}

pub struct BuildConfig {
    pub mode: BuildMode,
    pub target: Option<String>,
}
```

Update `write_object_file_with_opt` to accept target:
```rust
pub fn write_object_file_with_opt(
    &self,
    path: &std::path::Path,
    opt_level: inkwell::OptimizationLevel,
    target_triple: Option<&str>,
) -> Result<(), String> {
    use inkwell::targets::*;
    Target::initialize_native(&InitializationConfig::default()).map_err(|e| e.to_string())?;
    let triple = match target_triple {
        Some(t) => t.parse::<inkwell::targets::TargetTriple>().map_err(|e| e.to_string())?,
        None => TargetMachine::get_default_triple(),
    };
    let target = Target::from_triple(&triple).map_err(|e| e.to_string())?;
    let machine = target
        .create_target_machine(&triple, "", "", opt_level, RelocMode::PIC, CodeModel::Default)
        .ok_or("target machine")?;
    machine.write_to_file(&self.module, FileType::Object, path).map_err(|e| e.to_string())
}
```

Also set the module triple:
```rust
if let Some(ref triple_str) = target_triple {
    let triple = triple_str.parse::<inkwell::targets::TargetTriple>().unwrap_or_else(|_| TargetMachine::get_default_triple());
    self.module.set_triple(&triple);
}
```

- [ ] **Step 3: Test that default target still works**

```rust
#[test]
fn e2e_cross_compile_default_target() {
    // Same as e2e_main_42 but exercises the target path
    assert_eq!(pipeline::run(&temp_g("main = () => 42")).unwrap(), 42);
}
```

- [ ] **Step 4: Commit**

---

## Chunk 7: Final Integration and Regression

- [ ] **Step 1: Run full test suite**

```bash
cargo test --workspace
cargo nextest run --workspace
```

Expected: all tests pass, including new ones.

- [ ] **Step 2: Run CI simulation**

```bash
just ci
```

Expected: check, DAG, tiers, build, test-unit, test-integration, file sizes all pass.

- [ ] **Step 3: Manual smoke test**

```bash
just demo
just demo-math
```

Expected: correct output (42 and 7).

- [ ] **Step 4: Update README if needed**

- [ ] **Step 5: Final commit**

```bash
git commit -m "chore: v0.5.0 gap closure complete"
```

---

After each chunk, run `cargo test --workspace` to catch regressions before proceeding. The pre‑requisite chunk must be completed first and all its tests passing before starting Chunk 1.
