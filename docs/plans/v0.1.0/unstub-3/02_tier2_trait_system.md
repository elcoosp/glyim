## TIER 2 — Trait system completeness

Depends on Tier 1.2.a (`ImplDef.items`) and 1.2.d (long-lived `TraitContext`
owned by the pipeline) being done first — coherence and HRTB both operate on
the same `TraitContext`/`ImplDef` types.

### 2.1 Coherence overlap check ignores generic args — `glyim-typeck/src/coherence.rs`

**Current (`structural_tys_match`, line ~52-83, confirmed):**
```rust
(glyim_type::TyKind::Adt(id_a, _), glyim_type::TyKind::Adt(id_b, _)) => id_a == id_b,
```
The second tuple element (the `Substitution`) is discarded — `Vec<i32>` and
`Vec<String>` are treated as overlapping. This is a real soundness gap in
coherence checking (it would reject two impls that are actually fine, e.g.
`impl Foo for Vec<i32>` and `impl Foo for Vec<String>`, as overlapping when
they aren't).

**Fix:** recurse into the substitution's generic args, same pattern already
used for `Ref`/`RawPtr` two lines below it:
```rust
(glyim_type::TyKind::Adt(id_a, subs_a), glyim_type::TyKind::Adt(id_b, subs_b)) => {
    if id_a != id_b {
        return false;
    }
    let args_a = ctx.substitution_args(*subs_a);
    let args_b = ctx.substitution_args(*subs_b);
    if args_a.len() != args_b.len() {
        return false;
    }
    args_a.iter().zip(args_b.iter()).all(|(a, b)| match (a, b) {
        (glyim_type::GenericArg::Ty(ta), glyim_type::GenericArg::Ty(tb)) => {
            self.structural_tys_match(ctx, *ta, *tb)
        }
        // Lifetime/const generic args: treat as always-compatible for overlap
        // purposes unless this crate models const generics precisely enough
        // to compare — check glyim_type::GenericArg's variants before writing
        // this arm; if there's a Const(ConstVal) variant, compare it structurally
        // the same way, don't just return true.
        _ => true,
    })
}
```
Also apply the same recursive substitution-aware comparison to
`TyKind::Tuple(subs)` and `TyKind::FnDef`/`TyKind::FnPtr` arms if
`structural_tys_match` has cases for them below line 83 (read the rest of
the function — it was truncated in this exploration; finish reading before
editing to make sure every `Ty` variant with nested types recurses, not
just `Adt`).

**Verify:** coherence test: `impl Trait for Vec<i32> {}` and
`impl Trait for Vec<String> {}` in the same crate must both be accepted
(today: rejected as overlapping — confirm this is actually the current
broken behavior with a regression test before fixing, then confirm it
passes after).

---

### 2.2 HRTB predicates — mostly `Ambiguous` — `glyim-solve/src/hrtb.rs`

**Current:** `check_hrtb` for `RegionOutlives`, `TypeOutlives`, `WellFormed`,
`Coerce` frequently falls through to `crate::solver::SolverResult::Ambiguous`
even in provable cases (confirmed at lines ~294, ~316, ~330, ~341).

**Scope decision:** a fully general HRTB solver (real region inference with
placeholder regions, subtyping lattice, etc.) is a multi-week project on its
own and is **not** what's blocking real programs from compiling — the
practical gap is that *trivially provable* cases (the overwhelming majority
in real code) return `Ambiguous` instead of `Proven`, forcing callers to
reject or over-conservatively accept. Fix the easy 80%:

- **`RegionOutlives(a, b)`**: if `a`/`b` are the exact same `Region`, return
  `Proven` immediately (reflexivity) before falling into the conservative
  branch — check whether this trivial case is even handled today (read
  lines 283-302 in full; if reflexivity isn't checked, that's the bug, not
  the general algorithm).
- **`TypeOutlives(ty, region)`**: if `region` is `Region::STATIC` (or
  whatever this crate's "outlives everything" static region constant is
  called — grep `Region::` for a `'static`-equivalent), always `Proven` —
  everything outlives `'static`... wait, inverted: everything is outlived
  *by* `'static`, so `T: 'static` is provable whenever `T` contains no
  non-`'static` region components; for a first pass, `Proven` whenever `ty`
  is a scalar/owned type with no `Ref`/lifetime parameters in it at all
  (walk `ty`'s `TypeFlags` — `glyim-type/src/flags.rs` almost certainly
  already computes a "contains region" flag as part of `compute_flags`
  seen in `ty_ctx_mut.rs`; use that instead of re-walking the type).
- **`WellFormed(ty)`**: `Proven` for any concrete (non-generic, non-`Dynamic`,
  non-projection) type — well-formedness of e.g. `i32`, `bool`, a concrete
  `Adt` with no unsatisfied where-clauses is not actually ambiguous, it's
  simply true. Only fall back to `Ambiguous` for types containing unresolved
  inference variables or generic params whose bounds haven't been checked.
- **`Coerce(a, b)`**: `Proven` when `a == b` (identity coercion) or when one
  of the already-implemented non-HRTB coercion rules elsewhere in
  `glyim-solve` (check `fulfill.rs`/`infer.rs` for an existing
  `can_coerce`/`coerce` helper — reuse it, don't duplicate coercion logic
  here) already says yes; only genuinely open higher-ranked coercions stay
  `Ambiguous`.

**This item is "cheap wins that unblock real code", not "solve HRTB
correctly"** — leave a doc comment on `check_hrtb` stating explicitly which
cases remain conservative and why, so it isn't mistaken for complete.

**Verify:** solver tests for `fn f<'a>(x: &'a i32) where &'a i32: 'a` (self-
outlives, must prove) and a concrete-type well-formedness check that
currently wrongly returns `Ambiguous`.

---

### 2.3 Object safety — associated types & supertraits ignored — `glyim-type/src/object_safety.rs`

**Current (confirmed):** `MethodSignature` (line 33) has no
`associated_types`/`supertraits` fields at all; `check_object_safety`
(line 60) only ever inspects per-method receiver/generic-param shape.

**Fix:**
1. Extend the checked-trait's info, not `MethodSignature` (associated types
   and supertraits are trait-level, not method-level — don't bolt them onto
   `MethodSignature`, add a new input struct):
```rust
pub struct TraitObjectSafetyInput {
    pub requires_self_sized: bool,
    pub methods: Vec<MethodSignature>,
    pub associated_types: Vec<AssociatedTypeInfo>,   // NEW
    pub supertraits: Vec<TraitDefId>,                // NEW
}
pub struct AssociatedTypeInfo {
    pub name: Name,
    pub span: Span,
    pub is_constrained_in_all_methods: bool, // whether every method signature mentions it, making it inferable from the vtable's own methods
}
```
2. Add violation checks:
```rust
for at in &input.associated_types {
    if !at.is_constrained_in_all_methods {
        violations.push(ObjectSafetyViolation::UnconstrainedAssociatedType { name: at.name, span: at.span });
    }
}
```
   (The `UnconstrainedAssociatedType` variant already exists in
   `ObjectSafetyViolation` — it's declared but never constructed today;
   confirm that with `grep -rn UnconstrainedAssociatedType` before adding,
   since if it's already used somewhere this note is stale.)
3. Supertraits: a trait is only object-safe if **all** its supertraits are
   also object-safe (recursively) — since `glyim-type` shouldn't own trait-
   resolution recursion itself (keep this pure/data-in-data-out per the
   module's existing "avoids depending on glyim-hir" design note at the top
   of the file), change the signature to take pre-resolved
   `supertrait_safety: Vec<(TraitDefId, bool)>` computed by the caller
   (`glyim-typeck`, which does the recursive walk over
   `TraitContext`/`TraitDef.predicates` — `TraitDef` already has
   `predicates: Vec<Predicate>`, filter for supertrait predicates there) and
   simply fold:
```rust
for (trait_id, is_safe) in &input.supertrait_safety {
    if !is_safe {
        violations.push(ObjectSafetyViolation::SelfSized /* or a new SupertraitNotObjectSafe variant, cleaner */);
    }
}
```
   Prefer adding `ObjectSafetyViolation::SupertraitNotObjectSafe { trait_id: TraitDefId, span: Span }` as a new, honestly-named variant rather than overloading `SelfSized`.

**Verify:** a trait with an unconstrained associated type (`trait Foo { type
Bar; fn f(&self); }` where `Bar` never appears in any method signature) must
be flagged not-object-safe; a trait whose supertrait is itself not object-
safe must also be flagged.
