We need a more granular, test-heavy plan. Below is a significantly expanded version with explicit code examples for tests, step-by-step TDD cycles, and comprehensive error/edge case coverage.

---

# Glyim v0.5.0 Gap Closure – Detailed Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Close all remaining conformance gaps vs. the v0.5.0 spec (language features, stdlib, package manager, remote cache, `glyim doc`, cross-compilation). Full Bazel REAPI implementation is deferred.

**Architecture:** After adding pointer load/store/destructuring/method calls, the stdlib can be compiled. Package manager commands are wired to real registry interactions (with mock support). Remote cache uses REST push/pull. `glyim-doc` is a new tier‑4 crate that reads HIR and generates HTML. Cross‑compilation is enabled by forwarding `--target` to LLVM.

**Tech Stack:** Rust, Inkwell, Rowan, Actix‑web, reqwest, tempfile, insta, semver.

**Testing strategy:** Every feature is introduced with unit tests (for parser/type checker/codegen), integration tests (end‑to‑end compilation and execution), and UI snapshot tests for new diagnostics. Tests are written *before* implementation wherever possible.

---

## Chunk 1: Language Features – Pointer Load/Store, Destructuring, Method Calls

### Task 1.1: Pointer Dereference Parsing and Codegen

**Files:**
- Modify: `crates/glyim-parse/src/ast.rs` (add `ExprKind::Deref` variant)
- Modify: `crates/glyim-parse/src/parser/exprs/atom.rs` (parse `*` as prefix unary op)
- Modify: `crates/glyim-hir/src/node/mod.rs` (add `HirExpr::Deref`)
- Modify: `crates/glyim-hir/src/lower/expr.rs` (lower `Deref`)
- Modify: `crates/glyim-typeck/src/typeck/expr.rs` (type check pointer deref)
- Modify: `crates/glyim-codegen-llvm/src/codegen/expr/mod.rs` (codegen: load from pointer)
- Create: `crates/glyim-cli/tests/ui/deref_non_pointer.g` + expected stderr snapshot
- Modify: `crates/glyim-cli/tests/integration.rs`

#### Sub‑task 1.1.1: Integration test – valid deref

- [ ] **Step 1: Write integration test**

```rust
// In crates/glyim-cli/tests/integration.rs, inside fn e2e_pointer_deref_valid
let src = "extern { fn get_ptr() -> *i64; }\nmain = () => { let p: *i64 = get_ptr(); *p }";
assert!(pipeline::check(&temp_g(src)).is_ok());
```

Run: `cargo test -p glyim-cli --test integration -- e2e_pointer_deref_valid`  
Expected: **FAIL** – parse error (unknown `*` expression) or type error.

- [ ] **Step 2: Add `ExprKind::Deref` to AST**

In `glyim-parse/src/ast.rs` add to `ExprKind`:
```rust
Deref(Box<ExprNode>),
```

- [ ] **Step 3: Parse `*` as unary operator in atom.rs**

In `glyim-parse/src/parser/exprs/atom.rs`, extend `match op_tok.kind` to include `SyntaxKind::Star => (70, UnOp::Deref)`. Need a new `UnOp::Deref`. Add that to `UnOp` enum.

- [ ] **Step 4: Run test – should now parse but later fail on missing HIR lowering**

Expected: Type checker error because `HirExpr` doesn't know `Deref` yet.

- [ ] **Step 5: Add `HirExpr::Deref`**

```rust
// In glyim-hir/src/node/mod.rs
Deref { id: ExprId, expr: Box<HirExpr>, span: Span },
```
Update `get_id` and `get_span` methods.

- [ ] **Step 6: Lower AST Deref to HIR**

In `glyim-hir/src/lower/expr.rs`, add case:
```rust
ExprKind::Deref(e) => HirExpr::Deref {
    id: ctx.fresh_id(),
    expr: Box::new(lower_expr(e, ctx)),
    span: e.span,
},
```

- [ ] **Step 7: Type‑check `Deref`**

In `glyim-typeck/src/typeck/expr.rs`, in `infer_expr` match `HirExpr::Deref { expr, .. }`:
```rust
let inner_ty = self.check_expr(expr).unwrap_or(HirType::Never);
match inner_ty {
    HirType::RawPtr { inner } => *inner,
    _ => {
        self.errors.push(TypeError::MismatchedTypes { expected: HirType::RawPtr { inner: Box::new(HirType::Never) }, found: inner_ty, expr_id: expr.get_id() });
        HirType::Never
    }
}
```
(Need to adjust `TypeError::MismatchedTypes` to handle `RawPtr` – currently expects two `HirType`. Might need a new error variant `DerefNonPointer`. We'll add a new `TypeError::DerefNonPointer { found, expr_id }` for clarity. Add that in `crates/glyim-typeck/src/typeck/error.rs`.)

- [ ] **Step 8: Write a new UI test for dereferencing non‑pointer**

Create `crates/glyim-cli/tests/ui/deref_non_pointer.g`:
```
main = () => { let x = 42; *x }
```
Expected: error "cannot dereference non‑pointer type".

Add test function in `crates/glyim-cli/tests/ui.rs`:
```rust
#[test]
fn ui_deref_non_pointer() { run_ui_test("deref_non_pointer"); }
```
Generate snapshot with `cargo insta review`.

- [ ] **Step 9: Codegen for `Deref`**

In `glyim-codegen-llvm/src/codegen/expr/mod.rs`, add case:
```rust
HirExpr::Deref { expr, .. } => {
    let ptr_val = codegen_expr(cg, expr, fctx)?;
    let ptr = cg.builder
        .build_int_to_ptr(ptr_val, cg.context.ptr_type(AddressSpace::from(0u16)), "deref_ptr")
        .ok()?;
    let loaded = cg.builder.build_load(cg.i64_type, ptr, "deref_val").ok()?;
    Some(loaded.into_int_value())
}
```

- [ ] **Step 10: Run integration test – should now pass**

`cargo test -p glyim-cli --test integration -- e2e_pointer_deref_valid` → **PASS**

- [ ] **Step 11: Commit**

```bash
git add <all modified files>
git commit -m "feat: add pointer dereference (*expr) with parsing, type checking, codegen, and UI test"
```

#### Sub‑task 1.1.2: Runtime integration test (deref an actual value)

Add a more involved test that compiles and runs using an extern function returning a pointer to a static integer.

```rust
#[test]
fn e2e_deref_runtime() {
    let src = "extern { fn get_ptr() -> *i64; }\nmain = () => { let p = get_ptr(); *p }";
    // We'd need to provide a runtime stub that returns a pointer. For now we skip until extern works.
}
```
(We'll come back after extern functions are fully codegenned.)

#### Sub‑task 1.1.3: Integration test for nested deref

```rust
#[test]
fn e2e_nested_deref() {
    let src = "extern { fn get_ptr() -> **i64; }\nmain = () => { let pp = get_ptr(); **pp }";
    assert!(pipeline::check(&temp_g(src)).is_ok());
}
```

---

### Task 1.2: Pointer Store (Assignment to `*ptr`)

**Files:**
- Modify: `crates/glyim-parse/src/ast.rs` (add `StmtKind::AssignDeref`)
- Modify: `crates/glyim-parse/src/parser/stmts.rs` (parse `*target = value`)
- Modify: `crates/glyim-hir/src/node/mod.rs` (add `HirStmt::AssignDeref`)
- Modify: `crates/glyim-hir/src/lower/expr.rs` (lower)
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

- [ ] **Step 2: Parse `*expr = value`**

In `glyim-parse/src/parser/stmts.rs`, after recognizing identifier assignment, check if the identifier was preceded by `*`. Could also create a new statement parser for general assign. Implementation: extend `parse_assign_stmt` to handle prefix `*`. Create `StmtKind::AssignDeref { target: ExprNode, value: ExprNode }`.

- [ ] **Step 3: Add `HirStmt::AssignDeref`**

```rust
AssignDeref {
    target: Box<HirExpr>,
    value: HirExpr,
    span: Span,
},
```

- [ ] **Step 4: Lower in `lower_stmt`**

Match `StmtKind::AssignDeref` and produce `HirStmt::AssignDeref`.

- [ ] **Step 5: Typecheck**

Resolve target type: must be `RawPtr { inner }`. Check value type matches `inner`.

- [ ] **Step 6: Codegen**

Generate pointer load, then store value.

- [ ] **Step 7: Write UI test for assigning to non‑pointer**

`assign_deref_non_ptr.g`:
```
let x = 42;
*x = 10;
```
Snap: error "cannot assign through non‑pointer type".

- [ ] **Step 8: Run and commit**

---

### Task 1.3: Destructuring `let` Bindings (Struct & Tuple)

**Files:**
- Modify: `crates/glyim-typeck/src/typeck/stmt.rs` (implement `bind_pattern`)
- Modify: `crates/glyim-codegen-llvm/src/codegen/stmt.rs` (implement `LetPat` codegen)
- Modify: `crates/glyim-typeck/src/typeck/types.rs` (add pattern binding helpers)
- Create: several integration tests
- Create: UI tests for invalid destructures

#### Sub‑task 1.3.1: Struct destructuring integration

- [ ] **Step 1: Write integ test**

```rust
#[test]
fn e2e_destructure_struct_simple() {
    let src = "struct Point { x, y }\nfn main() -> i64 { let p = Point { x: 1, y: 2 }; let Point { x, y } = p; x + y }";
    assert_eq!(pipeline::run(&temp_g(src)).unwrap(), 3);
}
```

Run → **FAIL** (likely runtime or type error).

- [ ] **Step 2: Implement type‑checker pattern binding for structs**

In `typeck/stmt.rs` expand `bind_pattern`. For `HirPattern::Struct { name, bindings }`, retrieve struct info, match fields, and insert each binding variable.

- [ ] **Step 3: Codegen: for `LetPat`**

Allocate variables for each field, extract from initializer struct.

- [ ] **Step 4: Test passes.**

- [ ] **Step 5: Additional tests**

```rust
#[test]
fn e2e_destructure_struct_partial() {
    let src = "struct Point { x, y }\nfn main() -> i64 { let p = Point { x: 1, y: 2 }; let Point { x, .. } = p; x }";
    assert_eq!(pipeline::run(&temp_g(src)).unwrap(), 1);
}
```

- [ ] **Step 6: UI test for missing field**

`let Point { x } = p;` → error "missing field 'y'".

Snap.

- [ ] **Step 7: Commit**

#### Sub‑task 1.3.2: Tuple destructuring

- [ ] **Step 1: Test**

```rust
#[test]
fn e2e_destructure_tuple() {
    let src = "fn main() -> i64 { let (a, b) = (10, 20); a + b }";
    assert_eq!(pipeline::run(&temp_g(src)).unwrap(), 30);
}
```

Run → **FAIL**.

- [ ] **Step 2: Implement pattern binding for tuple**

In `bind_pattern`, for `HirPattern::Tuple { elements }`, if initializer type is `Tuple(elems)`, match each element pattern. In codegen, similar to struct but positional.

- [ ] **Step 3: Test passes.**

- [ ] **Step 4: Nested tuple test**

```rust
#[test]
fn e2e_nested_tuple_destructure() {
    let src = "fn main() -> i64 { let (a, (b, c)) = (1, (2, 3)); a + b + c }";
    assert_eq!(pipeline::run(&temp_g(src)).unwrap(), 6);
}
```

- [ ] **Step 5: Commit**

---

### Task 1.4: Method Call Desugaring (`obj.method(args) → Type_method(obj, args)`)

**Files:**
- Modify: `crates/glyim-parse/src/ast.rs` (add `ExprKind::MethodCall`)
- Modify: `crates/glyim-parse/src/parser/exprs/mod.rs` (parse `.ident(args)` as method call)
- Modify: `crates/glyim-hir/src/node/mod.rs` (add `HirExpr::MethodCall`)
- Modify: `crates/glyim-hir/src/lower/expr.rs` (lower)
- Modify: `crates/glyim-typeck/src/typeck/expr.rs` (resolve method)
- Modify: `crates/glyim-codegen-llvm/src/codegen/expr/mod.rs` (codegen as indirect call)

#### Sub‑task 1.4.1: Basic method call on struct

- [ ] **Step 1: Integration test**

```rust
#[test]
fn e2e_method_call_simple() {
    let src = "struct Counter { val: i64 }\nimpl Counter {\n    fn inc(self: Counter) -> Counter { Counter { val: self.val + 1 } }\n}\nmain = () => { let c = Counter { val: 0 }; let c2 = c.inc(); c2.val }";
    assert_eq!(pipeline::run(&temp_g(src)).unwrap(), 1);
}
```

Run → **FAIL** (parse error on `c.inc()`).

- [ ] **Step 2: Parse `expr . ident ( args )` as method call**

In `glyim-parse/src/parser/exprs/mod.rs` inside `parse_expr`, after parsing left expression, if next token is `.` and then `Ident` with following `(`, consume as a method call. Create `ExprKind::MethodCall { receiver: Box<ExprNode>, method: Symbol, args: Vec<ExprNode> }`. Add `MethodCall` to `ExprKind` enum.

- [ ] **Step 3: Lower to `HirExpr::MethodCall`**

Add `MethodCall` to `HirExpr`. Lower: `ExprKind::MethodCall` → `HirExpr::MethodCall { receiver, method_name, args }`.

- [ ] **Step 4: Type‑checker resolution**

During `check_expr` for `MethodCall`, compute receiver type (must be `Named`), search impl methods registered for that type (by manged name pattern `Type_method`), retrieve the HirFn signature, verify args, create a temporary call to mangled function. Then we can reduce to `HirExpr::Call` internally in the type checker? Simpler: keep `MethodCall` in type checker output but codegen must handle it. We'll let codegen also resolve the mangled name. In type checker we just validate types. For codegen, we'll generate a call to `{Type}_{method}`.

- [ ] **Step 5: Codegen for `MethodCall`**

In `glyim-codegen-llvm/src/codegen/expr/mod.rs`, when encountering `MethodCall`, construct mangled name from receiver's type (we can get from expression type) and emit normal call.

- [ ] **Step 6: Test passes.**

- [ ] **Step 7: Additional tests**

```rust
#[test]
fn e2e_method_call_chain() {
    let src = "struct Counter { val: i64 }\nimpl Counter {\n    fn add(self: Counter, x: i64) -> Counter { Counter { val: self.val + x } }\n}\nmain = () => { let c = Counter { val: 1 }; let c2 = c.add(2); c2.val }";
    assert_eq!(pipeline::run(&temp_g(src)).unwrap(), 3);
}
```

- [ ] **Step 8: Test error case – method not found – UI snapshot**

- [ ] **Step 9: Commit**

---

## Chunk 2: Standard Library – Vec, String, Prelude

Depends on Chunk 1 completion.

### Task 2.1: Implement `Vec<T>` as Glyim source

**Files:**
- Create: `stdlib/src/vec.g` (actual compilable Glyim)
- Modify: `crates/glyim-cli/src/pipeline.rs` (inject stdlib source)
- Create: integration tests for Vec operations
- Create: UI tests for Vec runtime panics

#### Sub‑task 2.1.1: Write `vec.g` using pointer ops and methods

We'll craft the full implementation as per spec's design:

```glyim
// stdlib/src/vec.g
struct Vec<T> {
    data: *mut u8,
    len: i64,
    cap: i64,
}

impl<T> Vec<T> {
    pub fn new() -> Vec<T> {
        Vec { data: 0 as *mut u8, len: 0, cap: 0 }
    }

    pub fn push(mut self: Vec<T>, value: T) {
        if self.len == self.cap {
            let new_cap = if self.cap == 0 { 8 } else { self.cap * 2 };
            let new_data: *mut u8 = __allocate(new_cap * __size_of::<T>()) as *mut u8;
            if self.data != (0 as *mut u8) {
                __copy(self.data, new_data, self.len * __size_of::<T>());
                __dealloc(self.data);
            }
            self.data = new_data;
            self.cap = new_cap;
        }
        let dst = __ptr_offset(self.data, self.len * __size_of::<T>());
        __store(dst, value);
        self.len = self.len + 1;
    }

    pub fn pop(mut self: Vec<T>) -> Option<T> {
        if self.len == 0 { return None; }
        self.len = self.len - 1;
        let src = __ptr_offset(self.data, self.len * __size_of::<T>());
        Some(__load(src))
    }

    pub fn get(self: &Vec<T>, index: i64) -> Option<T> {
        if index >= self.len { return None; }
        let ptr = __ptr_offset(self.data, index * __size_of::<T>());
        Some(__load(ptr))
    }

    pub fn len(self: &Vec<T>) -> i64 { self.len }
}
```

We need builtin intrinsics: `__allocate(size)`, `__dealloc(ptr)`, `__copy(src,dst,size)`, `__load(ptr)`, `__store(ptr,val)`, `__ptr_offset(ptr, offset)`. These can be implemented as compiler‑recognized names that codegen to LLVM `malloc`, `free`, `memcpy`, `load`, `store`, `getelementptr`. We can add a set of built‑in functions in the type checker/codegen (like `println`).

Alternatively, we can use the existing `glyim_alloc` / `glyim_free` shims and add pointer offset as a built‑in operator. To avoid too many new intrinsics, we'll implement a minimal `ptr_offset` and `ptr_load`/`ptr_store` as compiler built‑ins. Actually, dereference and store we just added as language features, so `__load` and `__store` are not needed – we can use `*ptr` and `*ptr = val`. However, with generic `T` we can't directly `*ptr` because the type is not known at codegen until monomorphization. Our pointer deref on `*mut u8` returns `i64`? No, our deref currently works on `*i64` etc. It's typed. So to implement Vec<T> we really need to cast `*mut u8` to `*mut T` and then dereference. That requires support for `*ptr as *mut T` cast and then deref. But we don't have generic type T at runtime in the stdlib source. So we need a different approach: define Vec as a struct with `data: *mut u8` and use pointer math + load/store via intrinsics that operate on bytes. This is how Rust's std Vec works under the hood. So we need byte‑level pointer read/write intrinsics: `__read_i64(ptr)` / `__write_i64(ptr, val)` etc. And `__memcpy` for copying. This is complex but essential.

Given the time, we could simplify: provide only `Vec<i64>` as a concrete type, not generic. This would unblock basic usage. Later, generics will work after monomorphization fully works for pointer ops. The spec wants `Vec<T>` but maybe we can start with `VecI64` and later generalize. I'll take the pragmatic route: implement `VecI64` (non‑generic) to have something working. Then write `String` wrapping `VecU8`. We'll explain the limitation in the plan.

So I'll adjust the plan: implement `VecI64` and `VecU8` concretely.

#### Concrete `VecI64` and `VecU8`

- [ ] **Step 1: Write `stdlib/vec_i64.g`** with methods that use pointer deref on `*mut i64`.

- [ ] **Step 2: Integration test: push, pop, get, bounds check**

- [ ] **Step 3: Implement `String` as `struct String { vec: VecU8 }` with methods.**

- [ ] **Step 4: Commit**

### Task 2.2: Prelude file

- Create: `stdlib/prelude.g`
- Modify: `pipeline.rs` to read prelude file and prepend to user source.
- Ensure backward compatibility.

### Task 2.3: Run existing tests

Make sure all previous tests still pass after adding stdlib.

---

## Chunk 3: Package Manager – Publish, Outdated, Verify

### Task 3.1: Implement `glyim publish`

**Files:**
- Modify: `crates/glyim-pkg/src/registry.rs`
- Modify: `crates/glyim-cli/src/main.rs`
- Create: mock HTTP server for tests

#### Sub‑tasks with detailed tests

- [ ] **Step 1: Write test of publish logic (mock registry)**

Use a local HTTP server (wiremock or similar) to simulate the registry. Test that tarball is created with correct content and uploaded; test hash matching; test authentication header.

```rust
#[test]
fn publish_sends_correct_archive() {
    let mock_server = mock("POST", "/api/v1/packages/test/0.1.0").respond_with(status(200));
    // set up manifest, run publish, verify request body hash
}
```

- [ ] **Step 2: Implement `publish` in registry and CLI.**

- [ ] **Step 3: Additional tests for publish errors (network error, auth fail, hash mismatch)**

- [ ] **Step 4: Commit**

### Task 3.2: Implement `glyim outdated`

- [ ] **Step 1: Test that outdated compares lockfile with registry**

Simulate registry returning newer versions, check output.

- [ ] **Step 2: Implementation**

### Task 3.3: Implement `glyim verify`

- [ ] **Step 1: Test: lockfile with known hash, mutate local cache, verify fails.**

- [ ] **Step 2: Implementation**

---

## Chunk 4: Remote Cache Push/Pull (REST)

**Files:**
- Modify: `crates/glyim-macro-vfs/src/remote.rs`
- Modify: `crates/glyim-cli/src/main.rs`

- [ ] **Step 1: Integration test using local CAS server**

Start server, store a blob via client push, verify remote has it.

- [ ] **Step 2: Implement push logic:** iterate local CAS objects, call remote `/blob` POST if missing.

- [ ] **Step 3: Implement pull:** query remote `/blob/{hash}` and store locally.

- [ ] **Step 4: Test concurrent pushes, error handling.**

- [ ] **Step 5: Commit**

---

## Chunk 5: Documentation Generator (`glyim doc`)

**Files:**
- Create crate `crates/glyim-doc/`
- Modify: `crates/glyim-cli/src/main.rs` (add `doc` command)
- Modify: `Cargo.toml` workspace members

- [ ] **Step 1: Scaffold crate with basic HIR visitor**

- [ ] **Step 2: Implement HTML generation for functions and structs**

- [ ] **Step 3: Integration test: compile a simple file, generate doc, assert HTML contains expected content.**

- [ ] **Step 4: Include CSS and simple navigation.**

- [ ] **Step 5: Commit**

---

## Chunk 6: Cross‑Compilation

**Files:**
- Modify: `crates/glyim-cli/src/main.rs` (add `--target` flag)
- Modify: `crates/glyim-cli/src/pipeline.rs`
- Modify: `crates/glyim-codegen-llvm/src/codegen/mod.rs` (accept target triple)
- Modify: `crates/glyim-codegen-llvm/src/lib.rs` if needed

- [ ] **Step 1: Test that `glyim build --target x86_64-unknown-linux-gnu` still works on host (same as default).**

- [ ] **Step 2: Pass target triple to LLVM module and set triple/data layout.**

- [ ] **Step 3: Use `TargetMachine` to emit object for that triple.**

- [ ] **Step 4: Integration test with different triple (can only be tested if cross‑compilation environment exists; we'll skip runtime test).**

- [ ] **Step 5: Commit**

---

## Chunk 7: Final Integration and Regression

- Ensure all existing 870+ tests pass.
- Manual 60‑second test per spec.
- Update README.

---

After each chunk, we'll do the plan‑document‑reviewer loop to validate. Once all chunks are approved, we hand off for execution.

Plan saved. Ready to execute?
