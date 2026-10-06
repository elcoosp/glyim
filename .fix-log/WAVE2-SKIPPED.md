# Wave 2 tasks not yet applied

These are tracked for a follow-up patch cycle. Rationale for deferral is
documented inline.

## T003 [TCK-24] arg type-check
Minimal arity guard for FnDef callees applied (T003-PATCHED). Full
arg-vs-formal unification is deferred: InferenceTable::unify has a
different signature than the FnCtxt::unify wrapper, and building the
snapshot/rollback plumbing requires studying `InferenceSnapshot` usage at
every call site first.

## T010 [STD-4] env.g FFI rewrite
Reverted. The new .g code passed parsing but hit a pre-existing solver
limitation (`mismatched types: T vs u8` on patterns like
`ptr::null_mut::<u8>()` / `slice::from_raw_parts::<u8>(p, n)`). Runtime
FFI additions kept. Re-apply after the solver is fixed.

## T011 [STD-5] process.g FFI rewrite
Same situation as T010: reverted pending the solver fix. The
`Command::output` follow-up is also reverted.

## T053 [LSP-4] real pattern spans
The quick fix (skip DUMMY-span refs in rename / navigation) is applied.
The real fix requires adding a `Span` field to the HIR `Pat` node and
propagating it through every pattern constructor.

## T092 [SOLVE-28] occurs check misses Projection/Dynamic
Not applied. The occurs-check walkers (occurs, has_unresolved_non_ty_infer,
collect_unresolved_vars) skip Projection and Dynamic arms, so a
self-referential type referencing a ?T through a projection can slip
through. ProjectionTy/HrtbBinder plumbing exists; needs careful reading
of the substitution machinery before touching.
