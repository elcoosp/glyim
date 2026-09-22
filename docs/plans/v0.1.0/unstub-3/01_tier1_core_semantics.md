## TIER 1 — Missing core semantics

### 1.1 Closure capture analysis — `glyim-typeck/src/check_expr.rs`

**Current (line 963):**
```rust
fn analyze_captures(
    &mut self,
    _body_expr: ExprId,
    _span: Span,
) -> Vec<(thir::LocalVarId, thir::CaptureKind, Ty)> {
    // Placeholder: walk the body and collect free variables.
    // For now, return empty.
    Vec::new()
}
```
And the caller (`Expr::Closure` arm, line ~725) never binds closure params
into scope before calling this — so even once implemented, params would be
misclassified as unresolved names. Both must be fixed together.

**Grounding (already confirmed in your source):**
- `self.env: LocalEnv` (`glyim-typeck/src/env.rs`) has `enter_scope()`,
  `leave_scope()`, `add_binding(name, ty, mutability) -> LocalVarId`,
  `lookup_by_name(name) -> Option<&LocalVarInfo>`.
- `LocalVarId`s are allocated sequentially and **never reused**
  (`env.rs` doc comment). This means: snapshot the id boundary *before*
  entering the closure's scope; after checking the body, any `VarRef`
  whose id is `< boundary` is a capture from an outer scope, and any
  `VarRef` with id `>= boundary` is the closure's own param/let-binding —
  **no separate free-variable walk is needed**, just record every
  `VarRef` id/type resolved while type-checking the body and filter by
  the boundary afterward.
- `check_path` (`glyim-typeck/src/unify.rs` line 56) is the single place
  `Expr::Path` resolves to a `thir::ExprKind::VarRef(var_info.id)` — hook
  capture recording there.
- `thir::Capture { local: LocalVarId, kind: CaptureKind, ty: Ty }` and
  `enum CaptureKind` already exist in `glyim-typeck/src/thir.rs` (line
  213/220) — read that enum's variants before writing the classification
  logic below and match the exact variant names.
- Closure params are `Vec<PatId>` (HIR), not `Vec<(Name, TypeRef)>` — you
  need whatever pattern-binding helper the rest of the checker uses for
  `let` statements (search `check_stmt.rs` for how `PatId` patterns bind
  names into `self.env`; reuse that exact function for closure params,
  don't write a second binder).

**Fix — add a capture-recording buffer to `FnCtxt`:**

In `glyim-typeck/src/check_body.rs`, find `struct FnCtxt` and add:
```rust
pub(crate) capture_log: Vec<(thir::LocalVarId, Ty, bool /* is_mut_use */)>,
```
initialized empty wherever `FnCtxt` is constructed.

**In `unify.rs::check_path`,** after resolving `var_info`, push to the log:
```rust
if let Some(var_info) = self.env.lookup_by_name(name) {
    self.capture_log.push((var_info.id, var_info.ty, false));
    let thir_expr = thir::Expr { kind: thir::ExprKind::VarRef(var_info.id), ty: var_info.ty, span };
    return (thir_expr, var_info.ty);
}
```
`is_mut_use` starts `false` here; it gets upgraded to `true` at the two
places that already know a place is used mutably — the `Expr::Assign` arm
(lhs) and the `Expr::Ref { mutability: Mutability::Mut, .. }` arm, both in
`check_expr.rs`. In each, after calling `self.check_expr(lhs_or_inner)`,
if the checked sub-expression's thir kind is `VarRef(id)`, mark the most
recent matching entry in `capture_log` as a mut use:
```rust
if let thir::ExprKind::VarRef(id) = lhs_expr.kind {
    if let Some(entry) = self.capture_log.iter_mut().rev().find(|(vid, ..)| *vid == id) {
        entry.2 = true;
    }
}
```

**Rewrite `analyze_captures`:**
```rust
fn analyze_captures(
    &mut self,
    body_expr: ExprId,
    boundary: thir::LocalVarId, // id count *before* closure params were bound
) -> Vec<(thir::LocalVarId, thir::CaptureKind, Ty)> {
    let log_start = self.capture_log.len();
    let (checked_body, _body_ty) = self.check_expr(body_expr);
    let mut seen = std::collections::HashSet::new();
    let mut captures = Vec::new();
    for (id, ty, is_mut) in self.capture_log.drain(log_start..) {
        if id.to_raw() >= boundary.to_raw() {
            continue; // bound inside the closure itself — not a capture
        }
        if !seen.insert(id) {
            continue; // already recorded (e.g. used twice) — keep first classification
        }
        let kind = if is_mut { thir::CaptureKind::ByMutRef } else { thir::CaptureKind::ByRef };
        captures.push((id, kind, ty));
    }
    self.stash_checked_body(body_expr, checked_body); // see note below
    captures
}
```
Confirm the exact `CaptureKind` variant names in `thir.rs` (this plan
assumes `ByRef`/`ByMutRef`/possibly `ByValue`; if the enum only has two
variants use those; if it has three including `ByValue` for `move`
closures, thread a `is_move: bool` param through from the `Closure` HIR
node — check whether `hir::Expr::Closure` carries a `move` flag; if it
doesn't today, that's a separate small HIR addition, call it out but don't
block this fix on it, default every capture to `ByRef`/`ByMutRef` for now).

**`stash_checked_body` note:** the current `Expr::Closure` arm calls
`self.check_expr(*body)` a *second* time after `analyze_captures` already
checked it once (wasteful, and non-idempotent if checking has side effects
like diagnostics — it would double-emit errors from the closure body).
Rewrite the whole arm:
```rust
Expr::Closure { params, body } => {
    self.env.enter_scope();
    let boundary = self.env.next_var_id(); // ADD this accessor to LocalEnv, see below
    let mut param_tys = Vec::with_capacity(params.len());
    for pat_id in params {
        let ty = self.fresh_infer_ty();
        self.bind_pattern(*pat_id, ty, Mutability::Not); // reuse the same helper check_stmt.rs uses for `let`
        param_tys.push(ty);
    }
    let (body_expr, body_ty) = self.check_expr(*body);
    let captures = self.analyze_captures_from_log(boundary); // drains capture_log, no second check_expr call
    self.env.leave_scope();

    let closure_ty = self.fresh_infer_ty(); // still a fresh var; see 1.1b below for making this real
    let capture_thir: Vec<thir::Capture> = captures
        .into_iter()
        .map(|(local, kind, ty)| thir::Capture { local, kind, ty })
        .collect();
    let closure_expr = thir::Expr {
        kind: thir::ExprKind::Closure {
            body: Box::new(thir::Body {
                owner: self.owner,
                params: param_tys.iter().map(|_| /* build thir::Param per your Body::params shape */).collect(),
                return_ty: body_ty,
                stmts: vec![thir::Stmt::Expr { expr: body_expr }],
                span,
            }),
            captures: capture_thir,
        },
        ty: closure_ty,
        span,
    };
    (closure_expr, closure_ty)
}
```
Split `analyze_captures` into `analyze_captures_from_log(boundary)` (just
the drain/filter logic above, no `check_expr` call inside it) since the
body is now checked exactly once, before capture extraction.

**Add to `LocalEnv` (`glyim-typeck/src/env.rs`):**
```rust
#[inline]
pub fn next_var_id(&self) -> LocalVarId {
    LocalVarId::from_raw(self.vars.len() as u32)
}
```

**Verify:** add a typeck test: `let x = 1; let f = |y| x + y;` — assert the
resulting closure's THIR has exactly one capture, for local `x`, kind
`ByRef`. Add a second test with `let mut x = 1; let f = || { x += 1; };` —
assert kind `ByMutRef`. Add a third with a nested closure
(`let f = |a| { let g = |b| a + b; g(1) };`) — assert the *inner* closure's
own param `b` is not captured, and `a` is (this is exactly what the
boundary-id filtering is for; if this test fails, the boundary snapshot is
being taken at the wrong point).

---

### 1.1b Closure type is still `fresh_infer_ty()`, never resolved

The report ("check_expr for Expr::Closure ... does not actually build a
closure type") is accurate beyond just captures: `closure_ty` above is an
unconstrained inference variable forever — nothing ever unifies it with a
concrete closure/`Fn`-trait type, so it will report as an unresolved infer
var at the end of type checking for any closure that isn't immediately
called.

This needs a real closure ADT, analogous to how `register_builtin_ranges`
in `glyim-type/src/ty_ctx_mut.rs` registers compiler-internal ADTs. Add,
next to `register_builtin_ranges`, a per-closure-expression registration:

```rust
// glyim-type/src/ty_ctx_mut.rs
pub fn register_closure(&mut self, capture_tys: Vec<Ty>) -> AdtId {
    use crate::adt_def::{AdtDef, AdtKind, FieldDef, VariantDef};
    let id = self.next_synthetic_adt_id(); // add a counter starting above 1005, the range/UnsafeCell block
    let mut field_defs = glyim_core::arena::IndexVec::new();
    for (i, ty) in capture_tys.iter().enumerate() {
        field_defs.push(FieldDef { name: self.resolver.intern(&format!("capture_{i}")), ty: *ty });
    }
    let def = AdtDef {
        kind: AdtKind::Struct,
        fields: field_defs.clone(),
        variants: vec![VariantDef { name: self.resolver.intern(""), fields: field_defs }],
    };
    self.register_adt(id, def);
    id
}
```
Add `next_synthetic_adt_id(&mut self) -> AdtId` backed by a
`synthetic_adt_counter: u32` field on `TyCtxMut`, seeded to e.g. `2_000_000`
so it can never collide with real user ADT ids or the 1000-1005 builtins.

Then in `check_expr.rs`'s `Expr::Closure` arm, after captures are known,
replace `let closure_ty = self.fresh_infer_ty();` with:
```rust
let capture_tys: Vec<Ty> = captures.iter().map(|(_, _, ty)| *ty).collect();
let closure_adt = self.ctx.register_closure(capture_tys.clone()); // self.ctx must be &mut TyCtxMut here
let closure_substs = self.ctx.intern_substitution(vec![]); // closures are monomorphic structs, no substs needed
let closure_ty = self.ctx.mk_adt(closure_adt, closure_substs);
```
This makes the closure a genuine nominal type usable downstream by
`AggregateKind::Closure(*closure_id, *closure_substs)` in
`glyim-lower/src/lower_rvalue.rs` (line ~687, already present — it
currently receives a `closure_id`/`closure_substs` pair sourced from HIR,
confirm that plumbing expects an `AdtId`-shaped id and adjust if it
expects a distinct `ClosureId` newtype instead; if so, wrap `AdtId` rather
than inventing a second id space).

**Verify:** typeck test — a closure's inferred type should now resolve to
`TyKind::Adt(closure_adt_id, _)`, not stay `TyKind::Infer(..)` after
`typeck_crate` finishes.

---

### 1.2 VTable generation — `glyim-layout/src/vtable.rs` + `glyim-layout/src/lib.rs` (`SimpleLayoutComputer::vtable_of`)

**Current (`lib.rs` ~line 745):**
```rust
impl crate::vtable::VTableComputer for SimpleLayoutComputer<'_> {
    fn vtable_of(&self, trait_def_id: TraitDefId, concrete_ty: Ty) -> Option<VTableLayout> {
        let concrete_layout = self.layout_of(concrete_ty).ok()?;
        Some(VTableLayout { trait_def_id, concrete_ty, size: concrete_layout.size,
            align: concrete_layout.align, drop_fn: None, methods: vec![] })
    }
}
```

**Root cause (confirmed, not guessed):** `SimpleLayoutComputer` only holds
`ctx: &TyCtx` and `target: TargetInfo` — it has **no access to trait/impl
data**, which lives in `glyim_solve::solver::TraitContext` (`impl_defs:
Vec<ImplDef>`, `impls_of_trait(trait_id)`). Worse: `ImplDef` itself
(`glyim-solve/src/solver.rs` line ~50) currently only stores
`{ def_id, trait_ref, predicates }` — **it does not record which concrete
`FnDefId` implements which trait method**. That mapping must exist before
any vtable can be built. This is a real schema gap, not just a missing
loop.

**This cannot be fixed inside `glyim-layout` alone** — `glyim-layout` must
not depend on `glyim-solve` (that would invert the crate DAG: solve already
depends on layout/type). The fix has three parts, in order:

**1.2.a — Add method-impl records to `ImplDef` (`glyim-solve/src/solver.rs`):**
```rust
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImplDef {
    pub def_id: ImplDefId,
    pub trait_ref: TraitRef,
    pub predicates: Vec<Predicate>,
    pub items: Vec<(Name, glyim_core::def_id::FnDefId)>, // NEW: method name -> concrete fn
}
```
Find every place `ImplDef { .. }` is constructed (grep `ImplDef {` across
the workspace — expect hits in `glyim-typeck/src/coherence.rs` and/or
wherever HIR `ItemKind::Impl` gets lowered into a registered `ImplDef`,
likely `glyim-pipeline` or `glyim-typeck/src/lib.rs`'s "coherence
registration" step the report already flags separately, see 1.2.d) and
populate `items` from `impl_item.methods` (the same list `resolve_method_call`
in `check_expr.rs` line 883 already iterates for name lookup — reuse that
iteration, don't write a second one).

**1.2.b — Give `VTableComputer::vtable_of` the data it needs.** Change the
trait signature in `glyim-layout/src/vtable.rs` to accept the resolved
method list instead of looking anything up itself — `glyim-layout` stays
ignorant of trait solving, the caller (which does have `TraitContext`)
supplies the answer:
```rust
pub trait VTableComputer {
    fn vtable_of(
        &self,
        trait_def_id: TraitDefId,
        concrete_ty: Ty,
        methods: &[(Name, FnDefId, FnSig)], // NEW: resolved (name, impl fn, signature) triples, in trait-declaration order
        drop_fn: Option<FnDefId>,           // NEW: resolved drop glue for concrete_ty, if any
    ) -> Option<VTableLayout>;
}
```
```rust
impl crate::vtable::VTableComputer for SimpleLayoutComputer<'_> {
    fn vtable_of(
        &self,
        trait_def_id: TraitDefId,
        concrete_ty: Ty,
        methods: &[(Name, FnDefId, FnSig)],
        drop_fn: Option<FnDefId>,
    ) -> Option<VTableLayout> {
        let concrete_layout = self.layout_of(concrete_ty).ok()?;
        let vtable_methods = methods
            .iter()
            .map(|(name, fn_def_id, sig)| crate::vtable::VTableEntry {
                name: *name,
                sig: sig.clone(),
                fn_def_id: *fn_def_id,
            })
            .collect();
        Some(VTableLayout {
            trait_def_id,
            concrete_ty,
            size: concrete_layout.size,
            align: concrete_layout.align,
            drop_fn,
            methods: vtable_methods,
        })
    }
}
```

**1.2.c — Wire it up at the one real call site: `glyim-pipeline`.** This is
where `TraitContext` (trait solving), `TyCtx` (layout), and `mono_cache`
(drop glue, per the report's `generate_drop_glue`) are all simultaneously
available. Add a function there, e.g. in `glyim-pipeline/src/mono_cache.rs`
next to `generate_drop_glue`:
```rust
pub fn build_vtable(
    trait_ctx: &TraitContext,
    ty_ctx: &TyCtx,
    layout_computer: &impl VTableComputer,
    trait_def_id: TraitDefId,
    concrete_ty: Ty,
    drop_fn: Option<FnDefId>,
) -> Option<VTableLayout> {
    let trait_def = trait_ctx.trait_defs().iter().find(|t| t.def_id == trait_def_id)?; // trait_defs() is currently #[cfg(test)]-only, see note below
    let impl_def = trait_ctx
        .impls_of_trait(trait_def_id)
        .find(|imp| /* imp.trait_ref applies to concrete_ty — reuse whatever substitution-match helper coherence.rs uses */)?;
    let methods: Vec<_> = impl_def.items.iter()
        .filter_map(|(name, fn_def_id)| {
            let sig = ty_ctx.fn_sig(*fn_def_id)?.clone();
            Some((*name, *fn_def_id, sig))
        })
        .collect();
    layout_computer.vtable_of(trait_def_id, concrete_ty, &methods, drop_fn)
}
```
Note: `TraitContext::trait_defs()`/`impl_defs()` are currently
`#[cfg(test)]`-only (see `glyim-solve/src/solver.rs`) — remove that cfg
gate (or add a non-test public accessor) since production code now needs
them, not just tests.

Call `build_vtable` from wherever `glyim-codegen-llvm`/`glyim-codegen`
currently constructs a trait-object value (search for where `dyn Trait`
coercion or unsized-coercion codegen happens — likely in
`lower.rs`'s handling of a cast/coercion to a `TyKind::Dynamic`; the report
lists `object_safety.rs`/`display.rs`'s `TyKind::Dynamic` handling as the
other end of this, see item 2.3), and cache the resulting `VTableLayout`
per `(trait_def_id, concrete_ty)` in `mono_cache` alongside drop glue so
it's only built once per monomorphized pair.

**1.2.d — Also fixes report item "glyim-typeck coherence.rs — impls
checked but not stored globally."** Once `ImplDef.items` exists and impls
are registered into a real `TraitContext` that outlives typeck (owned by
the pipeline, not local to `coherence.rs`), that report item is resolved
as a side effect — confirm `glyim-typeck/src/lib.rs`'s `typeck_crate`
returns/passes through the populated `TraitContext` (or a list of
`ImplDef`s) to its caller instead of dropping it, and that
`glyim-pipeline` is the one constructing the long-lived `TraitContext` and
feeding per-crate impls into it.

**Verify:** a codegen test that defines a trait with 2 methods, one impl,
casts a concrete value to `dyn Trait`, and asserts the emitted vtable has
2 non-null method-pointer entries (not the current empty `vec![]`) plus a
correct `drop_fn` when the concrete type has fields needing drop.

**glyim-codegen/src/vtable.rs** needs no code change — it's just index
constants (`VTABLE_DROP_FN_INDEX` etc.) and is already correct; it's the
*consumer* of the now-real `VTableLayout` from 1.2.b, wire codegen's
lowering of a `dyn Trait` construction to read `VTableLayout.methods` in
order and place each `fn_def_id`'s codegen'd function pointer at
`VTABLE_METHODS_START + i`, using `VTableLayout::method_offset` (already
correctly implemented, `glyim-layout/src/vtable.rs` line ~65).

---

### 1.3 `Iterator::next` — builtin-only lookup (`glyim-solve/src/solver.rs`)

**Current:** `SimpleTraitSolver::iterator_next_info` only returns `Some` if
`trait_ctx.builtin_next_fn_id` is set; otherwise the whole `for`-loop
lowering path in `glyim-lower` falls back to a simplified model (per the
report, and confirmed: `glyim-lower/src/lower.rs`'s `LowerCtx::iterator_next_fn`
default always returns `None`).

This is **intentionally tiered, not accidental** — `glyim-lang-core/lib/iter.g`
defines `Iterator` as a real trait in the language's own standard library
(`.g` source, not compiler-builtin), so "real" `next` resolution is: find
the concrete type's `impl Iterator for T { fn next(...) }` via the same
`TraitContext.impls_of_trait` + `ImplDef.items` machinery added in 1.2.a,
**not** a special builtin fast-path. The `builtin_next_fn_id` field should
become a *fallback* for the small number of compiler-magic iterator types
(e.g. range iterators, if those are special-cased rather than going through
`.g` impls), not the primary path.

**Fix — rewrite `iterator_next_info`:**
```rust
fn iterator_next_info(
    &self,
    ctx_mut: &mut TyCtxMut,
    iter_ty: Ty,
    elem_ty: Ty,
) -> Option<SolverIteratorNextInfo> {
    // 1. Real path: resolve via the Iterator trait's registered impls.
    let iterator_trait_id = self.iterator_trait_def_id?; // add this field, populated when glyim-lang-core's Iterator trait is registered — mirror however `builtin_next_fn_id` is currently populated in the pipeline bootstrap, same call site, just also stash the trait's DefId
    if let Some(impl_def) = self.trait_ctx.impls_of_trait(iterator_trait_id)
        .find(|imp| trait_ref_applies_to(&imp.trait_ref, iter_ty, ctx_mut)) // reuse the same substitution-match helper as 1.2.c
    {
        if let Some((_, fn_def_id)) = impl_def.items.iter().find(|(name, _)| ctx_mut.name_str(*name) == "next") {
            let fn_sig = ctx_mut.fn_sig(*fn_def_id)?.clone();
            let option_ty = /* build Option<elem_ty> the same way glyim-lang-core/lib/option.g's Option is registered — find its AdtId lookup helper, likely on TyCtx, e.g. ctx_mut.option_adt_id() */;
            return Some(SolverIteratorNextInfo {
                fn_def_id: *fn_def_id,
                fn_substs: /* substitution binding Self=iter_ty for this impl */,
                fn_ty: fn_sig_to_ty(ctx_mut, &fn_sig),
                option_ty,
                discr_ty: /* Option's discriminant type — reuse whatever discriminant lookup mono_cache's drop-glue-for-enums code already does */,
                ref_iter_ty: ctx_mut.mk_ref(Region::ERASED, iter_ty, Mutability::Mut),
            });
        }
    }
    // 2. Fallback: compiler-builtin iterator (e.g. Range iterators) — unchanged existing behavior.
    let fn_def_id = self.builtin_next_fn_id?;
    // ...existing builtin construction logic stays here as the fallback...
}
```
The exact helper names in the `/* ... */` placeholders don't exist yet
under those names — search first (`grep -n "option_adt_id\|fn_sig_to_ty\|Region::ERASED"`
across `glyim-type`); if they don't exist, this is where to add small
`TyCtx`/`TyCtxMut` accessor methods analogous to `adt_def`/`trait_def`,
not to reimplement Option/discriminant lookup ad hoc inside the solver.

**Also fixes:** `glyim-lower/src/lower.rs`'s `LowerCtx::iterator_next_fn`
default becomes reachable-but-correct once the pipeline's real
`PipelineLowerCtx` (mentioned in the report as *not* overriding it) is
updated to call through to this fixed `iterator_next_info` instead of
leaving the trait-default `None` in place. Find `PipelineLowerCtx` in
`glyim-pipeline/src/pipeline_context.rs` and add the override:
```rust
fn iterator_next_fn(&mut self, iter_ty: Ty, elem_ty: Ty) -> Option<SolverIteratorNextInfo> {
    self.trait_solver.iterator_next_info(self.ty_ctx_mut(), iter_ty, elem_ty)
}
```

**Verify:** a `for x in my_custom_iter { ... }` test over a user-defined
`.g`-level `impl Iterator for MyIter` (not a builtin range) lowers to a
`Call` to the user's `next` function, not the simplified fallback model.

---

### 1.4 `Range` expression lowers to an empty dummy tuple — `glyim-lower/src/lower_rvalue.rs`

**Current (line ~88-101):**
```rust
thir::ExprKind::Range { start, end, inclusive } => {
    // For now, we'll treat a range expression as a tuple of (start, end) or something.
    let _start_val = start.as_ref().map(|e| self.lower_expr_to_rvalue(e));
    let _end_val = end.as_ref().map(|e| self.lower_expr_to_rvalue(e));
    let _inclusive = inclusive;
    let operands = Vec::new();
    glyim_mir::Rvalue::Aggregate(glyim_mir::AggregateKind::Tuple, operands)
}
```
**This is a genuine bug** (the values are computed and then dropped!) —
but the fix is smaller than the report implies, because
**`glyim-type/src/ty_ctx_mut.rs::register_builtin_ranges` (line ~444) is
already a complete, correct implementation** registering real `AdtDef`s
for `Range`/`RangeInclusive`/`RangeFrom`/`RangeTo`/`RangeToInclusive` at
fixed `AdtId`s 1000-1004 with real `start`/`end` fields. Don't re-implement
that — just use it here. (The report's characterization of these as
"placeholder ADT IDs...not tied to actual user-defined types" is not a
bug: compiler-internal fixed-id ADTs for builtin types is the correct,
intentional design — no action needed on `ty_ctx_mut.rs` itself.)

**Fix:**
```rust
thir::ExprKind::Range { start, end, inclusive } => {
    let start_val = start.as_ref().map(|e| self.lower_operand(e)); // whatever helper elsewhere in this file turns a thir::Expr into an Operand — search `fn lower_operand` in this same file, reuse it, don't build a new StorageLive/temp dance here
    let end_val = end.as_ref().map(|e| self.lower_operand(e));
    let (adt_id, operands) = match (start_val, end_val, inclusive) {
        (Some(s), Some(e), false) => (glyim_core::def_id::AdtId::from_raw(1000), vec![s, e]),
        (Some(s), Some(e), true)  => (glyim_core::def_id::AdtId::from_raw(1001), vec![s, e]),
        (Some(s), None, _)        => (glyim_core::def_id::AdtId::from_raw(1002), vec![s]),
        (None, Some(e), false)    => (glyim_core::def_id::AdtId::from_raw(1003), vec![e]),
        (None, Some(e), true)     => (glyim_core::def_id::AdtId::from_raw(1004), vec![e]),
        (None, None, _) => unreachable!("full unbounded RangeFull has no start/end — see note below"),
    };
    let substs = self.ctx.ty_ctx_mut().intern_substitution(vec![glyim_type::GenericArg::Ty(expr.ty /* element type — confirm this is accessible here, else thread it from the caller */)]);
    glyim_mir::Rvalue::Aggregate(glyim_mir::AggregateKind::Adt(adt_id, 0, substs), operands)
}
```
Note: unbounded `..` (`RangeFull`, both `start`/`end` are `None`) has no
registered ADT id in `register_builtin_ranges` — it's a zero-sized unit
type with no fields. If the language surface supports bare `..` as an
expression (check `glyim-frontend/src/parser/expr.rs` for whether it
parses `Range { start: None, end: None, .. }` at all — if the grammar
never produces that shape, this arm is genuinely unreachable and the
`unreachable!()` is correct; if it does parse, add a 6th builtin ADT
`RangeFull` at id `1006` with zero fields, next to the other five in
`register_builtin_ranges`, and handle it here instead of panicking).

**Verify:** MIR-snapshot test (the codebase already has a snapshot
mechanism, `glyim-test/src/snapshot/`) for `let r = 1..5;` — assert the
produced `Rvalue::Aggregate` uses `AggregateKind::Adt(AdtId(1000), ...)`
with two real operands, not an empty tuple.

---

### 1.5 Const evaluation — unsupported expression kinds (`glyim-const-eval/src/eval.rs`)

**Current:** `Expr::Call`, `MethodCall`, `Closure`, `Loop`, `While`, `For`,
`Range`, `Return`, `Break`, `Continue`, `Assign` (except simple path), `Ref`,
limited `Cast` all hard-`Err`. `MAX_EVAL_DEPTH` is a fixed constant with no
per-call dynamic limit.

**Scope decision:** const-eval intentionally does not need full Turing-
complete evaluation — Rust's own `const fn` evaluator restricts I/O, heap
allocation, and unbounded loops too. The *correct* production-grade target
here is **not** "implement everything the report lists", it's "support the
subset that's actually reachable from `const`/array-length contexts,
reject the rest with a clear diagnostic instead of a generic string." Do
these, in order of value:

1. **`Expr::Loop`/`Expr::While`** — implement with a hard iteration cap
   derived from `MAX_EVAL_DEPTH` (reuse it as a step budget, not just a
   call-depth budget — rename semantically or add a sibling
   `MAX_EVAL_STEPS` constant). This is needed for realistic `const fn`
   bodies (e.g. compile-time table generation) and is bounded/safe:
   ```rust
   Expr::Loop { body } => {
       let mut steps = 0u32;
       loop {
           steps += 1;
           if steps > MAX_EVAL_STEPS {
               return Err(ConstEvalError::new("const evaluation exceeded step budget (possible infinite loop)", expr.span));
           }
           match self.evaluate_expr(body, env, depth + 1)? {
               // however this evaluator currently represents break-with-value —
               // check whether there's already a ControlFlow-like return type
               // threaded through evaluate_expr (grep `enum EvalControlFlow` /
               // `enum ExprResult` in this file) and reuse it; if evaluate_expr
               // currently just returns `ConstEvalResult<ConstValue>` with no
               // way to signal break/continue, that plumbing has to be added
               // first — this is the one place in this item that's a genuine
               // architectural change, not a mechanical fill-in.
           }
       }
   }
   ```
   **Prerequisite:** before this can work, `evaluate_expr`'s return type
   needs to be able to express "this block ended via `break <value>`" vs
   "this block ended via `continue`" vs "normal value", because `Break`/
   `Continue`/`Return` currently `Err` out unconditionally. Change the
   internal signature (keep the public API returning
   `ConstEvalResult<ConstValue>` at the top level) to something like:
   ```rust
   enum Flow { Value(ConstValue), Break(Option<ConstValue>), Continue, Return(ConstValue) }
   fn evaluate_expr_flow(&mut self, expr: &Expr, env: &mut Env, depth: u32) -> ConstEvalResult<Flow>;
   ```
   and have `Expr::Block`'s statement loop, `Expr::If`, `Expr::Match`
   (which already work today per the report) propagate `Flow::Break`/
   `::Continue`/`::Return` upward without evaluating further statements,
   while `Loop`/`While`/`For` catch `Break`/`Continue` and everything else
   (`Call` sites, top-level `evaluate`) still just unwraps `Flow::Value`
   and treats `Break`/`Continue`/`Return` reaching there as a hard error
   ("break outside loop", etc. — real diagnostics, not "not supported").

2. **`Expr::For`** — once `Loop`/`While`/`Flow` exist, desugar `for pat in
   iter { body }` the same way the real (non-const) lowering does — check
   `glyim-hir/src/lower/lower_expr.rs` for how `For` desugars to
   `Iterator::next`-based `Loop` there, and mirror only the shape (not the
   full trait-solving — const-eval's `For` only needs to support `Range`
   iteration in practice; if the loop variable comes from `1..10`,
   directly step an integer counter rather than trying to const-evaluate
   trait dispatch. If the iterable isn't a `Range` literal at the syntactic
   level, `Err` with "for-loop const evaluation only supports range
   iteration" — an honest, scoped limitation beats a fake general
   implementation).

3. **`Expr::Return`/`Break`/`Continue`** — become real `Flow` variants per
   above instead of unconditional `Err`.

4. **`Expr::Assign`** (currently only simple-path assignment works) —
   extend to field/index projections by reusing whatever place-resolution
   the existing simple-path case already has (grep the current `Assign`
   arm; it should already resolve a `Path` to a mutable slot in `env` —
   extend that same resolution to walk one level of `Field`/`Index` before
   assigning, mirroring `write_through_projections_with_locals` in
   `glyim-mir-interp` at a much smaller scope since const-eval's `env` is
   presumably a `HashMap<Name, ConstValue>` or similar, not MIR locals —
   check the actual `Env`/state type in this file before writing this).

5. **`Expr::Call`/`Expr::MethodCall`** — support calling other `const fn`s
   (this is the single highest-value item in this list — without it,
   const generics and const fns that call helper const fns are unusable).
   Requires: resolving the callee to a HIR `ItemKind::Fn` marked `const`
   (check whether the HIR/AST already has a `is_const: bool` on function
   items — if not, that's a small frontend/HIR addition, flag it as a
   prerequisite rather than skip this item), recursively evaluating its
   body with a fresh `env` seeded from the evaluated argument values, and
   respecting `depth + 1` against `MAX_EVAL_DEPTH` (already exists) for
   recursion, separately from the new `MAX_EVAL_STEPS` loop budget. Reject
   (clear diagnostic) calls to any function not marked `const`.

6. **`Expr::Closure`/`Expr::Ref`** — leave as `Err` ("closures/references
   are not supported in constant evaluation") — this matches real Rust's
   own const-eval restrictions and is not a gap worth closing; update the
   report mentally to drop these two from the "TODO" list.

7. **`eval_cast`** — extend beyond primitive numeric/bool to pointer casts
   only if `Expr::Ref`/raw pointers are ever reachable in a const context
   at all (per point 6, they're rejected upstream, so **no change needed
   here** — this sub-item is dead once point 6 stands, since a cast can
   only ever see values `evaluate_expr` was willing to produce).

**Verify:** const-eval test suite additions: `const fn double(x: i32) -> i32
{ x * 2 }` used in an array length context; a `const` loop summing `0..10`;
a `const fn` with an early `return` inside an `if`.

---

### 1.6 Drop elaboration — per-projection dataflow (`glyim-opt/src/drop_elaboration.rs`)

**Current:** `MaybeInitialized::compute` (line ~20-68) tracks `entry:
Vec<Vec<bool>>` indexed by `[block_idx][local_idx]` — one bit per whole
local, no distinction between `x` fully moved vs. only `x.field` moved.

**Fix — introduce a move-path tree, the same technique real MIR-based
compilers use (rustc's `MovePathIndex`):**

1. Add a `MovePath` arena built once per body, before dataflow:
```rust
struct MovePath {
    place: Place,           // local + projection prefix this node represents
    parent: Option<MovePathIndex>,
    children: Vec<MovePathIndex>,
}
glyim_core::define_idx!(MovePathIndex); // matches the idx-newtype pattern already used elsewhere (e.g. LocalVarId)

struct MovePaths {
    paths: IndexVec<MovePathIndex, MovePath>,
    // Map from (local, projection) -> MovePathIndex, built by walking every
    // place that's ever the target of a Drop, a move-out (Operand::Move),
    // or a partial-move pattern binding in the body.
    lookup: HashMap<Place, MovePathIndex>,
}
```
   Build it by scanning the body once for every distinct `Place` that
   appears as: (a) a `Drop` terminator target, (b) an `Operand::Move`
   source, (c) an assignment target. For each such place, walk its
   projection prefixes (`local`, `local.field`, `local.field.subfield`,
   ...) inserting a `MovePath` node per prefix level not already present,
   linking child->parent. This mirrors exactly what the module doc already
   describes as missing ("per‑projection MaybeInitialized dataflow").

2. Change `MaybeInitialized::entry` from `Vec<Vec<bool>>` (per local) to
   `Vec<FixedBitSet>` sized to `move_paths.paths.len()` (per move-path
   node, i.e. per field/element, not per local). `glyim-borrowck` already
   depends on `fixedbitset` (see its `Cargo.toml`) — add the same
   dependency to `glyim-opt/Cargo.toml` and reuse it here instead of
   `Vec<bool>`, for both correctness-by-construction (bitset ops) and
   consistency with the rest of the codebase.

3. Transfer function changes: an assignment to `place` (with a non-empty
   projection, e.g. `x.field = ...`) sets **only** `x.field`'s move-path
   bit, not `x`'s — but must also set `x`'s bit if and only if *every*
   sibling field of `x` becomes initialized (this "unsplit" rule is what
   makes whole-`x` drops correct again once all fields are reassigned).
   A `Drop` or `Operand::Move` of `x.field` clears `x.field`'s bit (and
   implicitly makes `x` itself "partially moved" — do **not** clear `x`'s
   own bit derivation from a memoized "all children initialized" check
   rather than storing it independently, to avoid the two facts diverging).

4. Elaboration: where `run()` (line 158) currently decides whether to emit
   an unconditional vs. drop-flag-guarded drop for a whole local, extend it
   to walk the ADT's fields (via `glyim_type::AdtKind`/`adt_def.fields`,
   already imported in this file) and, for a `Struct`/`Enum` variant being
   dropped, recursively emit a **per-field** conditional drop (query each
   field's move-path bit at the drop point) instead of one flag for the
   whole value. This directly fixes the report's stated failure mode
   ("missed optimizations or incorrect drop flags" — the "incorrect" part
   specifically means: today, partially-moved-from structs either double-
   drop a moved field or leak a non-moved one, since there's only one flag
   for the whole struct).

**Verify:** MIR test: a struct with two `String` fields, one moved out in
one branch of an `if`, the other left intact; assert the elaborated drop at
function end drops only the still-initialized field on that path
(currently: it either drops both — use-after-move on the moved field — or
neither — leak). This is a real soundness bug in the current code, not
just a missed optimization, so treat this item as Tier-0-severity even
though it's filed under Tier 1 here for dependency-ordering reasons (it
needs `fixedbitset`, no other Tier-0 item does).

---

### 1.7 Dynamic range slicing (`arr[i..j]`) — confirmed NOT a `glyim-opt` bug

`glyim-opt/src/slice_desugar.rs`'s own module doc (already in your source,
lines ~30-50) correctly identifies that this belongs in `glyim-lower` at
THIR→MIR build time, as an ordinary `Rvalue`, not a `Place` projection.
**Do not modify `slice_desugar.rs` for this** — it's already correct and
already explains where the real fix goes. Implement it there:

**`glyim-lower/src/lower_rvalue.rs`, `thir::ExprKind::Index` arm (line
~501-574):** currently, when `index` is itself a `thir::ExprKind::Range`,
check what it does today (read lines 501-574 in full before editing — the
report doesn't say this arm currently panics or errors, so confirm exact
current behavior first: it may already partially handle the constant-range
sub-case via `ConstantIndex`/`Subslice` place projections, and only the
*non-constant* bound case is the actual gap). For the case where `index`
is `Range { start, end, .. }` and at least one of `start`/`end` is **not**
a compile-time-constant `thir::Expr::Literal`, replace the
`Place`-projection path with the `Rvalue` sequence the doc comment
describes:
```rust
// data_ptr = base_ptr + start * elem_size
// len = end - start
// result = { ptr: data_ptr, len }
let base_ptr_place = self.lower_place(base)?; // however this file already gets a Place for the base slice/array
let elem_ty = /* element type of base's type */;
let elem_size = self.const_eval_or_layout_size(elem_ty)?; // use glyim-layout here (this crate should already depend on it if it computes any sizes; if not, add the dependency same as item 0.1)
let start_op = start.as_ref().map(|e| self.lower_operand(e)).unwrap_or(Operand::Constant(mir_const_zero));
let end_op = end.as_ref().map(|e| self.lower_operand(e)).unwrap_or(/* base's len, via Rvalue::Len on base_ptr_place */);
let data_ptr_local = self.new_temp(ptr_ty);
self.push_stmt(StatementKind::Assign(Place::new(data_ptr_local),
    Rvalue::BinaryOp(BinOp::Offset, Operand::Copy(base_ptr_place /* as ptr */), /* start_op * elem_size, via a Mul BinaryOp temp */)));
let len_local = self.new_temp(usize_ty);
self.push_stmt(StatementKind::Assign(Place::new(len_local),
    Rvalue::BinaryOp(BinOp::Sub, end_op.clone(), start_op.clone())));
Rvalue::Aggregate(AggregateKind::Tuple, vec![Operand::Copy(Place::new(data_ptr_local)), Operand::Copy(Place::new(len_local))])
```
Match this against whatever `new_temp`/`push_stmt`/`lower_place`/
`lower_operand` helpers this file actually exposes (it's a 1500-line file;
these almost certainly already exist under similar names — grep before
inventing new ones) and against how the *existing* constant-range case
builds its `{ ptr, len }` aggregate (search this same file for
`slice_operands` around line 1571 — reuse that exact tuple shape so both
the constant and dynamic paths produce identically-shaped values for
downstream codegen).

**Verify:** codegen/interpreter test for `let s = &arr[i..j]` where `i`/`j`
are runtime function parameters, not literals — currently this either
fails to lower or silently mis-slices; after the fix it should produce
correct `{ptr, len}` for e.g. `arr = [1,2,3,4,5], i=1, j=4` → `[2,3,4]`.
