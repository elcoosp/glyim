# Glyim Test Framework — Agent Instructions

## Mandatory: Test Location

**ALL tests MUST be written as unit tests inside `crates/*/src/tests/`, NOT as integration tests in `crates/*/tests/`.**  
This keeps compilation fast because unit tests share the crate's private API.

### Directory Structure

```
crates/glyim-type/
├── src/
│   ├── lib.rs          ← add: mod tests;
│   ├── context.rs
│   └── tests/          ← YOUR TESTS GO HERE
│       ├── mod.rs      ← cumulative: append mod declarations, DO NOT overwrite
│       ├── interning.rs
│       ├── freeze.rs
│       └── substitution.rs
└── Cargo.toml
```

### How to Wire It Up

1. **In `crates/<crate>/src/lib.rs`** – ensure `#[cfg(test)] mod tests;` exists. If missing, add it **at the end** of the file using a `::REPLACE` block (see example below).

2. **In `crates/<crate>/src/tests/mod.rs`** – this file lists all test submodules.  
   - You MUST **read the existing content** from the source context provided in the user prompt.  
   - Append your new `mod my_module;` lines **at the end**, preserving existing lines.  
   - Output a `::WRITE` block with the full file content (original + new lines).  
   - **NEVER remove or reorder existing lines** – this would delete other streams' test registrations.

---

## Example: Adding a Test Module

Assume the current `crates/glyim-codegen/src/tests/mod.rs` already contains:

```rust
mod abi;
mod aggregate;
```

You need to add `mod slice_projection;`. You will output:

```glyim-ops
::WRITE crates/glyim-codegen/src/tests/mod.rs
mod abi;
mod aggregate;
mod slice_projection;
::END
```

If the file does not exist, create it with only your module(s).

---

## Example: Adding `#[cfg(test)] mod tests;` to `lib.rs`

If the line is missing, append it at the end of the file. For example, if `lib.rs` ends with:

```rust
pub mod vtable;
```

You would output:

```glyim-ops
::REPLACE crates/glyim-codegen/src/lib.rs
---FIND---
pub mod vtable;
---REPLACE---
pub mod vtable;

#[cfg(test)]
mod tests;
::END
```

---

## Using `glyim-test` Crate

Add `glyim-test` as a dev-dependency in `Cargo.toml`:

```glyim-ops
::REPLACE crates/glyim-codegen/Cargo.toml
---FIND---
[dev-dependencies]
---REPLACE---
[dev-dependencies]
glyim-test = { workspace = true }
::END
```

If the `[dev-dependencies]` section does not exist, you may need to add it after the regular dependencies.

Then write test files as normal Rust modules inside `src/tests/`. Each test file has full access to `pub(crate)` items.

---

## Core APIs Reference

### Type Context Helpers

| Function | Signature | Purpose |
|----------|-----------|---------|
| `test_ty_ctx()` | `-> TyCtxMut` | Create a fresh mutable type context |
| `test_frozen_ty_ctx()` | `-> TyCtx` | Create a frozen (Send+Sync) type context |
| `with_fresh_ty_ctx(f)` | `(TyCtx, R)` | Run function with TyCtxMut, freeze, return both |

### Type Construction

Use `TyCtxMut` methods. **NEVER call `Ty::from_raw()` or `Substitution::from_raw()`** — these are `pub(crate)`.

| Method | Returns |
|--------|---------|
| `ctx.bool_ty()` | `Ty::BOOL` |
| `ctx.never_ty()` | `Ty::NEVER` |
| `ctx.unit_ty()` | `Ty::UNIT` |
| `ctx.mk_ty(TyKind::Int(IntTy::I32))` | `i32` type |
| `ctx.mk_ref(Region::Erased, inner, Mutability::Not)` | `&T` |
| `ctx.intern_substitution(vec![GenericArg::Ty(ty)])` | `Substitution` |

### Type Assertions

Two APIs: **panic-based** (`assert_ty`) and **Result-based** (`check_ty`).

```rust
use glyim_test::{assert_ty, check_ty};

// Panic-based
assert_ty(&ctx, ty).is_int(IntTy::I32);
assert_ty(&ctx, ty).is_ref(Mutability::Mut).is_bool();

// Result-based
let result = check_ty(&ctx, ty).is_int(IntTy::I32).finish();
assert!(result.is_ok());
```

### MIR Assertions

```rust
use glyim_test::assert_mir;

assert_mir(&ctx, &body)
    .block_count(3)
    .local_count(5)
    .block_terminator(0, "Goto");
```

### Diagnostic Assertions

```rust
use glyim_test::{assert_no_errors, assert_has_errors, assert_error_count, assert_diag_contains};

assert_no_errors(&diagnostics);
assert_has_errors(&diagnostics);
assert_error_count(&diagnostics, 2);
assert_diag_contains(&diagnostics, "mismatched types");
```

### Layout Assertions

```rust
use glyim_test::assert_layout;

assert_layout(&ctx, bool_ty, 1, 1);  // size=1, align=1
```

### Snapshot Testing

```rust
use glyim_test::{snapshot_cst, snapshot_mir, snapshot_def_map};

snapshot_cst("parse_fn", "fn main() {}");
snapshot_mir("lower_add", &ctx, &body);
snapshot_def_map("simple_module", &def_map);
```

---

## Mock Implementations

All mocks implement their real upstream traits. Use them when your stream depends on a crate that isn't implemented yet.

### MockSolver (implements `TraitSolver`)

```rust
use glyim_test::mock::MockSolver;
use glyim_solve::TraitSolver;

let mut solver = MockSolver::new()
    .respond_for_any(SolverResult::Proven);
```

### MockCodegen (implements `CodegenBackend`)

```rust
use glyim_test::mock::MockCodegen;
let mock = MockCodegen::new();
```

### TestDbBuilder

```rust
use glyim_test::mock::TestDbBuilder;
let db = TestDbBuilder::new()
    .name("my_test")
    .target_triple("x86_64-unknown-linux-gnu")
    .file(PathBuf::from("main.g"), Arc::from("fn main() {}"))
    .build();
```

---

## TDD Workflow for Agents

1. **Examine the current `mod.rs`** – read its content from the source context in the user prompt.
2. **Create test file(s)** in `crates/<crate>/src/tests/` using `::WRITE`.
3. **Append `mod` declarations to `mod.rs`** using a `::WRITE` that contains the original content plus your new lines at the end.
4. **Write ALL test cases** from your stream brief BEFORE implementing.
5. **Ensure tests compile** (they will fail at runtime).
6. **Implement** until all tests pass.
7. **Run full verification:**
   ```bash
   cargo test -p <crate>
   cargo clippy -p <crate> -- -D warnings
   cargo fmt --check -p <crate>
   cargo check --workspace
   ```

---

## Anti-Patterns to Avoid

| Don't | Do |
|-------|----|
| `Ty::from_raw(0)` | `Ty::ERROR` sentinel |
| `Substitution::from_raw(...)` | `ctx.intern_substitution(...)` |
| Integration tests in `tests/` | Unit tests in `src/tests/` |
| `todo!()` in non-test code | `tracing::warn!("STUB: reason")` |
| Silent no-ops | Visible stub with tracing warning |
| Stringly-typed errors | `GlyimDiagnostic` constructors |
| Writing tests after implementation | Write ALL tests first (TDD) |
| Overwriting `tests/mod.rs` | Read the file first, then append your `mod` lines |
| Running only your test module | Run the full crate suite |
