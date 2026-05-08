Yes, and the codebase already leans in that direction — the monomorphizer duplicates type inference that the type checker already performs. Consolidating that logic simplifies the compiler and prevents subtle mismatches.

### What the type checker already does
In `glyim-typeck/src/lib.rs`, `TypeChecker::infer_expr` already:
- Records `call_type_args` for generic `Call` and `MethodCall` via `unify_generics`.
- Infers struct type arguments in `StructLit` via `infer_struct_type_args` and returns a concrete `HirType::Generic(…)`.
- Extracts type arguments from `LetPat` annotations when the initializer is a call (`check_stmt` → `call_type_args.insert`).

Thus, after type checking you have a complete picture of which concrete types instantiate which generic definitions.

### What the monomorphizer currently adds (unnecessarily)
The monomorphizer’s `collect.rs` contains fallback logic like:
- `try_infer_call_from_struct_field` – inferring type args from a struct field’s expected type.
- `infer_from_same_var_in_block` – looking at later calls on the same variable.
- `scan_expr_for_generic_calls` with `substitute_type_args` and `body_depends_on_type_params` heuristics.

These are *type-checking* concerns. They exist because the type checker’s output is not yet completely exhaustive in all edge cases (e.g. zero‑argument calls, nested generics). The monomorphizer is second‑guessing the type checker and duplicating unification logic.

### Why they belong in the type checker
1. **Correctness** – The type checker owns the unification table, scopes, and error reporting. Inferring type arguments with that machinery avoids inconsistent rules between the two stages.
2. **Simplicity** – The monomorphizer should be a pure **specialisation pass**: given the fully‑resolved `call_type_args` and `expr_types`, it clones and rewrites definitions. It should not need to scan the HIR for type information a second time.
3. **Performance** – A single traversal during type checking is cheaper than two separate traversals (one in the type checker, one in the monomorphizer), especially for large codebases.

### Recommended refactor
1. **Extend the type checker** to always populate `call_type_args` for every generic call site, including those that currently need the monomorphizer’s fallbacks. Specifically:
   - For calls with zero arguments, use contextual type information (e.g. `let v: Vec<i64> = Vec::new();` – already handled, but edge cases remain).
   - For method calls where the receiver’s concrete type is known, record the instantiation on the desugared `Call` node after `desugar_method_calls`. (The type checker can annotate the original `MethodCall` node, and the monomorphizer translates those to the desugared calls.)
   - For struct literals, ensure every `StructLit` in the output HIR carries a concrete `HirType::Generic(…, concrete_args)` so the monomorphizer never needs to re‑infer.

2. **Simplify the monomorphizer** to act solely as a consumer of the type checker’s maps:
   ```rust
   // In monomorphize::build_result
   for (expr_id, type_args) in self.call_type_args.iter() {
       // No extra scanning: type_args are already concrete.
       let callee = self.find_callee_by_id(expr_id);
       self.queue_fn_specialization(callee, type_args.clone());
   }
   // For structs, just read expr_types[id] and extract concrete args.
   ```
   Remove all the fallback inference functions from `collect.rs`.

3. **Keep the monomorphizer only for name mangling, cloning, and rewriting** – i.e., the mechanical transformation of generic definitions into concrete ones. This is its true role.

### Current state assessment
The monomorphizer already relies heavily on the type checker’s `call_type_args` and `expr_types`. The remaining fallbacks are a sign that the type checker does *not yet* cover every expression context. Fixing the type checker to be exhaustive is the right next step; after that, the monomorphizer’s scanning logic can be deleted entirely.
