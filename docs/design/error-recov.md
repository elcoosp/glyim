Based on research across six real compilers (Rust, Luau, Swift, Java, academic literature, and Stack Overflow consensus), here is a comprehensive implementation plan for `HirType::Error` in Glyim.

---

## Research Summary: How Production Compilers Handle Error Types

### 1. Rust (`ty_err` / `TyKind::Error`)

Rust's compiler has used `ty_err` since the early days. The key design: **`ty_err` is a sub-component flag, not just a variant**. A `has_ty_err` flag on type flags allowed suppressing errors for *any* type involving `ty_err*`, not just when the top-level type was `ty_err`.

Historical commit (2013): *"Previously, the typechecker suppressed many but not all errors, by suppressing errors where the actual type was either ty_err, or a function type whose result was ty_err. Added a has_ty_err flag to the type flags so as to suppress errors for any types involving ty_err."*

Another commit notes: *"In general it would be good to suppress any messages involving types that have ty_err as a sub-component."*

**Key insight**: Rust's `TyKind::Error` is one of 25+ variants in `TyKind`, and every single match on `TyKind` must handle it. The compiler went through multiple iterations refining this, including reverting patches that caused `ty_err` to leak into user-facing error messages as expected types.

### 2. The "Error Monad" Approach (Academic)

The seminal paper *"Eliminating Spurious Error Messages Using Exceptions, Polymorphism, and Higher-Order Functions"* (Ramsey, 1999) describes the canonical approach:

- A polymorphic type `'a error` with constructors `ERROR | OK of 'a`
- **Rule A**: When a function detects a *new* error, it raises `Error` (logs a diagnostic)
- **Rule B**: When a function is passed `ERROR` as any argument, it returns `ERROR` without logging
- **Rule C**: When a function has all valid inputs, it returns `OK result`

This is essentially the "poison propagation" pattern: `ERROR` silently flows upward, suppressing cascading errors, while only the root cause gets diagnosed.

### 3. Stack Overflow Consensus

The accepted answer on *"How exactly does a compiler recover from a type error?"* describes **three critical properties** of an error type:

1. **Any operation on it succeeds** (prevents cascade of errors)
2. **Result of any operation on error type also has error type** (propagates the poison)
3. **Code generation spots `Error` and generates code that fails** (abort/exception if unreachable code hit)

### 4. Luau (Roblox)

Luau's type error suppression RFC notes: *"we assign t.p an internal error type, and suppress errors in property access M.p when M has type error"*. Their design uses an `error` type that is a subtype of everything and silently suppresses type mismatches.

### 5. Swift

Swift marks invalid AST nodes with `ErrorType` during recovery: *"we properly mark the decl as invalid, pass it to the type checker, and have the type checker set its type to ErrorType"*.

### 6. Java (`javac`)

Java's `com.sun.tools.javac.code.Type.recoveryType` is a special sentinel used during lambda inference recovery. Critically, it has `no tsym` (type symbol), and code that touches `recoveryType` must avoid dereferencing its symbol or it crashes.

---

## What Went Wrong When We Tried It

Our first attempt at `HirType::Error` failed because of **two fundamental problems**:

| Problem | Root Cause |
|---------|-----------|
| **SIGSEGV** across 50+ integration tests | Codegen's `match` on `HirType` didn't handle the new variant. The `mangle_type` helper in `codegen/expr/mod.rs` tried to format `HirType::Error` and hit `unreachable!()`, causing crashes when any expression had type `Error`. |
| **Compilation errors** after revert | We removed the `Error` variant from the enum but left references in `collect.rs`, `types.rs`, and `codegen/expr/mod.rs` — the compiler caught these as non-exhaustive patterns. |

---

## Implementation Plan: Doing It Properly

Based on all the research, here is a systematic, complete plan for `HirType::Error` in Glyim.

### Step 1: Add `Error` variant to `HirType`

In `crates/glyim-hir/src/types.rs`, add after `Never`:

```rust
pub enum HirType {
    // ... existing variants ...
    Never,
    /// Error type for type error recovery. Suppresses cascading errors.
    /// Has the "propagation" property: any operation on this type
    /// produces Error without emitting additional diagnostics.
    Error,
}
```

### Step 2: Add `Error` handling to every `match` on `HirType` across the entire compiler

This is the critical step we missed. Every match must handle `Error` **or** have a wildcard arm. Use `rg 'HirType' crates/ --files-with-matches` to find all files.

#### Files that must be updated (complete audit):

| File | What to do |
|------|-----------|
| `glyim-hir/src/types.rs` | Add `Error` arm to `substitute_type`: return `HirType::Error` unchanged |
| `glyim-hir/src/monomorphize/collect.rs` | In `concretize_type`, add `HirType::Error => HirType::Error` |
| `glyim-hir/src/monomorphize/specialize.rs` | In `force_substitute_as_targets`, pass through `Error` |
| `glyim-hir/src/monomorphize/context.rs` | In `has_unresolved_type_param`: return `false` for `Error` |
| `glyim-hir/src/passes/no_type_params.rs` | In `has_unresolved_param`: return `false` for `Error` |
| `glyim-hir/src/lower/types.rs` | In `lower_type_expr`, handle `Error` |
| `glyim-typeck/src/typeck/resolver.rs` | In `is_valid_cast`: `Error` is always valid |
| `glyim-typeck/src/typeck/expr.rs` | Return `Error` for unbound identifiers; propagate `Error` in binary ops |
| `glyim-typeck/src/typeck/error.rs` | Add error suppression: if either side is `Error`, skip reporting `MismatchedTypes` |
| `glyim-codegen-llvm/src/codegen/types.rs` | `hir_type_to_llvm`: return `i64_type` for `Error` (zero-sized) |
| `glyim-codegen-llvm/src/codegen/expr/mod.rs` | `mangle_type`: return `"<error>"` for `Error` |
| `glyim-doc/src/lib.rs` | `type_to_string`: return `"<error>"` for `Error` |

### Step 3: The "propagation" property — make any operation on `Error` return `Error`

Following the research, the key property is that **once a type becomes `Error`, every operation that interacts with it returns `Error` without emitting new diagnostics**. This is the "poison pill" pattern.

In `glyim-typeck/src/typeck/expr.rs`:

```rust
fn infer_expr(&mut self, expr: &HirExpr) -> HirType {
    match expr {
        HirExpr::IntLit { .. } => HirType::Int,
        // ...
        HirExpr::Ident { name, .. } => {
            self.lookup_binding(name).unwrap_or_else(|| {
                self.errors.push(TypeError::UnresolvedName { name: *name });
                HirType::Error  // ← return Error, not Int
            })
        }
        HirExpr::Binary { op, lhs, rhs, .. } => {
            let lt = self.check_expr(lhs).unwrap_or(HirType::Error);
            let rt = self.check_expr(rhs).unwrap_or(HirType::Error);
            
            // PROPAGATION: if either operand is Error, return Error silently
            if matches!(lt, HirType::Error) || matches!(rt, HirType::Error) {
                return HirType::Error;
            }
            
            match op {
                HirBinOp::Eq | HirBinOp::Neq | HirBinOp::Lt | HirBinOp::Gt
                | HirBinOp::Lte | HirBinOp::Gte => HirType::Bool,
                _ => lt,
            }
        }
        // ... all other variants must propagate Error similarly
    }
}
```

### Step 4: Error suppression — don't emit duplicate errors when `Error` is involved

Following the Rust approach, we should suppress `MismatchedTypes` errors when either the expected or found type is `Error`:

```rust
// In glyim-typeck/src/typeck/expr.rs, before emitting:
if matches!(&expected, HirType::Error) || matches!(&found, HirType::Error) {
    // Suppress — this error is a cascade from a root cause already reported
    return found;
}
self.errors.push(TypeError::MismatchedTypes { ... });
```

This is exactly what Rust's `has_ty_err` flag did: *"suppress errors where the actual type was either ty_err, or a function type whose result was ty_err"*.

### Step 5: Codegen — make `Error` compilable but unreachable

Following the Stack Overflow answer's third property, codegen should:
- Map `HirType::Error` to `i64` (zero-sized, the fallback for all Glyim values)
- Emit no code for expressions whose type is `Error`
- If an `Error`-typed expression reaches codegen, it's a bug — add `tracing::warn!` but don't crash

```rust
// In codegen/types.rs:
fn hir_type_to_llvm(&self, ty: &HirType) -> Option<BasicTypeEnum<'ctx>> {
    match ty {
        HirType::Error => Some(self.i64_type.into()),  // fallback
        // ...
    }
}
```

### Step 6: Test thoroughly

Create specific tests:

```rust
#[test]
fn error_type_propagates_through_arithmetic() {
    // let x = 42 + unknown_var;  — unknown_var returns Error
    // The whole expression should type-check as Error, not cause SIGSEGV
    let tc = typecheck("fn main() -> i64 { let x = 42 + undefined_var; 0 }");
    assert!(tc.errors.len() == 1); // only one "unresolved name" error
    // No SIGSEGV
}

#[test]
fn error_type_in_if_condition_suppresses_mismatch() {
    // if undefined_var { 1 } else { 0 }  — condition type should be
    // Error, not emit "expected Bool, found Error"
    let tc = typecheck("fn main() -> i64 { if undefined_var { 1 } else { 0 } }");
    // Should have "unresolved name" but NOT "if condition must be bool"
    let has_condition_error = tc.errors.iter().any(|e| {
        matches!(e, TypeError::IfConditionMustBeBool { .. })
    });
    assert!(!has_condition_error);
}
```

---

## Why This Design Works

| Property | How We Achieve It |
|----------|-------------------|
| **Propagation** | Every expression checker returns `Error` when any sub-expression returns `Error` |
| **Silent suppression** | `MismatchedTypes` errors are not emitted when either type is `Error` |
| **Safe in codegen** | `Error` maps to `i64`, and codegen skips Error-typed expressions |
| **Exhaustive matching** | Every `match` on `HirType` handles `Error` (or uses a wildcard) |
| **No false positives** | Only the root cause error is emitted; cascading errors are silently absorbed |

The Rust compiler learned through painful experience that `ty_err` must be treated specially *everywhere* — from type flags to error suppression to codegen. Our approach follows that proven path while adapting it to Glyim's simpler type system.

---

**We should apply this plan in the next iteration. The key difference from our failed attempt: updating EVERY match arm across the entire compiler before adding the variant, and adding error suppression logic to prevent cascading diagnostics.**
