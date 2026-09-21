//! Higher-Ranked Trait Bounds (HRTB) implementation.
//!
//! Handles `for<'a>` bounds by:
//! 1. Creating placeholder regions in a new universe
//! 2. Substituting bound regions with placeholders in the binder's value
//! 3. Checking that the resulting predicate holds for *all* regions

use glyim_type::*;

/// Result of instantiating a binder with placeholder regions.
#[derive(Debug)]
pub struct PlaceholderInstantiation<T> {
    /// The value with bound regions replaced by placeholders.
    pub value: T,
    /// The placeholder regions that were created.
    pub placeholders: Vec<PlaceholderRegion>,
    /// The universe the placeholders live in.
    pub universe: UniverseIndex,
}

/// Instantiate a `Binder<T>` by replacing each bound region with a placeholder
/// in a new universe.
pub fn instantiate_binder_with_placeholders<T>(
    binder: &Binder<T>,
    infer: &mut crate::InferenceTable,
    ctx: &mut TyCtxMut,
) -> PlaceholderInstantiation<T>
where
    T: Clone + SubstituteBoundVars,
{
    let universe = infer.create_universe();

    let placeholders: Vec<PlaceholderRegion> = binder
        .bound_vars
        .iter()
        .enumerate()
        .filter_map(|(idx, var)| match var {
            BoundVariableKind::Region(kind) => Some(PlaceholderRegion {
                universe,
                bound: kind.clone(),
                index: idx as u32,
            }),
            _ => None,
        })
        .collect();

    let region_map = build_region_substitution(&binder.bound_vars, &placeholders);
    let value = binder.value.clone().substitute(&region_map, ctx);

    PlaceholderInstantiation {
        value,
        placeholders,
        universe,
    }
}

fn build_region_substitution(
    bound_vars: &[BoundVariableKind],
    placeholders: &[PlaceholderRegion],
) -> BoundVarSubstitution {
    let mut region_map: Vec<Region> = Vec::new();
    let mut placeholder_idx = 0;

    for (idx, var) in bound_vars.iter().enumerate() {
        match var {
            BoundVariableKind::Region(_) => {
                if placeholder_idx < placeholders.len() {
                    region_map.push(Region::Placeholder(placeholders[placeholder_idx].clone()));
                    placeholder_idx += 1;
                }
            }
            BoundVariableKind::Ty(kind) => {
                let _ = (idx, kind);
            }
            BoundVariableKind::Const => {}
        }
    }

    BoundVarSubstitution {
        region_map,
        has_placeholders: !placeholders.is_empty(),
    }
}

/// A substitution mapping bound variable indices to their replacements.
#[derive(Debug, Clone)]
pub struct BoundVarSubstitution {
    /// Maps bound region index to replacement region.
    pub region_map: Vec<Region>,
    /// Whether this substitution contains any placeholders.
    pub has_placeholders: bool,
}

impl BoundVarSubstitution {
    /// Create an empty substitution.
    pub fn empty() -> Self {
        Self {
            region_map: Vec::new(),
            has_placeholders: false,
        }
    }
}

/// Trait for types that can have their bound variables substituted.
pub trait SubstituteBoundVars: Sized {
    /// Substitute bound variables according to the given mapping.
    fn substitute(self, sub: &BoundVarSubstitution, ctx: &mut TyCtxMut) -> Self;
}

impl SubstituteBoundVars for Region {
    fn substitute(self, sub: &BoundVarSubstitution, _ctx: &mut TyCtxMut) -> Self {
        match self {
            Region::LateBound(_depth, idx, ref _kind) => {
                if let Some(replacement) = sub.region_map.get(idx as usize) {
                    replacement.clone()
                } else {
                    self
                }
            }
            _ => self,
        }
    }
}

impl SubstituteBoundVars for Ty {
    fn substitute(self, sub: &BoundVarSubstitution, ctx: &mut TyCtxMut) -> Self {
        match ctx.ty_kind(self).clone() {
            TyKind::Ref(region, inner, mutability) => {
                let region = region.substitute(sub, ctx);
                let inner = inner.substitute(sub, ctx);
                ctx.mk_ref(region, inner, mutability)
            }
            TyKind::RawPtr(inner, mutability) => {
                let inner = inner.substitute(sub, ctx);
                ctx.mk_ty(TyKind::RawPtr(inner, mutability))
            }
            TyKind::Slice(inner) => {
                let inner = inner.substitute(sub, ctx);
                ctx.mk_ty(TyKind::Slice(inner))
            }
            TyKind::Array(inner, cnst) => {
                let inner = inner.substitute(sub, ctx);
                ctx.mk_ty(TyKind::Array(inner, cnst))
            }
            TyKind::Tuple(substs) => {
                substitute_through_generic_args(sub, ctx, substs, TyKind::Tuple)
            }
            TyKind::Adt(id, substs) => {
                let new_substs = substitute_substitution(substs, sub, ctx);
                ctx.mk_ty(TyKind::Adt(id, new_substs))
            }
            TyKind::FnDef(id, substs) => {
                let new_substs = substitute_substitution(substs, sub, ctx);
                ctx.mk_ty(TyKind::FnDef(id, new_substs))
            }
            TyKind::Closure(id, substs) => {
                let new_substs = substitute_substitution(substs, sub, ctx);
                ctx.mk_ty(TyKind::Closure(id, new_substs))
            }
            TyKind::Opaque(id, substs) => {
                let new_substs = substitute_substitution(substs, sub, ctx);
                ctx.mk_ty(TyKind::Opaque(id, new_substs))
            }
            TyKind::FnPtr(sig) => {
                let new_inputs = substitute_substitution(sig.inputs, sub, ctx);
                let new_output = sig.output.substitute(sub, ctx);
                ctx.mk_ty(TyKind::FnPtr(FnSig {
                    inputs: new_inputs,
                    output: new_output,
                    c_variadic: sig.c_variadic,
                    unsafety: sig.unsafety,
                    abi: sig.abi,
                }))
            }
            TyKind::Dynamic(preds, region) => {
                let region = region.substitute(sub, ctx);
                ctx.mk_ty(TyKind::Dynamic(preds, region))
            }
            TyKind::Projection(proj) => {
                let new_substs = substitute_substitution(proj.trait_ref.substs, sub, ctx);
                ctx.mk_ty(TyKind::Projection(ProjectionTy {
                    trait_ref: TraitRef {
                        def_id: proj.trait_ref.def_id,
                        substs: new_substs,
                    },
                    item_name: proj.item_name,
                }))
            }
            TyKind::Bound(_idx, _bound) => self,
            _ => self,
        }
    }
}

/// Helper to substitute through a Tuple, avoiding double &mut borrow on ctx.
fn substitute_through_generic_args(
    sub: &BoundVarSubstitution,
    ctx: &mut TyCtxMut,
    substs: Substitution,
    kind_ctor: impl Fn(Substitution) -> TyKind,
) -> Ty {
    let new_substs = substitute_substitution(substs, sub, ctx);
    ctx.mk_ty(kind_ctor(new_substs))
}

impl SubstituteBoundVars for Predicate {
    fn substitute(self, sub: &BoundVarSubstitution, ctx: &mut TyCtxMut) -> Self {
        match self {
            Predicate::Trait(tp) => {
                let new_substs = substitute_substitution(tp.trait_ref.substs, sub, ctx);
                Predicate::Trait(TraitPredicate {
                    trait_ref: TraitRef {
                        def_id: tp.trait_ref.def_id,
                        substs: new_substs,
                    },
                    polarity: tp.polarity,
                })
            }
            Predicate::RegionOutlives(rp) => {
                let a = rp.a.substitute(sub, ctx);
                let b = rp.b.substitute(sub, ctx);
                Predicate::RegionOutlives(RegionOutlivesPredicate { a, b })
            }
            Predicate::TypeOutlives(tp) => {
                let ty = tp.ty.substitute(sub, ctx);
                let region = tp.region.substitute(sub, ctx);
                Predicate::TypeOutlives(TypeOutlivesPredicate { ty, region })
            }
            Predicate::WellFormed(ty) => {
                let ty = ty.substitute(sub, ctx);
                Predicate::WellFormed(ty)
            }
            Predicate::Coerce(a, b) => {
                let a = a.substitute(sub, ctx);
                let b = b.substitute(sub, ctx);
                Predicate::Coerce(a, b)
            }
        }
    }
}

fn substitute_substitution(
    substs: Substitution,
    sub: &BoundVarSubstitution,
    ctx: &mut TyCtxMut,
) -> Substitution {
    let args = ctx.substitution_args(substs).to_vec();
    let new_args: Vec<GenericArg> = args
        .into_iter()
        .map(|arg| match arg {
            GenericArg::Ty(t) => GenericArg::Ty(t.substitute(sub, ctx)),
            GenericArg::Lifetime(r) => GenericArg::Lifetime(r.substitute(sub, ctx)),
            GenericArg::Const(c) => GenericArg::Const(c),
        })
        .collect();
    ctx.intern_substitution(new_args)
}

/// Maximum recursion depth for [`ty_struct_eq`] and friends. Beyond this we
/// conservatively return "not proven equal" rather than risk a stack overflow
/// on pathological deeply-nested recursive types (de-stubbing §2.6).
const MAX_STRUCT_EQ_DEPTH: usize = 256;

/// Structural (interning-independent) equality of two types.
///
/// `Ty`/`Substitution`/`Region` equality is by interned handle, but HRTB
/// instantiation re-interns substituted types, so two structurally identical
/// types can carry different handles. This recurses through the type
/// structure (including substitution arguments) so identity coercion can be
/// proven for substituted types. A `depth` budget bounds the recursion.
fn ty_struct_eq(a: Ty, b: Ty, ctx: &TyCtx, depth: usize) -> bool {
    if depth >= MAX_STRUCT_EQ_DEPTH {
        // Pathological nesting: give up rather than overflow the stack. The
        // caller treats a `false` here as "not proven equal", which is the
        // safe conservative outcome for HRTB identity coercion.
        return false;
    }
    match (ctx.ty_kind(a), ctx.ty_kind(b)) {
        (TyKind::Ref(ra, ia, ma), TyKind::Ref(rb, ib, mb)) => {
            ra == rb && ma == mb && ty_struct_eq(*ia, *ib, ctx, depth + 1)
        }
        (TyKind::RawPtr(ia, ma), TyKind::RawPtr(ib, mb)) => {
            ma == mb && ty_struct_eq(*ia, *ib, ctx, depth + 1)
        }
        (TyKind::Slice(ia), TyKind::Slice(ib)) => ty_struct_eq(*ia, *ib, ctx, depth + 1),
        (TyKind::Array(ia, ca), TyKind::Array(ib, cb)) => {
            ca == cb && ty_struct_eq(*ia, *ib, ctx, depth + 1)
        }
        (TyKind::Tuple(sa), TyKind::Tuple(sb)) => substs_struct_eq(*sa, *sb, ctx, depth + 1),
        (TyKind::Adt(ida, sa), TyKind::Adt(idb, sb)) => {
            ida == idb && substs_struct_eq(*sa, *sb, ctx, depth + 1)
        }
        (TyKind::FnDef(ida, sa), TyKind::FnDef(idb, sb)) => {
            ida == idb && substs_struct_eq(*sa, *sb, ctx, depth + 1)
        }
        (TyKind::Closure(ida, sa), TyKind::Closure(idb, sb)) => {
            ida == idb && substs_struct_eq(*sa, *sb, ctx, depth + 1)
        }
        (TyKind::Opaque(ida, sa), TyKind::Opaque(idb, sb)) => {
            ida == idb && substs_struct_eq(*sa, *sb, ctx, depth + 1)
        }
        (TyKind::FnPtr(sa), TyKind::FnPtr(sb)) => {
            sa.c_variadic == sb.c_variadic
                && sa.unsafety == sb.unsafety
                && sa.abi == sb.abi
                && substs_struct_eq(sa.inputs, sb.inputs, ctx, depth + 1)
                && ty_struct_eq(sa.output, sb.output, ctx, depth + 1)
        }
        (TyKind::Dynamic(pa, ra), TyKind::Dynamic(pb, rb)) => {
            ra == rb && preds_struct_eq(pa, pb, ctx, depth + 1)
        }
        (TyKind::Projection(pa), TyKind::Projection(pb)) => {
            pa.trait_ref.def_id == pb.trait_ref.def_id
                && substs_struct_eq(pa.trait_ref.substs, pb.trait_ref.substs, ctx, depth + 1)
                && pa.item_name == pb.item_name
        }
        (TyKind::Int(a), TyKind::Int(b)) => a == b,
        (TyKind::Uint(a), TyKind::Uint(b)) => a == b,
        (TyKind::Float(a), TyKind::Float(b)) => a == b,
        (TyKind::Bool, TyKind::Bool) => true,
        (TyKind::Char, TyKind::Char) => true,
        (TyKind::String, TyKind::String) => true,
        (TyKind::Never, TyKind::Never) => true,
        (TyKind::Unit, TyKind::Unit) => true,
        (TyKind::Param(a), TyKind::Param(b)) => a == b,
        (TyKind::Infer(_), TyKind::Infer(_)) => false,
        (TyKind::Error, TyKind::Error) => true,
        (TyKind::Bound(_, _), TyKind::Bound(_, _)) => false,
        _ => false,
    }
}

fn substs_struct_eq(sa: Substitution, sb: Substitution, ctx: &TyCtx, depth: usize) -> bool {
    let a = ctx.substitution_args(sa);
    let b = ctx.substitution_args(sb);
    a.len() == b.len()
        && a.iter().zip(b.iter()).all(|(ga, gb)| match (ga, gb) {
            (GenericArg::Ty(ta), GenericArg::Ty(tb)) => ty_struct_eq(*ta, *tb, ctx, depth + 1),
            (GenericArg::Lifetime(ra), GenericArg::Lifetime(rb)) => ra == rb,
            (GenericArg::Const(ca), GenericArg::Const(cb)) => ca == cb,
            _ => false,
        })
}

fn preds_struct_eq(
    pa: &glyim_type::Binder<Box<[glyim_type::Predicate]>>,
    pb: &glyim_type::Binder<Box<[glyim_type::Predicate]>>,
    ctx: &TyCtx,
    depth: usize,
) -> bool {
    let a = &pa.value;
    let b = &pb.value;
    a.len() == b.len()
        && a.iter()
            .zip(b.iter())
            .all(|(p, q)| pred_struct_eq(p, q, ctx, depth + 1))
}

fn pred_struct_eq(
    p: &glyim_type::Predicate,
    q: &glyim_type::Predicate,
    ctx: &TyCtx,
    depth: usize,
) -> bool {
    match (p, q) {
        (Predicate::Trait(tp), Predicate::Trait(tq)) => {
            tp.trait_ref.def_id == tq.trait_ref.def_id
                && substs_struct_eq(tp.trait_ref.substs, tq.trait_ref.substs, ctx, depth + 1)
                && tp.polarity == tq.polarity
        }
        (Predicate::RegionOutlives(rp), Predicate::RegionOutlives(rq)) => {
            rp.a == rq.a && rp.b == rq.b
        }
        (Predicate::TypeOutlives(tp), Predicate::TypeOutlives(tq)) => {
            ty_struct_eq(tp.ty, tq.ty, ctx, depth + 1) && tp.region == tq.region
        }
        (Predicate::WellFormed(ta), Predicate::WellFormed(tb)) => {
            ty_struct_eq(*ta, *tb, ctx, depth + 1)
        }
        (Predicate::Coerce(a, b), Predicate::Coerce(c, d)) => {
            ty_struct_eq(*a, *c, ctx, depth + 1) && ty_struct_eq(*b, *d, ctx, depth + 1)
        }
        _ => false,
    }
}

/// Returns true if `region` (by structural equality, ignoring the universe)
/// appears anywhere within `ty`. Used to prove reflexive `T: 'r` bounds under
/// HRTB — if `T` contains `'r`, then `T: 'r` holds for every instantiation.
fn region_in_ty(region: &Region, ty: Ty, ctx: &TyCtx) -> bool {
    match ctx.ty_kind(ty) {
        TyKind::Ref(r, inner, _) => r == region || region_in_ty(region, *inner, ctx),
        TyKind::RawPtr(inner, _) => region_in_ty(region, *inner, ctx),
        TyKind::Slice(inner) => region_in_ty(region, *inner, ctx),
        TyKind::Array(inner, _) => region_in_ty(region, *inner, ctx),
        TyKind::Tuple(substs) => ctx
            .substitution_args(*substs)
            .iter()
            .any(|a| matches!(a, GenericArg::Ty(t) if region_in_ty(region, *t, ctx))),
        TyKind::Adt(_, substs)
        | TyKind::FnDef(_, substs)
        | TyKind::Closure(_, substs)
        | TyKind::Opaque(_, substs) => ctx
            .substitution_args(*substs)
            .iter()
            .any(|a| matches!(a, GenericArg::Ty(t) if region_in_ty(region, *t, ctx))),
        TyKind::FnPtr(sig) => {
            ctx.substitution_args(sig.inputs)
                .iter()
                .any(|a| matches!(a, GenericArg::Ty(t) if region_in_ty(region, *t, ctx)))
                || region_in_ty(region, sig.output, ctx)
        }
        TyKind::Dynamic(_, r) => r == region,
        _ => false,
    }
}

/// Returns true if `ty` has any *open* components that prevent a definitive
/// HRTB verdict: an inference variable, a generic type parameter, a
/// late-bound placeholder, an early-bound region parameter, or an inference
/// region. A `Placeholder` region (introduced by HRTB instantiation) is
/// *not* open — it is fully resolved — so `HAS_RE_PLACEHOLDER` is deliberately
/// excluded. Such types cannot be discharged without further context, so the
/// caller should remain `Ambiguous`. Owned/scalar types (which contain no
/// such components) return `false` and are trivially well-formed / outlive
/// every region.
fn ty_has_open_components(ty: Ty, ctx: &TyCtx) -> bool {
    let flags = ctx.ty_flags(ty);
    flags.intersects(
        TypeFlags::HAS_TY_INFER
            | TypeFlags::HAS_TY_PARAM
            | TypeFlags::HAS_TY_PLACEHOLDER
            | TypeFlags::HAS_RE_INFER
            | TypeFlags::HAS_RE_PARAM
            | TypeFlags::HAS_ERROR,
    )
}

/// Returns true if `ty` is concrete and trivially well-formed for HRTB
/// purposes: no open type/region components (no inference vars, generic
/// params, late-bound placeholders, or region variables), and not a
/// `dyn`/projection type whose where-clauses are uncheckable here.
fn ty_is_concrete_well_formed(ty: Ty, ctx: &TyCtx) -> bool {
    if ty_has_open_components(ty, ctx) {
        return false;
    }
    // `dyn Trait` / `impl Trait` (projection) carry where-clauses that this
    // cheap HRTB check cannot verify, so treat them as not-yet-proven.
    !matches!(
        ctx.ty_kind(ty),
        TyKind::Dynamic(_, _) | TyKind::Projection(_)
    )
}

/// Check whether a higher-ranked trait bound is satisfied.
///
/// This function:
/// 1. Instantiates the binder with placeholders in `ctx_mut`
/// 2. Freezes `ctx_mut` into a `TyCtx`
/// 3. Checks the instantiated predicate against the solver
///
/// **Important:** This consumes `ctx_mut` (via freeze) because the solver
/// requires a frozen `TyCtx`. Returns `(SolverResult, TyCtx)` so the
/// caller can reuse the frozen context.
pub fn check_hrtb(
    binder: &Binder<Predicate>,
    solver: &mut dyn crate::solver::TraitSolver,
    infer: &mut crate::InferenceTable,
    mut ctx_mut: TyCtxMut,
) -> (crate::solver::SolverResult, TyCtx) {
    // Instantiate the binder with placeholders in the caller's context
    let instantiation = instantiate_binder_with_placeholders(binder, infer, &mut ctx_mut);

    // Freeze the context so the solver can read the types
    let ctx = ctx_mut.freeze();

    let result = match &instantiation.value {
        Predicate::Trait(tp) => solver.can_prove(&ctx, tp),
        Predicate::RegionOutlives(rp) => {
            // Check that the placeholder regions satisfy outlives.
            // For HRTB, we need to ensure that for all regions, a outlives b.
            // Cheap-win cases that are *trivially* provable everywhere:
            //   * reflexivity (`a == b`) — every region outlives itself;
            //   * either side is `'static` — `'static` outlives everything and
            //     is outlived by everything.
            // Anything genuinely open (two distinct placeholders or a
            // placeholder vs. an unrelated early-bound region) stays
            // Ambiguous rather than being falsely proven.
            match (&rp.a, &rp.b) {
                _ if rp.a == rp.b => crate::solver::SolverResult::Proven,
                (Region::Placeholder(_), Region::Static) => crate::solver::SolverResult::Proven,
                (Region::Static, Region::Placeholder(_)) => crate::solver::SolverResult::Proven,
                (Region::Static, Region::Static) => crate::solver::SolverResult::Proven,
                _ => crate::solver::SolverResult::Ambiguous,
            }
        }
        Predicate::TypeOutlives(tp) => {
            // `T: 'r` is provable when `'r` is `'static`, or when `T` *contains*
            // `'r` (the bound is reflexive), or when `T` is an owned/scalar type
            // that contains no non-static regions at all (it trivially
            // outlives every region). It stays Ambiguous when `T` carries
            // unresolved inference variables or regions it does not itself
            // contain, since those cannot be discharged under HRTB.
            if matches!(tp.region, Region::Static) || region_in_ty(&tp.region, tp.ty, &ctx) {
                crate::solver::SolverResult::Proven
            } else {
                // No inference/placeholder/param type vars, and no region
                // variables anywhere -> owned type, provably outlives all.
                if !ty_has_open_components(tp.ty, &ctx) {
                    crate::solver::SolverResult::Proven
                } else {
                    crate::solver::SolverResult::Ambiguous
                }
            }
        }
        Predicate::WellFormed(ty) => {
            // Well-formedness of a concrete type (no generic params, no
            // late-bound placeholders, no inference variables, no
            // `dyn`/projection components whose bounds are uncheckable) is not
            // actually ambiguous — it is simply true. Only types with open
            // components (unresolved inference vars, generic params whose
            // bounds haven't been checked, or `dyn`/projection positions whose
            // where-clauses are pending) remain Ambiguous.
            if ty_is_concrete_well_formed(*ty, &ctx) {
                crate::solver::SolverResult::Proven
            } else {
                crate::solver::SolverResult::Ambiguous
            }
        }
        Predicate::Coerce(a, b) => {
            // Identity coercion (`a` and `b` are structurally equal types) is
            // always valid. Note: `Ty` equality is by interned index, and
            // HRTB instantiation re-interns substituted types, so two
            // structurally identical types may carry different `Ty` handles.
            // Compare via `TyKind` (structural) rather than relying on
            // `can_coerce`'s index-based `a == b` identity check. Genuinely
            // open higher-ranked coercions (and non-identity coercions the
            // existing rules reject) stay Ambiguous.
            if ty_struct_eq(*a, *b, &ctx, 0) || crate::solver::can_coerce(&ctx, *a, *b) {
                crate::solver::SolverResult::Proven
            } else {
                crate::solver::SolverResult::Ambiguous
            }
        }
    };

    (result, ctx)
}

/// instantiate_hrtb_predicate.
pub fn instantiate_hrtb_predicate(
    binder: &Binder<Predicate>,
    infer: &mut crate::InferenceTable,
    ctx: &mut TyCtxMut,
) -> PlaceholderInstantiation<Predicate> {
    instantiate_binder_with_placeholders(binder, infer, ctx)
}

#[cfg(test)]
mod tests {
    use super::*;
    use glyim_core::primitives::Mutability;

    #[test]
    fn test_placeholder_region_creation() {
        let mut ctx = glyim_test::test_ty_ctx();
        let mut infer = crate::InferenceTable::new();

        let bound_vars: Box<[BoundVariableKind]> = Box::new([
            BoundVariableKind::Region(BoundRegionKind::BrAnon(0)),
            BoundVariableKind::Region(BoundRegionKind::BrAnon(1)),
        ]);

        let binder = Binder::bind(
            Region::LateBound(DebruijnIndex::INNERMOST, 0, BoundRegionKind::BrAnon(0)),
            bound_vars,
        );

        let inst = instantiate_binder_with_placeholders(&binder, &mut infer, &mut ctx);
        assert_eq!(inst.placeholders.len(), 2);
        assert_eq!(inst.universe, UniverseIndex(1));
    }

    #[test]
    fn test_substitute_bound_region_in_ref() {
        let mut ctx = glyim_test::test_ty_ctx();
        let mut infer = crate::InferenceTable::new();

        let bound_vars: Box<[BoundVariableKind]> =
            Box::new([BoundVariableKind::Region(BoundRegionKind::BrAnon(0))]);

        let bound_region =
            Region::LateBound(DebruijnIndex::INNERMOST, 0, BoundRegionKind::BrAnon(0));
        let i32_ty = ctx.mk_ty(TyKind::Int(glyim_core::primitives::IntTy::I32));
        let ref_ty = ctx.mk_ref(bound_region.clone(), i32_ty, Mutability::Not);

        let binder = Binder::bind(ref_ty, bound_vars);
        let inst = instantiate_binder_with_placeholders(&binder, &mut infer, &mut ctx);

        match ctx.ty_kind(inst.value) {
            TyKind::Ref(region, _, _) => {
                assert!(
                    matches!(region, Region::Placeholder(_)),
                    "Bound region should be replaced with placeholder, got {:?}",
                    region
                );
            }
            other => panic!("Expected Ref type, got {:?}", other),
        }
    }

    #[test]
    fn test_substitute_preserves_static_region() {
        let mut ctx = glyim_test::test_ty_ctx();
        let mut infer = crate::InferenceTable::new();

        let bound_vars: Box<[BoundVariableKind]> =
            Box::new([BoundVariableKind::Region(BoundRegionKind::BrAnon(0))]);

        let i32_ty = ctx.mk_ty(TyKind::Int(glyim_core::primitives::IntTy::I32));
        let ref_ty = ctx.mk_ref(Region::Static, i32_ty, Mutability::Not);

        let binder = Binder::bind(ref_ty, bound_vars);
        let inst = instantiate_binder_with_placeholders(&binder, &mut infer, &mut ctx);

        match ctx.ty_kind(inst.value) {
            TyKind::Ref(region, _, _) => {
                assert!(
                    matches!(region, Region::Static),
                    "Static region should be preserved, got {:?}",
                    region
                );
            }
            other => panic!("Expected Ref type, got {:?}", other),
        }
    }
}
