# Migration Plan: Type Inference from Monomorphize → Typeck

## Root Problem

The monomorphizer is doing **type inference** — a typeck responsibility. The type checker produces incomplete `call_type_args`, and the monomorphizer compensates with fallback heuristics. This is architecturally wrong and causes bugs. Below is a complete inventory and migration plan.

---

## Inventory: What Monomorphize Is Doing That Belongs in Typeck

### 🔴 Category A: Type Inference (belongs in typeck entirely)

| Monomorphize Function | What It Does | Why It's Typeck's Job |
|---|---|---|
| `extract_type_substitutions` | Matches param types against arg types to build a substitution map | This is unification. Typeck already has `unify_generics` via the `unify` module. Duplicated logic. |
| `infer_from_same_var_in_block` / `find_in_block` / `extract_type_args_from_call_on_var` | For a zero-arg generic call like `let x = Vec_new()`, looks at later calls on `x` to infer type args | Flow-sensitive type inference. The type checker should propagate constraints from usage context, or report ambiguity. |
| `try_infer_call_from_struct_field` | When a zero-arg generic call is a struct field value, infers type args from the field's declared type | Constraint propagation from expected type. Classic bidirectional type inference. |
| `body_depends_on_type_params` | Checks if a function body references type params in size-critical ways (SizeOf, As). Used to justify defaulting unresolved params to `Int` | This is a **soundness escape hatch**. If typeck resolved all type args, we wouldn't need to guess `Int`. The heuristic that "if the body doesn't use the type param, default to Int" is type-level reasoning. |
| The `Int` default fallbacks (2 places in `scan_expr_for_generic_calls`) | When type args can't be inferred, defaults all to `HirType::Int` | This is the type checker silently choosing a type. It should either infer correctly or emit an error. |
| `LetPat` annotation→call inference in `scan_expr_for_generic_calls` | Reads `LetPat.ty` annotation to infer call type args | Typeck's `check_stmt` for `LetPat` already partially does this, but incompletely. |

### 🟡 Category B: Name Resolution (belongs in typeck)

| Monomorphize Function | What It Does | Why It's Typeck's Job |
|---|---|---|
| `find_callee_by_id_from_hir` / `find_callee_in_expr` | Walks the entire HIR to find which function a `Call` or `MethodCall` expression refers to, including mangling `Vec` + `get` → `Vec_get` | Name resolution is typeck's domain. Typeck already resolves methods in `infer_expr` for `MethodCall`. It should record the resolved callee symbol. |
| Method mangling in `find_callee_in_expr` | `format!("{}_{}", type_name, method_name)` then interning | This is method resolution. Typeck does it during `MethodCall` inference but doesn't record the result. |

### 🟠 Category C: Type Concretization (shared responsibility, but logic should originate in typeck)

| Monomorphize Function | What It Does | Why Typeck Should Do More |
|---|---|---|
| `concretize_type` / `concretize_type_args` | Replaces `Generic(name, args)` with `Named(mangled)` when all args are concrete | This is a *rendering* of the type checker's output. Typeck should produce types where generic params are already resolved to concrete types. The mangling step (choosing the name) is monomorphize's job, but determining *which* concrete type each param maps to is typeck's. |
| `has_unresolved_type_param` | Checks if a type still contains unresolved single-letter type params | If typeck fully resolves types, this should never be true for types in `call_type_args`. It becomes a debug assertion, not runtime logic. |

### 🟢 Category D: Purely Monomorphize (stays)

| Function | What It Does | Stays Because |
|---|---|---|
| `specialize_fn` / `specialize_struct` / `specialize_enum` | Substitutes type params, produces specialized copies | Pure code generation concern |
| `substitute_expr_types` / `substitute_stmt_types` | Walks HIR and substitutes types in expressions | Part of specialization |
| `mangle_name` / `MangleTable` | Creates mangled names like `Vec_i64` | Codegen naming convention |
| `queue_fn_specialization` | Queue management | Monomorphize orchestration |
| `scan_expr_for_struct_instantiations` | Finds struct/enum literals that need specialization | Could be simplified but is about *what to specialize*, not *what types things are* |
| `process_type_specializations` | Processes the type work queue | Monomorphize orchestration |

---

## Migration Plan

### Phase 1: Enrich `TypeCheckOutput`

**Goal**: Typeck produces enough information that monomorphize never needs to infer.

```rust
pub struct TypeCheckOutput {
    pub expr_types: Vec<HirType>,
    pub call_type_args: HashMap<ExprId, Vec<HirType>>,

    // NEW: Resolved callee for every Call and MethodCall
    pub resolved_callees: HashMap<ExprId, Symbol>,

    // NEW: For MethodCall, the resolved base type name (e.g., "Vec")
    pub method_receiver_types: HashMap<ExprId, Symbol>,

    pub interner: Interner,
}
```

**Steps**:

1. **Record resolved callee in `infer_expr` for `Call`**:
   - When typeck finds the function definition for a `Call` expression, record `fn_def.name` in `resolved_callees[expr_id]`.
   - When typeck resolves via `extern_fns`, record the extern name.
   - This replaces `find_callee_by_id_from_hir` / `find_callee_in_expr`.

2. **Record resolved callee in `infer_expr` for `MethodCall`**:
   - After resolving the impl method (typeck already does this), record the mangled name (e.g., `Vec_new`) in `resolved_callees[expr_id]`.
   - Record the receiver's base type symbol in `method_receiver_types[expr_id]`.

3. **Ensure `call_type_args` is populated for ALL generic calls**:
   - Currently typeck only populates `call_type_args` via `unify_generics`. If unification fails, it returns `HirType::Error` and doesn't record partial results.
   - Change: always record `call_type_args` when a generic function is called, even if partially resolved.
   - For zero-arg generic calls where no context is available, emit a `TypeError::AmbiguousGenericCall` instead of silently leaving `call_type_args` empty.

### Phase 2: Enhance Type Checker's Inference

**Goal**: Eliminate every inference fallback in monomorphize by making typeck complete.

#### 2a: Bidirectional Type Inference (replaces `try_infer_call_from_struct_field` and `LetPat` inference)

Add an **expected type** parameter to `infer_expr`:

```rust
fn infer_expr_with_expected(&mut self, expr: &HirExpr, expected: Option<&HirType>) -> HirType
```

- When a `StructLit` field value is a `Call` to a generic function, pass the field's declared type as the expected type.
- When a `LetPat` has a type annotation, pass the annotation as the expected type for the value expression.
- When `infer_expr_with_expected` processes a `Call` to a generic function and `call_type_args` is empty, use the expected return type to build a substitution (unify `fn_def.ret` with `expected`).

This replaces:
- `try_infer_call_from_struct_field` in monomorphize
- The `LetPat` annotation→call inference in monomorphize's `scan_expr_for_generic_calls`
- The `LetPat` partial handling in typeck's `check_stmt`

#### 2b: Flow-Sensitive Inference (replaces `infer_from_same_var_in_block`)

When a generic function call has no type args and no expected type:

1. **Defer the call** — record the `ExprId` as "needs resolution."
2. **After checking the enclosing block**, look at all uses of the binding that receives the call's result.
3. **Unify** the binding's type from later uses back to the call.
4. If still ambiguous, emit `TypeError::AmbiguousGenericCall`.

This replaces `infer_from_same_var_in_block` and `find_in_block`.

Alternatively (simpler): Require explicit type annotations for zero-arg generic calls. This is what Rust does (`let x: Vec<i64> = Vec::new()`). Emit an error instead of guessing.

#### 2c: Complete Unification (replaces `extract_type_substitutions`)

The type checker already has `unify_generics` which uses the proper `UnificationTable`. Verify it handles all the cases that monomorphize's `extract_type_substitutions` handles:

| Case | `extract_type_substitutions` | `unify_generics` via `hir_type_to_ty` |
|---|---|---|
| `Named` param matching type param | ✅ | ✅ (via `param_vars`) |
| `RawPtr` vs `RawPtr` | ✅ | ✅ (via `TyKind::RawPtr`) |
| `Generic` vs `Generic` same name | ✅ | ✅ (via `TyKind::App`) |
| `Tuple` vs `Tuple` | ✅ | ✅ (needs `TyKind::Tuple` — **may be missing**) |
| `Option`/`Result` vs concrete | ❌ | Need to add |
| `Func` vs `Func` | ❌ | Need to add |

**Action**: Add `TyKind::Tuple`, `TyKind::Option`, `TyKind::Result`, `TyKind::Func` to the `ty` module and `hir_type_to_ty` conversion so unification handles all type shapes.

#### 2d: Remove the `Int` Default

In `scan_expr_for_generic_calls`, there are two places that default all unresolved type params to `HirType::Int`:

```rust
// FALLBACK 1: if body doesn't depend on type params
let concrete: Vec<HirType> = fn_def.type_params.iter().map(|_| HirType::Int).collect();

// FALLBACK 2: same thing for outer case
```

**Migration**: If typeck produces complete `call_type_args`, these fallbacks are never reached. Add `AmbiguousGenericCall` as a type error. The fallback in monomorphize becomes dead code and can be removed.

### Phase 3: Simplify Monomorphize

**Goal**: Monomorphize becomes a pure specialization engine with zero inference.

#### 3a: Replace `scan_expr_for_generic_calls` with a simple queue builder

Current: 200+ lines of inference logic, fallbacks, and HIR walking.

New (pseudocode):
```rust
fn collect_fn_specializations(&mut self) {
    for (expr_id, type_args) in &self.call_type_args {
        if type_args.iter().any(|a| self.has_unresolved_type_param(a)) {
            continue; // or panic in debug — this shouldn't happen
        }
        if let Some(callee) = self.resolved_callees.get(expr_id) {
            self.queue_fn_specialization(*callee, type_args.clone());
        }
    }
    // Walk the specialization queue, producing specialized functions
    while let Some((fn_name, type_args)) = self.fn_work_queue.pop() {
        // ... same as current, but no inference fallbacks
    }
}
```

**Deletions**:
- `extract_type_substitutions` — gone (typeck unifies)
- `infer_from_same_var_in_block` / `find_in_block` / `extract_type_args_from_call_on_var` — gone (typeck infers or errors)
- `try_infer_call_from_struct_field` — gone (typeck uses expected types)
- `body_depends_on_type_params` — gone (no more `Int` defaults)
- `find_callee_by_id_from_hir` / `find_callee_in_expr` — gone (typeck records resolved callees)
- `call_type_args_overrides` — gone (typeck's `call_type_args` is complete)
- Both `Int` default fallbacks — gone (typeck errors on ambiguity)

#### 3b: Simplify `concretize_type`

Currently walks types and replaces `Generic(name, args)` → `Named(mangled)` based on specialization results. This is fine as a rendering step, but simplify:

- Remove the `has_unresolved_type_param` guards — assert instead.
- The function stays in monomorphize (it's about choosing mangled names for codegen), but it becomes a simpler mapping.

#### 3c: Simplify `scan_expr_for_struct_instantiations`

This can stay but be simplified:
- Use `resolved_callees` and `expr_types` from typeck instead of walking HIR to find types.
- For `StructLit`, read `expr_types[id]` to get the concrete type directly instead of re-deriving it.

#### 3d: Remove `has_unresolved_type_param` from runtime paths

Replace with `debug_assert!(!self.has_unresolved_type_param(ty))` in queue operations. If this fires, it's a typeck bug.

### Phase 4: Validation

1. **Add `AmbiguousGenericCall` error to typeck** — ensure every generic call has resolved type args before monomorphize runs.
2. **Add assertions in monomorphize** — every type arg in `call_type_args` should be fully concrete. If not, it's a typeck bug, not a monomorphize bug.
3. **Test matrix**: For each current code path in monomorphize's inference, verify typeck now handles it:
   - Generic function call with args → `unify_generics` (already works)
   - Generic method call → `unify_generics` on method (already works, ensure `call_type_args` is populated)
   - Zero-arg generic call with type annotation → bidirectional inference (Phase 2a)
   - Zero-arg generic call in struct field → expected type from field (Phase 2a)
   - Zero-arg generic call assigned to var, later used → flow inference or error (Phase 2b)
   - Mangled callee resolution → `resolved_callees` (Phase 1)

---

## Summary: What Moves Where

| Current Location | Responsibility | Moves To |
|---|---|---|
| `MonoContext::extract_type_substitutions` | Unification of param/arg types | **Delete** — typeck's `unify_generics` handles this |
| `MonoContext::infer_from_same_var_in_block` | Flow-based type inference | **Move to typeck** or require annotations |
| `MonoContext::try_infer_call_from_struct_field` | Expected-type inference | **Move to typeck** via bidirectional inference |
| `MonoContext::body_depends_on_type_params` | Soundness check for `Int` default | **Delete** — remove `Int` default, add type error |
| `MonoContext::find_callee_by_id_from_hir` | Name resolution | **Delete** — typeck records `resolved_callees` |
| `MonoContext::find_callee_in_expr` | Name resolution | **Delete** — typeck records `resolved_callees` |
| `MonoContext::call_type_args_overrides` | Patching incomplete typeck output | **Delete** — typeck produces complete output |
| `MonoContext::has_unresolved_type_param` (runtime) | Compensating for incomplete typeck | **Convert to debug assertion** |
| `Int` default fallbacks | Guessing types | **Delete** — typeck errors on ambiguity |
| `TypeChecker::unify_generics` | Unification | **Enhance** to handle Tuple, Option, Result, Func |
| `TypeChecker::infer_expr` | Type inference | **Enhance** with expected-type parameter |
| `TypeCheckOutput` | Typeck output | **Enhance** with `resolved_callees`, `method_receiver_types` |

## What Stays in Monomorphize

- `specialize_fn`, `specialize_struct`, `specialize_enum` — pure substitution
- `substitute_expr_types`, `substitute_stmt_types` — AST rewriting
- `mangle_name`, `MangleTable` — name mangling for codegen
- `concretize_type` — simplified, just `Generic→Named` mapping
- `scan_expr_for_struct_instantiations` — simplified, reads typeck's types
- `process_type_specializations` — queue processing
- `collect_and_specialize` — simplified orchestration (no inference loops)

The end result: monomorphize becomes a **deterministic specialization engine** that reads fully-resolved types from typeck and produces concrete code. Zero inference, zero heuristics, zero fallbacks.
