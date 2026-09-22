# Glyim Compiler De-Stubbing Plan (source-verified)

**How this document was produced:** the original `KNOWN_GAPS.md`-style report was
checked line-by-line against the actual `dump.txt` source tree (295 files, ~75k
lines), not taken on faith. Two consequences:

1. Two items the report calls "gaps" are **already fully implemented** — no work
   needed, don't let an agent "fix" something that isn't broken.
2. Two items are **worse or deeper** than the report describes. In particular
   #2 (for-loops) is not a rare fallback, it is the *only* code path that ever
   runs in production, and #8 (array drop glue) is a symptom of a missing
   `TyKind::Array` case in the core substitution function, not a bug local to
   the drop-glue generator.

Work is ordered by **actual severity as verified in source**, not by the
report's original numbering.

---

## Audit summary

| # | Report claim | Verified status | Action |
|---|---|---|---|
| 5 | ThinLTO not wired | **False — fully implemented.** `glyim-cli/src/linker.rs::thin_lto_link` shells out to `llvm-lto2`; wired end-to-end in `main.rs` (~L3909) and `LlvmBackend::generate` (~L5987, `glyim-codegen-llvm/src/lower.rs`). | **None.** |
| 4 | ConstRef always zero-init | **Mostly false.** `PipelineLowerCtx::const_value`/`cv_const` (`glyim-pipeline/src/mono_cache.rs` ~L47995-48119) already folds scalar/tuple/array/struct consts. Only `ConstValue::Range` still falls back. | Small fix (Phase 6). |
| 2 | For-loop fallback "only in tests" | **Worse than reported.** `PipelineLowerCtx` never overrides `LowerCtx::iterator_next_fn`; the trait default (`None`) always applies. **Every for-loop in every production build takes the one-iteration fallback.** | **P0 — Phase 1.** |
| 8 | Array drop glue no-ops for generic length | **Deeper than reported.** `TyCtx::subst_ty` (`glyim-type`, ~L64569) has no arm for `TyKind::Array` and takes only `HashMap<u32, Ty>` (no const-generic substitution at all). The array-length param can never be resolved during monomorphization. | **P0 — Phase 2.** |
| 1/10 | Async multi-poll returns `Pending` forever | Confirmed exactly as reported (`glyim-hir/src/lower/lower_async.rs` ~L814-1051). | P1 — Phase 3. |
| 3 | Partial moves not modeled | Confirmed. Infrastructure (`drop_flags`, guarded `SwitchInt` drop) already built in `glyim-lower/src/builder.rs`; never populated because `lower_rvalue.rs` hardcodes `Operand::Copy` for every field projection. | P1 — Phase 4. |
| 7 | Deref autoderef missing for ADTs | Confirmed. `TyCtx::deref_ty` (`glyim-type`, ~L64076) only handles `Ref`/`RawPtr`. | P1 — Phase 5. |
| 6 | Windows SEH uses Itanium landingpads | Confirmed, genuine toolchain gap (inkwell/llvm-sys don't export funclet C-API). | P2 — Phase 7. |
| 9 | Proc-macro two-stage build not integrated | Confirmed, but narrower than it sounds: cdylib codegen, `dlopen` loader, and in-process `Registry` are **already done** (`glyim-proc-macro/src/lib.rs`). Only `glyim-cli` orchestration is missing. | P2 — Phase 8. |
| — | Inclusive range slicing (`..=`) unsupported | Confirmed, `glyim-lower/src/lower_rvalue.rs`. | P3 — Phase 9. |

---

## Phase 0 — Regression harness (do this first, before touching anything)

Every phase below changes codegen for previously-"working" (but silently
wrong) programs. Before any code changes, add characterization tests that
pin the **current broken behavior**, so CI immediately shows red/green
instead of an agent silently believing it succeeded.

```bash
cd glyim-test
mkdir -p src/fixtures/destub
```

Create one `*.glyim` + expected-output pair per phase up front:

```
src/fixtures/destub/for_loop_sum.glyim        # sum a Vec<i32>, expect real total
src/fixtures/destub/array_drop_generic_n.glyim # struct Foo<const N: usize>([Bar; N])
src/fixtures/destub/async_two_awaits.glyim     # two sequential .await points
src/fixtures/destub/partial_move_field.glyim   # move struct field, use the rest
src/fixtures/destub/box_deref_method.glyim     # Box<T>.method() where method is on T
src/fixtures/destub/inclusive_range_slice.glyim
```

Run each through `compile_and_run_compiled` (already exists in
`glyim-test` harness, Linux-only — that's fine, CI is Linux) and assert on
stdout/exit code, not on "it compiled." A stub that silently loops once or
zero-inits will produce a *wrong number*, which is exactly what these tests
must catch.

---

## Phase 1 — Iterator for-loop fallback (P0, highest priority)

### Root cause

`glyim-lower/src/lower_rvalue.rs` (`ExprKind::For` lowering, ~L465) is
already correct — it calls `self.ctx.iterator_next_fn(iter_ty, elem_ty)` and
has a full, working "real" path (allocates `&mut iter`, calls `next()`,
switches on the `Option` discriminant, binds the payload, loops). The bug is
one layer up: `glyim-pipeline/src/mono_cache.rs`'s `impl LowerCtx for
PipelineLowerCtx<'a>` (~L47903-48021) **never implements
`iterator_next_fn`**, so it silently inherits the trait's stub default at
`glyim-lower/src/lower.rs` (~L32449):

```rust
fn iterator_next_fn(&self, _iter_ty: Ty, _elem_ty: Ty) -> Option<IteratorNextInfo> {
    None
}
```

Every `for x in y { ... }` in every real build hits this, and only the loop
body's first iteration ever runs.

### Fix

1. **Find the `Iterator` trait's `next` method through the same trait-impl
   lookup typeck already uses.** Typeck resolved `for` loops to an
   `IntoIterator`/`Iterator` implementation at some point (or the type
   checker wouldn't have accepted the loop) — that resolution needs to be
   threaded through to lowering instead of being thrown away. Two ways to do
   this depending on what typeck currently stores:

   - **If typeck already records the resolved `next` `FnDefId` + substs on
     the THIR `ExprKind::For` node** (check `glyim-typeck/src/check_expr.rs`
     and `glyim_typeck::thir::ExprKind::For`): thread it straight through —
     `PipelineLowerCtx::iterator_next_fn` becomes a pure data lookup, no
     new trait solving needed.
   - **If typeck does not record it** (more likely, since it clearly wasn't
     wired to lowering): `PipelineLowerCtx` needs its own trait-impl lookup,
     mirroring what `glyim-typeck/src/check_expr.rs::resolve_method_call`
     already does for method calls (collect impls, unify `Self` with
     `iter_ty`, find the `next` method) but restricted to the `Iterator`
     trait specifically.

2. Implement `IteratorNextInfo` construction in `mono_cache.rs`:

```rust
// glyim-pipeline/src/mono_cache.rs

impl<'a> LowerCtx for PipelineLowerCtx<'a> {
    // ... existing methods unchanged ...

    fn iterator_next_fn(&self, iter_ty: Ty, elem_ty: Ty) -> Option<IteratorNextInfo> {
        // 1. Find the `Iterator` impl whose `Self` unifies with `iter_ty`.
        //    Mirror glyim-typeck/src/check_expr.rs::resolve_method_call's
        //    impl-collection loop, but scoped to hir items implementing the
        //    `Iterator` trait (impl_item.trait_ref matches "Iterator").
        let iterator_trait_name = self.ty_ctx.interner().intern("Iterator");
        for item in self.hir.items.iter() {
            let ItemKind::Impl(impl_item) = &item.kind else { continue };
            let Some(trait_ref) = &impl_item.trait_ref else { continue };
            if trait_ref.segments.last().map(|s| s.name) != Some(iterator_trait_name) {
                continue;
            }
            // Resolve the impl's Self type and check it unifies with iter_ty
            // (structurally equal is enough post-monomorphization: by the
            // time lowering runs, iter_ty is concrete, not a type variable).
            let self_ty = /* resolve_type_ref(impl_item.self_ty, ...) */;
            if self_ty != iter_ty {
                continue;
            }
            let next_method = impl_item.methods.iter().find(|m| {
                self.ty_ctx.name_str(m.name) == "next"
            })?;
            let fn_def_id = /* look up the FnDefId typeck registered for this
                                impl method's body -- same id space that
                                fn_sig()/const_value() already use */;
            let option_ty = /* Ty for Option<elem_ty>: construct via
                                self.ty_ctx's known `Option` AdtId + elem_ty,
                                looked up the same way check_expr.rs resolves
                                `Option<T>` paths */;
            return Some(IteratorNextInfo {
                fn_def_id,
                fn_substs: Substitution::empty(),
                fn_ty: /* FnPtr(&mut iter_ty) -> option_ty */,
                ref_iter_ty: self.ty_ctx.mk_ref(Region::Erased, iter_ty, Mutability::Mut),
                option_ty,
                discr_ty: Ty::USIZE, // whatever discriminant type Option uses; check adt_repr
            });
        }
        None
    }
}
```

   Exact plumbing (how to go from an `ImplMethod` to a callable `FnDefId`,
   how `Option<T>`'s `AdtId` is looked up) depends on internal registries
   this plan can't see without reading `glyim-def-map` and
   `glyim-typeck/src/tyconv.rs` in full — **an agent implementing this must
   grep for how `resolve_method_call` in `check_expr.rs` turns a matched
   `ImplMethod` into something callable, and copy that exact mechanism.**
   Do not invent a new lookup path; reuse the existing one so behavior stays
   consistent with what typeck already accepted.

3. **Fail loudly instead of silently** if no impl is found for a type that
   typeck accepted in a `for` loop — that indicates a typeck/lowering
   desync bug, not a legitimate "no impl" case:

```rust
None => {
    self.diagnostics.push(GlyimDiagnostic::internal_error(&format!(
        "internal error: for-loop over `{}` type-checked but no Iterator::next \
         impl was found during lowering (typeck/lowering desync) — this is a \
         compiler bug, not a user error",
        self.ctx.ty_ctx().display_ty(iter_ty),
    )));
    // still emit a Call-based well-formed CFG so codegen doesn't choke,
    // but never silently degrade to "run once and break."
}
```

   Delete the "run body once and break" fallback arm in
   `lower_rvalue.rs` entirely once `PipelineLowerCtx` reliably returns
   `Some`. Keep the `None` branch **only** in `MockLowerCtx` (tests) where
   it is explicitly documented and intentional.

### Tests

- `for_loop_sum.glyim`: `let v = [1,2,3,4,5]; let mut s = 0; for x in v.iter() { s += x; } return s;` must return `15`, not `1`.
- A loop over a user-defined type implementing `Iterator` manually (not just array/`Vec`), to prove the general trait-impl path works, not just a builtin special case.

---

## Phase 2 — Const-generic array drop glue (P0)

### Root cause

Not `glyim-pipeline/src/mono_cache.rs::generate_array_drop_glue` (~L494) —
that function is correct *given* a fully-substituted `Ty`. The real bug is
in `glyim-type`'s `TyCtx::subst_ty` (~L64569): it substitutes
`Param`/`Ref`/`Adt`/`Tuple`/`FnDef`/`Projection` but has **no arm for
`TyKind::Array`**, and its signature only accepts a **type**-param map
(`HashMap<u32, Ty>`), with no way to carry const-generic substitutions at
all:

```rust
pub fn subst_ty(&mut self, ty: Ty, subst: &std::collections::HashMap<u32, Ty>) -> Ty {
    ...
    // TyKind::Array is not matched here — falls through to the default arm
    // and is returned completely unsubstituted, `ConstKind::Param` intact.
}
```

Separately, polymorphize's `mark_used_params` (`glyim-lower/src/mono.rs`
~L601) also only recurses into the array's *element type*, never marks the
length const's param index as used:

```rust
TyKind::Array(inner, _) => {
    mark_used_params(*inner, ctx, used);   // <-- length const ignored
}
```

So even once `subst_ty` can substitute const generics, polymorphize could
still decide `N` is "unused" and merge monomorphizations for different `N`,
corrupting array layouts. **Both must be fixed together, in this order.**

### Fix

**Step 2a — extend the substitution map to carry const-generic args.**

```rust
// glyim-type/src/... (wherever TyCtx::subst_ty lives)

/// A substitution now carries both type-param and const-param replacements.
/// Keying by the same u32 index space `ParamTy`/`ParamConst` already use.
pub struct TySubstMap {
    pub tys: std::collections::HashMap<u32, Ty>,
    pub consts: std::collections::HashMap<u32, Const>,
}

pub fn subst_ty(&mut self, ty: Ty, subst: &TySubstMap) -> Ty {
    if subst.tys.is_empty() && subst.consts.is_empty() {
        return ty;
    }
    let kind = self.ty_kind(ty).clone();
    let r = match kind {
        TyKind::Param(pt) => subst.tys.get(&pt.index).copied().unwrap_or(ty),

        TyKind::Array(inner, len) => {
            let new_inner = self.subst_ty(inner, subst);
            let new_len = self.subst_const(&len, subst);
            self.mk_ty(TyKind::Array(new_inner, new_len))
        }
        TyKind::Slice(inner) => {
            let new_inner = self.subst_ty(inner, subst);
            self.mk_ty(TyKind::Slice(new_inner))
        }

        // ... existing Ref / Adt / Tuple / FnDef / Projection arms unchanged,
        // just update their recursive subst_ty calls to pass `subst: &TySubstMap`
        // instead of `&HashMap<u32, Ty>` ...

        _ => return ty,
    };
    r
}

/// New: substitute a `Const`, resolving `ConstKind::Param` the same way
/// `subst_ty` resolves `TyKind::Param`.
pub fn subst_const(&mut self, c: &Const, subst: &TySubstMap) -> Const {
    match &c.kind {
        ConstKind::Param(ParamConst { index, .. }) => {
            subst.consts.get(index).cloned().unwrap_or_else(|| c.clone())
        }
        _ => c.clone(),
    }
}
```

Every call site of `subst_ty(ty, &HashMap<u32, Ty>)` needs updating to build
a `TySubstMap` instead. Search for all callers:

```bash
grep -rn "subst_ty(" glyim-lower glyim-pipeline glyim-typeck glyim-type
```

The monomorphization worklist in `glyim-lower/src/mono.rs` is where
`GenericArg::Const` values from a `MonoItem`'s `Substitution` need to flow
into `TySubstMap::consts` — find where it currently builds the
`HashMap<u32, Ty>` for type args (search `GenericArg::Ty(t) =>` in
`mono.rs`) and add the sibling `GenericArg::Const(c) => consts.insert(index, c.clone())` arm right next to it.

**Step 2b — fix polymorphize's param-usage tracking.**

```rust
// glyim-lower/src/mono.rs

fn mark_used_params(ty: Ty, ctx: &dyn TypeLookup, used: &mut [bool]) {
    match ctx.ty_kind(ty) {
        TyKind::Array(inner, len) => {
            mark_used_params(*inner, ctx, used);
            mark_used_params_in_const(len, ctx, used);   // <-- the fix
        }
        // ... unchanged otherwise ...
    }
}
```

(`mark_used_params_in_const` already exists at ~L647 and is correct — it
was just never called from the `Array` arm.)

**Step 2c — make the drop-glue fallback a hard assertion, not a silent
no-op.** Once 2a/2b land, reaching `generate_array_drop_glue` with a
non-constant length is a genuine compiler bug (monomorphization should have
resolved it), not a legitimate case:

```rust
// glyim-pipeline/src/mono_cache.rs, generate_array_drop_glue
let n = match len.kind {
    glyim_type::ConstKind::Uint(n) => n,
    glyim_type::ConstKind::Int(n) => n as u128,
    _ => {
        panic!(
            "internal error: array drop glue requested for `[T; {:?}]` with a \
             non-monomorphic length after monomorphization — this means \
             TyCtx::subst_ty or polymorphize's param-usage tracking has a bug. \
             len = {:?}",
            len, len
        );
    }
};
```

Panicking here during development/CI turns "silently leaks memory in
release" into "caught immediately by the regression test," which is the
entire point of de-stubbing.

### Tests

```
struct Buf<const N: usize> { data: [DropCounter; N] }
```
Instantiate `Buf::<3>` and `Buf::<7>`, drop both, assert the global drop
counter equals `3 + 7 = 10`. This test is worthless without Step 2a/2b — it
will silently pass-with-a-leak under the old code (no observable crash), so
the assertion must count *actual* drop calls, not just "did it not crash."

---

## Phase 3 — Async multi-poll state machine (P1)

### Root cause

Confirmed exactly as reported. `desugar_one_async_fn_state_machine`
(`glyim-hir/src/lower/lower_async.rs` ~L814) builds a real `FooState` enum,
a real `FooFuture` struct, and a real `impl Future for FooFuture`, but the
generated `poll` body's `Start`/`S_k` match arms are hardcoded to
`Poll::Pending`, and `Done` panics. `collect_suspend_points` (~L121) and
`compute_live_across_suspends` (~L517) — the two pieces of infrastructure a
real resume-dispatch transform needs — **already exist and already walk
full structured control flow** (`if`/`match`/`while`/`loop`/`for`).

### Architectural recommendation: do the split at MIR, not HIR

Building resume-dispatch in HIR means re-solving "split an expression tree
with arbitrary nested control flow at N marked points" — that's exactly
what a control-flow graph is for, and glyim already lowers to MIR with a
real CFG (`BasicBlockIdx`/`Terminator`) before codegen. Two engineering
reasons to move this pass to MIR instead of extending the HIR skeleton:

1. `glyim-borrowck/src/liveness.rs::compute_liveness` **already does
   backward dataflow liveness on a MIR `Body`** — exactly the "which locals
   are live across each suspend point" computation `S_k`'s fields need.
   Reuse it (change `pub(crate) fn compute_liveness` to `pub fn` and export
   `LivenessResult` from `glyim-borrowck`) instead of writing a second,
   HIR-based liveness analysis (the current `compute_live_across_suspends`
   is a hand-rolled name-collection walk, not real liveness — it will
   over-capture in the presence of shadowing/scoping).
2. HIR has no basic blocks, so "jump to the right resume point" has no
   natural representation there. MIR's `Goto`/`SwitchInt` terminators are
   exactly the right shape for a resume dispatch table.

### Fix — scoped v1, honest about its boundary

A fully general async transform (arbitrary control flow, awaits inside
loops) is the single largest item in this plan — treat it as its own
project, not a quick patch. Ship a **correct, real v1** for the common and
overwhelmingly most useful shape — sequential top-level awaits, including
inside `if`/`match` tail position — and have the desugarer **emit a clear
diagnostic error**, not a silent miscompile, for shapes it doesn't yet
support (awaits inside `while`/`loop`/`for` bodies). This mirrors how real
generator/coroutine transforms are shipped incrementally in production
compilers.

1. **New pass location:** `glyim-lower/src/async_state_transform.rs`, run
   *after* the async wrapper fn's un-desugared body (still containing
   `Expr::Await`, treated for this purpose as a normal call-like node
   `<fut>.__glyim_poll_step()`) has been lowered to MIR by the normal
   `MirBuilder`, and *before* the state-enum/future-struct HIR items are
   finalized — i.e. lower first, transform second.

2. **Detect unsupported shapes and reject clearly:**

```rust
// glyim-lower/src/async_state_transform.rs

/// Returns Err with a diagnostic if `body` contains an await inside a loop
/// (`while`/`loop`/`for`) — the v1 transform does not support resuming into
/// a loop body's mid-iteration state. Extend this once the loop case is
/// implemented (see "v2" note below).
fn reject_unsupported_suspend_shapes(
    hir_body: &glyim_hir::Body,
    suspend_points: &[SuspendPoint],
) -> Result<(), GlyimDiagnostic> {
    for sp in suspend_points {
        if await_is_inside_loop(hir_body, sp.await_expr) {
            return Err(GlyimDiagnostic::error(
                "`.await` inside a loop body is not yet supported by the async \
                 state-machine lowering (tracked: KNOWN_GAPS.md async-v2). Hoist \
                 the await out of the loop, or collect futures into a Vec and \
                 await them sequentially outside the loop.",
            ).with_span(hir_body.expr_spans[sp.await_expr]));
        }
    }
    Ok(())
}
```

   This is the single most important line in this phase: it converts a
   silent infinite-`Pending` miscompile into a compile-time error with an
   actionable message. **Never let unsupported-shape detection silently
   fall through to the old skeleton behavior.**

3. **For supported (non-loop) bodies, split the MIR CFG at each `.await`
   call terminator into `N+1` segments**, driven by `compute_liveness`:

```rust
// After MirBuilder has produced `body: Body` for the poll function, where
// each `.await` was lowered as a `Call` terminator to a sentinel
// `__glyim_poll_step` function (so it shows up as a normal terminator we can
// find and rewrite):

struct SuspendSite {
    call_bb: BasicBlockIdx,      // the block containing the await's poll() call
    resume_bb: BasicBlockIdx,    // where execution continues after Ready
    live_locals: Vec<LocalIdx>,  // from compute_liveness's live_out at call_bb
}

fn split_at_suspend_points(
    body: &mut Body,
    state_enum_def: &StateEnumLayout,   // field layout built by build_state_enum
    suspend_sites: &[SuspendSite],
) {
    for (k, site) in suspend_sites.iter().enumerate() {
        // 1. At `call_bb`, after the existing poll()-call terminator, insert a
        //    SwitchInt on the Poll<T> discriminant (Ready=1/Pending=0, check
        //    actual repr):
        //      Ready(v) -> bind v, fall through to `resume_bb` (unchanged
        //                  control flow -- this is the "future resolves on
        //                  first poll" fast path, preserved from v0)
        //      Pending  -> store `live_locals` + the suspended future into
        //                  `self.state = FooState::S{k} { fut, live0, live1, .. }`,
        //                  then `return Poll::Pending`
        let pending_bb = body.new_block_for_pass();
        // build_state_store_and_return_pending(body, pending_bb, state_enum_def, k, site);

        // 2. At function entry, the existing `match self.state { .. }` (already
        //    generated by build_future_impl's poll body) must jump to `resume_bb`
        //    for the `S{k}` arm instead of returning Pending -- rewrite that
        //    arm's body to:
        //      a. destructure the live locals + `fut` back out of `self.state`
        //      b. re-poll `fut`; SwitchInt Ready/Pending exactly as in step 1
        //         (a resumed future might immediately suspend again)
        //      c. on Ready: fall through into `resume_bb`'s original code
        // rewrite_resume_arm(body, k, site, state_enum_def);
    }

    // 3. The final (post-last-await) segment's normal return path becomes
    //    `self.state = FooState::Done; return Poll::Ready(<tail>)`.
}
```

   The two elided helper functions (`build_state_store_and_return_pending`,
   `rewrite_resume_arm`) are mechanical CFG surgery using the same
   `BasicBlockData`/`Terminator`/`StatementKind::Assign`
   patterns already used all over `glyim-lower/src/builder.rs` and
   `glyim-pipeline/src/mono_cache.rs` — an agent should model them directly
   on `generate_struct_drop_glue`'s block-chaining style (build blocks
   back-to-front, wire `target`s), not invent a new IR-construction idiom.

4. **Update `build_state_enum`** (`lower_async.rs` ~L660) to use each
   suspend site's *real* `live_locals` types (from the MIR `LocalDecl`s,
   now that they're computed post-lowering) instead of the current
   placeholder `i32` field types — this was previously impossible because
   the HIR-level pass ran pre-typeck and had no concrete types available;
   running post-MIR fixes that for free.

5. **v2 (follow-up, not in this phase):** awaits inside loops need the loop
   body itself split into pre-suspend/post-suspend halves and the loop's
   back-edge threaded through the resume dispatch — same technique, applied
   recursively to the loop's sub-CFG. Leave `reject_unsupported_suspend_shapes`
   in place until this lands; do not attempt it in the same change as v1.

### Tests

```
async fn two_step(a: i32, b: i32) -> i32 {
    let x = ready(a).await;
    let y = ready(b).await;
    x + y
}
```
Poll manually via a mock executor that returns `Pending` the *first* time
each future is polled and `Ready` the second time (this is the case v0's
"treat Pending as a panic" could never survive) — assert the final result
is `a + b` and that `poll()` was called more than once per future.

Also add the "must reject" test:
```
async fn bad() -> i32 {
    let mut sum = 0;
    for i in 0..3 { sum += ready(i).await; }
    sum
}
```
assert compilation fails with the async-v2 diagnostic, not a silent
miscompile.

---

## Phase 4 — Partial moves / drop-flag population (P1)

### Root cause

All the machinery already exists and is correct:
`MirBuilder::drop_flags: HashMap<LocalIdx, LocalIdx>` and the guarded
`SwitchInt`-wrapped `Drop` in `elaborate_scope_drops`
(`glyim-lower/src/builder.rs` ~L236-260) are fully implemented and tested —
they're just permanently inert because `drop_flags` is never populated. The
actual gap is that `glyim-lower/src/lower_rvalue.rs` always lowers a field
projection as `Operand::Copy` (two call sites: `lower_expr_to_rvalue`'s
`ExprKind::Field` arm ~L602-623, and `lower_expr_to_place`'s twin at
~L1016-1027 feeds into a `Move`-producing caller elsewhere), regardless of
whether the field's type is actually `Copy`.

### Fix

**Step 4a — make field access respect `Copy`/`Move` based on the field's
type**, in `lower_rvalue.rs`:

```rust
thir::ExprKind::Field { receiver, field, ty: field_ty } => {
    let base_place = self.lower_expr_to_place(receiver);
    let field_idx = match self.resolve_field_index(receiver.ty, *field, expr.span) {
        Some(idx) => idx,
        None => return glyim_mir::Rvalue::Use(glyim_mir::Operand::Constant(
            glyim_mir::MirConst { kind: glyim_mir::MirConstKind::Error, ty: *field_ty, span: expr.span },
        )),
    };
    let place = self.place_with_projection(base_place, ProjectionElem::Field(field_idx));

    // Tier 1.8: a non-Copy field being read here is being *moved out* of its
    // parent (this expression is being consumed as an rvalue, not borrowed --
    // `&x.field` goes through ExprKind::Ref, not this arm). Register a drop
    // flag for the whole parent local so elaborate_scope_drops guards its
    // end-of-scope Drop, and clear the flag right here at the move site.
    let field_is_copy = self.ctx.ty_ctx().is_copy(*field_ty);
    let operand = if field_is_copy {
        glyim_mir::Operand::Copy(place)
    } else {
        self.register_partial_move(base_place.local, expr.span);
        glyim_mir::Operand::Move(place)
    };
    glyim_mir::Rvalue::Use(operand)
}
```

**Step 4b — add the flag-clearing helper to `MirBuilder`**
(`glyim-lower/src/builder.rs`), next to `elaborate_scope_drops`:

```rust
/// Ensure `local` has a drop-flag, and emit the statement that clears it
/// (marks the whole value as "no longer fully initialized" so its scope-exit
/// Drop, guarded by `elaborate_scope_drops`, is skipped). Idempotent: a
/// local moved-from more than once (e.g. two different fields) reuses the
/// same flag and only needs to be cleared once for the *whole-struct* drop
/// guard's purposes -- the flag models "should the parent's Drop impl run
/// at all," not per-field state (fine-grained per-field drop guards are a
/// natural v2 if a type has multiple droppable fields moved independently;
/// today this conservatively disables the parent's Drop entirely on any
/// partial move, which is always sound, just not maximally precise).
pub(crate) fn register_partial_move(&mut self, local: LocalIdx, span: Span) {
    let flag = *self.drop_flags.entry(local).or_insert_with(|| {
        let f = self.alloc_local(self.ctx.ty_ctx().bool_ty(), Mutability::Mut, span);
        // Flags default to `true` (fully initialized) until the first move.
        f
    });
    self.push_stmt(
        glyim_mir::StatementKind::Assign(
            glyim_mir::Place::new(flag),
            glyim_mir::Rvalue::Use(glyim_mir::Operand::Constant(glyim_mir::MirConst {
                kind: glyim_mir::MirConstKind::Bool(false),
                ty: self.ctx.ty_ctx().bool_ty(),
                span,
            })),
        ),
        span,
    );
}
```

**Step 4c — initialize each flag to `true` at the point its backing local is
declared** (in `lower_body`, right after `StorageLive` for locals whose type
`needs_drop`), so the flag's default state before any move is "drop the
whole thing":

```rust
// In MirBuilder::lower_body, right after allocating a local that needs_drop:
if self.needs_drop(local_ty) {
    // Pre-allocate the flag now (rather than lazily in register_partial_move)
    // so it's initialized to `true` unconditionally, even on paths that never
    // move from this local.
    let flag = self.alloc_local(self.ctx.ty_ctx().bool_ty(), Mutability::Mut, span);
    self.push_stmt(
        glyim_mir::StatementKind::Assign(
            glyim_mir::Place::new(flag),
            glyim_mir::Rvalue::Use(glyim_mir::Operand::Constant(glyim_mir::MirConst {
                kind: glyim_mir::MirConstKind::Bool(true),
                ty: self.ctx.ty_ctx().bool_ty(),
                span,
            })),
        ),
        span,
    );
    self.drop_flags.insert(local, flag);
}
```

   (Move the flag-lookup-or-insert logic out of `register_partial_move` once
   this pre-allocation is in place — `register_partial_move` then only needs
   to look up the pre-existing flag and set it to `false`.)

**Step 4d — cross-check against `glyim-borrowck`'s independent move
analysis.** The report notes `glyim-borrowck/src/move_analysis.rs` already
tracks per-field move paths for borrow-checking purposes, separately from
lowering. After 4a-4c land, borrowck's view and lowering's view of "what got
moved" must agree, or borrowck will accept/reject programs lowering handles
differently. Add a debug-assertion pass that walks both and diffs them on
the test suite before treating this phase as done.

### Tests

```
struct Pair { a: String, b: String }  // String is not Copy
fn consume(s: String) -> usize { s.len() }
fn f() -> usize {
    let p = Pair { a: "hi".into(), b: "bye".into() };
    let n = consume(p.a);   // partial move of p.a
    n + p.b.len()           // p.b still valid; p itself must NOT double-drop
}
```
Run under a leak/double-free checker (ASan, or a `DropCounter` type
recording call count) — must show exactly one drop of `p.b`'s `String`
backing allocation and zero double-drops of `p.a`'s (already moved into
`consume`).

---

## Phase 5 — `Deref` autoderef for ADTs (P1)

### Root cause

`TyCtx::deref_ty` (`glyim-type`, ~L64076) only handles the structural cases:

```rust
pub fn deref_ty(&self, ty: Ty) -> Option<Ty> {
    match self.ty_kind(ty) {
        TyKind::Ref(_, inner, _) => Some(*inner),
        TyKind::RawPtr(inner, _) => Some(*inner),
        _ => None,
    }
}
```

`resolve_method_call` in `glyim-typeck/src/check_expr.rs` (~L1236) already
loops `deref_ty` up to 10 times and already has the impl-matching machinery
(`collect_for` closure, ~L1264) that a `Deref`-trait lookup needs — it just
needs `deref_ty` to also try resolving via a registered `impl Deref for X`.

### Fix

`deref_ty` currently lives on `TyCtx` with no access to the HIR (it's a
pure type-level query), but resolving `impl Deref for Box<T> { type Target
= T; }` requires an impl lookup, which today only exists inside
`check_expr.rs`'s typeck context. Two options, in order of preference:

**Option A (preferred): make deref-impl lookup a `TyCtx`-level registry**,
populated once during typeck setup (same pattern already used for
`auto_trait_registry` — see `has_negative_impl`/`has_manual_impl` right next
to `deref_ty` in the same file, ~L64080-64090). This keeps `deref_ty` a
cheap synchronous query instead of a HIR walk on every autoderef step:

```rust
// glyim-type: alongside `auto_trait_registry`, add:
pub struct DerefRegistry {
    /// Self type -> Target type, populated from `impl Deref for X { type
    /// Target = Y; }` items during typeck's impl-collection pass.
    targets: HashMap<Ty, Ty>,
}

impl TyCtx {
    pub fn register_deref_impl(&mut self, self_ty: Ty, target_ty: Ty) {
        self.deref_registry.targets.insert(self_ty, target_ty);
    }

    pub fn deref_ty(&self, ty: Ty) -> Option<Ty> {
        match self.ty_kind(ty) {
            TyKind::Ref(_, inner, _) => Some(*inner),
            TyKind::RawPtr(inner, _) => Some(*inner),
            TyKind::Adt(..) => self.deref_registry.targets.get(&ty).copied(),
            _ => None,
        }
    }
}
```

Populate `deref_registry` during the same HIR-item-scanning pass typeck
already runs to build `auto_trait_registry` / impl tables (find it —
probably in `glyim-typeck/src/coherence.rs` given the file's name, or
wherever `has_manual_impl`'s registry gets built) — add a branch: when an
`ItemKind::Impl` has `trait_ref` naming `Deref`, resolve its
`associated_types` entry for `Target` and call `register_deref_impl`.

**Option B (fallback if the registry can't be threaded through in time):**
keep `deref_ty` structural-only, and instead extend
`resolve_method_call`'s existing HIR-scanning loop
(`check_expr.rs` ~L1264 `collect_for`) to *also* check, at each autoderef
step, whether an `impl Deref for <step_ty>` item exists in `hir.items` and
compute its target inline. This duplicates work across every method call
rather than caching it once, so Option A is strongly preferred for anything
beyond a handful of `Deref` impls in a program (every `Box`/`Rc`/`Vec` use
triggers this).

Either way, standard-library `Box<T>`/`Rc<T>`/`Vec<T>` (in
`glyim-lang-alloc/lib/boxed.g`, `rc.g`, `vec.g`) need their `impl Deref`
blocks checked to make sure they're written the way the registry-population
pass expects to find them (associated `type Target = T;`, not some other
encoding).

### Tests

```
let b: Box<Vec<i32>> = Box::new(vec![1,2,3]);
b.push(4);        // push is defined on Vec<T>, reached only via Box's Deref
assert_eq(b.len(), 4);
```

---

## Phase 6 — Range-const materialization (P2, small)

`glyim-pipeline/src/mono_cache.rs::cv_const` (~L48027-48100) already handles
every `ConstValue` variant except `Range`:

```rust
ConstValue::Range(..) => return None,  // falls back to zero-init ConstRef
```

`MirConstKind` needs a `Range { start: Box<MirConst>, end: Box<MirConst>,
inclusive: bool }` variant (or equivalent — check what shape
`glyim_const_eval::ConstValue::Range` already carries and mirror it), and
`cv_const` gets one more arm:

```rust
ConstValue::Range(start, end, inclusive) => {
    let (start_ty, end_ty) = /* Range<T>'s T from `ty`'s TyKind::Adt substs */;
    MirConstKind::Range {
        start: Box::new(MirConst { kind: self.cv_const(start, start_ty)?, ty: start_ty, span: Span::DUMMY }),
        end: Box::new(MirConst { kind: self.cv_const(end, end_ty)?, ty: end_ty, span: Span::DUMMY }),
        inclusive: *inclusive,
    }
}
```

The LLVM backend (`glyim-codegen-llvm/src/lower.rs`) needs a matching arm
wherever it already switches over `MirConstKind` to build an
`AggregateKind`-style struct literal (`Range<T>` is just a 2-field struct
`{ start: T, end: T }` at the ABI level) — model it on however
`MirConstKind::Aggregate` is already lowered there.

### Tests
`const R: Range<i32> = 0..10; let n = (R.end - R.start);` must fold to `10`
at compile time with no `ConstRef` global emitted (check the emitted LLVM
IR / MIR for absence of `__glyim_const_*`).

---

## Phase 7 — Windows SEH funclets (P2, toolchain FFI)

### Root cause

Confirmed real, and it's a toolchain-API gap, not a logic bug: the pinned
LLVM 22 build's `inkwell`/`llvm-sys` version doesn't export the funclet
C-API (`LLVMBuildCleanupPad`, `LLVMBuildCleanupRet`, `LLVMBuildCatchSwitch`,
`LLVMBuildCatchPad`, `LLVMBuildCatchRet`). Both personalities currently
share the Itanium `landingpad`/`resume` lowering in
`glyim-codegen-llvm/src/lower.rs::emit_landingpad` (~L2909).

### Fix

These functions **are** part of LLVM-C's stable `llvm-c/Core.h` (present
since LLVM 8) — `llvm-sys`/`inkwell` simply don't *bind* them at the pinned
version. The fix is raw FFI, not an LLVM upgrade (upgrading is the
alternative if raw FFI proves unworkable, but try FFI first — it's much
less disruptive):

```rust
// glyim-codegen-llvm/src/seh_ffi.rs (new file)
//
// Manual bindings for the LLVM-C funclet API, not exposed by the pinned
// inkwell/llvm-sys version. Signatures copied verbatim from llvm-c/Core.h
// for the LLVM version this crate links against -- verify against the
// actual installed `llvm-c/Core.h` before trusting these signatures blindly.

use llvm_sys::prelude::{LLVMBuilderRef, LLVMValueRef, LLVMBasicBlockRef, LLVMTypeRef};
use std::os::raw::{c_char, c_uint};

unsafe extern "C" {
    pub fn LLVMBuildCleanupPad(
        B: LLVMBuilderRef,
        ParentPad: LLVMValueRef,
        Args: *mut LLVMValueRef,
        NumArgs: c_uint,
        Name: *const c_char,
    ) -> LLVMValueRef;

    pub fn LLVMBuildCleanupRet(
        B: LLVMBuilderRef,
        CatchPad: LLVMValueRef,
        BB: LLVMBasicBlockRef,
    ) -> LLVMValueRef;

    pub fn LLVMBuildCatchSwitch(
        B: LLVMBuilderRef,
        ParentPad: LLVMValueRef,
        UnwindBB: LLVMBasicBlockRef,
        NumHandlers: c_uint,
        Name: *const c_char,
    ) -> LLVMValueRef;

    pub fn LLVMAddHandler(CatchSwitch: LLVMValueRef, Dest: LLVMBasicBlockRef);

    pub fn LLVMBuildCatchPad(
        B: LLVMBuilderRef,
        ParentPad: LLVMValueRef,
        Args: *mut LLVMValueRef,
        NumArgs: c_uint,
        Name: *const c_char,
    ) -> LLVMValueRef;

    pub fn LLVMBuildCatchRet(
        B: LLVMBuilderRef,
        CatchPad: LLVMValueRef,
        BB: LLVMBasicBlockRef,
    ) -> LLVMValueRef;
}
```

Add `llvm-sys` as a **direct** dependency of `glyim-codegen-llvm` (it's
already a transitive dependency via `inkwell` — check `Cargo.toml`, it may
already be available under a different feature set) so these raw symbols
resolve against the exact same linked LLVM library inkwell uses — do not
link a second copy of LLVM.

Then, in `emit_landingpad`, branch on `Personality::Seh` and use these
instead of `build_landing_pad`:

```rust
// glyim-codegen-llvm/src/lower.rs

fn emit_landingpad(&mut self) -> CompResult<()> {
    if !self.has_cleanup {
        self.current_landingpad = None;
        return Ok(());
    }
    match self.personality {
        Personality::Seh => self.emit_seh_cleanuppad(),
        Personality::Itanium => self.emit_itanium_landingpad(), // existing code, renamed
        Personality::None => Ok(()),
    }
}

fn emit_seh_cleanuppad(&mut self) -> CompResult<()> {
    use crate::seh_ffi::{LLVMBuildCleanupPad, LLVMBuildCleanupRet};
    unsafe {
        // A top-level cleanuppad has no parent pad (null token). Nested
        // cleanups within a cleanup (e.g. a destructor that itself panics --
        // deliberately unsupported/aborts, matching what most funclet-based
        // unwinders do for double-panics) pass the enclosing pad instead.
        let raw_builder = self.builder.as_mut_ptr(); // inkwell exposes this
        let cleanuppad = LLVMBuildCleanupPad(
            raw_builder,
            std::ptr::null_mut(), // no parent pad
            std::ptr::null_mut(),
            0,
            c"cleanuppad".as_ptr(),
        );
        if cleanuppad.is_null() {
            return Err(vec![GlyimDiagnostic::internal_error(
                "LLVMBuildCleanupPad returned null -- SEH funclet emission failed",
            )]);
        }
        self.current_seh_pad = Some(cleanuppad);
        // The corresponding LLVMBuildCleanupRet(builder, cleanuppad, target_bb)
        // is emitted wherever the existing Itanium path currently emits
        // `resume` -- same call site, branch on self.personality there too.
    }
    Ok(())
}
```

`self.builder.as_mut_ptr()` (inkwell's `Builder` exposes the raw
`LLVMBuilderRef` for exactly this kind of escape hatch — check the inkwell
version in `Cargo.toml` for the exact accessor name, it may be
`.as_mut_ptr()` or require `unsafe { std::mem::transmute(...) }` on older
inkwell releases). Wrap every raw call in a narrow, well-documented `unsafe`
block; do not scatter raw FFI calls throughout `lower.rs` — keep them behind
the `seh_ffi` module's safe(r) wrapper functions.

**Validate against a real MSVC target before declaring this done** — the
existing test `seh_target_lowers_cleanup_landingpad_green` (~L3317) only
checks that Itanium-shaped IR is emitted for SEH targets and explicitly
documents this as the known gap; once funclets are wired, that test's
assertions (`ir.contains("landingpad") && ir.contains("resume")`) need to
flip to asserting `cleanuppad`/`cleanupret` instead, and the test should be
renamed to drop "green" (which currently signals "accepted approximation").
Cross-linking the resulting object against a real MSVC CRT (or `lld-link`
+ Windows unwind tables) in CI is the only way to be confident this
actually unwinds correctly — LLVM IR text inspection alone can't catch a
malformed funclet token chain.

---

## Phase 8 — Proc-macro build orchestration (P2)

### Root cause

Narrower than the report suggests. Already fully working:
- `--emit=cdylib` codegen (`glyim-cli/src/main.rs` ~L3745-3977)
- `dlopen`/`LoadLibraryW` loader + in-process `Registry`
  (`glyim-proc-macro/src/lib.rs`, `load_cdylib` ~L296)
- Pipeline wiring: `with_proc_registry` accepts an external `Registry`
  (`glyim-hir/src/pipeline_api.rs` ~L41762, `glyim-pipeline` ~L55993)

What's missing is purely **`glyim-cli` driver orchestration**: given a crate
graph where crate `B` depends on proc-macro crate `A`, nothing currently:
1. detects `A` is a proc-macro crate (needs a manifest flag, e.g.
   `[package] proc-macro = true`, mirrored from Cargo's convention — check
   whether glyim has its own manifest format or piggybacks on `Cargo.toml`),
2. compiles `A` for the **host** triple (not the target triple — critical
   for cross-compilation) with `--emit=cdylib`,
3. calls `glyim_proc_macro::load_cdylib` on the resulting artifact,
4. passes the resulting `Registry` into `B`'s compilation via
   `with_proc_registry`.

### Fix

```rust
// glyim-cli/src/main.rs (new function, called from the main compile driver
// before compiling the crate that *uses* proc macros)

fn build_proc_macro_dependencies(
    crate_graph: &CrateGraph,   // whatever glyim's existing multi-crate model is
    host_triple: &str,
    build_dir: &Path,
) -> Result<glyim_proc_macro::Registry, Vec<GlyimDiagnostic>> {
    let mut registry = glyim_proc_macro::Registry::new();

    for dep in crate_graph.proc_macro_dependencies() {
        // 1. Compile `dep` for the HOST target (not `--target` the user
        //    passed), always with optimizations off/minimal for build speed,
        //    emitting a cdylib. Reuses the exact same `compile_file_with_artifacts`
        //    path as a normal build, just with target_triple overridden to
        //    `host_triple` and `emit = EmitKind::Cdylib`.
        let cdylib_path = build_dir.join(format!("lib{}_pm.{}", dep.name, cdylib_ext(host_triple)));
        compile_crate_for_target(dep, host_triple, EmitKind::Cdylib, &cdylib_path)?;

        // 2. Load it and merge its macros into the shared registry.
        let loaded = glyim_proc_macro::load_cdylib(cdylib_path.to_str().unwrap())
            .map_err(|e| vec![GlyimDiagnostic::internal_error(&format!(
                "failed to load proc-macro crate `{}`: {}", dep.name, e
            ))])?;
        registry.merge(loaded.registry); // add a `merge` method to Registry if absent
    }

    Ok(registry)
}
```

Wire this in wherever `main.rs` currently builds the `Pipeline` /
`LowerCtx` for the primary crate — call `build_proc_macro_dependencies`
first (if the crate graph has any proc-macro deps), then thread the
resulting `Registry` through `.with_proc_registry(Some(&registry))`.

`cdylib_ext(host_triple)` — reuse whatever logic `--emit=cdylib`'s existing
object-naming code already has for choosing `.so`/`.dylib`/`.dll`; don't
duplicate it.

**Caching:** proc-macro crates rarely change between builds of their
dependents — worth a content-hash cache (`build_dir/pm-cache/<hash>.so`) so
incremental builds don't recompile the proc-macro crate every time. Not
required for correctness; add only after the basic path works and is
tested.

### Tests

A `#[derive(MyDerive)]`-style proc macro crate + a consumer crate using it
end-to-end through `glyim-cli` (not just the in-process `Registry` unit
tests that already exist) — this is the one thing that was never actually
exercised, since everything downstream of `load_cdylib` was already tested
in isolation.

---

## Phase 9 — Inclusive range slicing `..=` (P3, small)

`glyim-lower/src/lower_rvalue.rs`'s `ExprKind::Index` range-slicing path
(~L629 onward, the `start`/`end`/`inclusive` destructuring around L1600-1780)
already threads an `inclusive: bool` field through — check whether it's
already read and just not acted on, or genuinely dropped. Based on the
comment location, the fix is almost certainly: wherever `end` is used
directly as the slice's upper bound, add 1 when `inclusive` is true before
the existing bounds-check logic (~L1667-1680, the `end_le_len` check) runs:

```rust
let effective_end = if *inclusive {
    // `a..=b` slices through index b inclusive, i.e. length end is b+1.
    // Must still bounds-check b+1 <= len (not b <= len) to catch b == len-0
    // off-by-one at the boundary, and must check for overflow if b == usize::MAX.
    self.build_checked_add_one(end_local, expr.span)  // new small helper
} else {
    end_local
};
```

Add a checked-add (not wrapping) since `b == usize::MAX` with `..=` is a
real overflow case that must panic like Rust's own bounds-check panics do,
not silently wrap to `0`.

### Tests
`let v = [10,20,30,40]; let s = &v[1..=2]; assert_eq(s, &[20,30]);` plus an
out-of-bounds `..=` case that must panic, not read past the end.

---

## Suggested execution order for an agent

Do the phases in this order — each is written to be independently
compilable/testable, and later phases don't depend on earlier ones except
where noted:

1. Phase 0 (harness) — always first.
2. Phase 1 (for-loops) — highest real-world impact, isolated to one trait impl.
3. Phase 2 (array drop glue / subst_ty) — touches core `glyim-type`, do it
   early while the codebase is least churned, since many other things
   depend on `subst_ty` being correct.
4. Phase 4 (partial moves) — small, mechanical, high safety value.
5. Phase 5 (Deref autoderef) — independent of the above.
6. Phase 6 (range consts) — small, independent.
7. Phase 9 (inclusive range slicing) — small, independent.
8. Phase 3 (async state machine) — largest single item, do it once the
   codebase is stable from the smaller fixes, since it's the one most
   likely to need iteration.
9. Phase 7 (SEH) — requires access to a Windows/MSVC test target; sequence
   it whenever that environment is available, independent of everything else.
10. Phase 8 (proc-macro orchestration) — independent, do last since it's
    pure driver plumbing with no interaction with the other phases.

After every phase: run the **full** existing test suite (not just the new
regression fixture), since several of these fixes (`subst_ty` especially)
touch shared infrastructure with wide blast radius.
