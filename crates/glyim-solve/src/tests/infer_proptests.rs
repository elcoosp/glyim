//! Property tests for the unification foundation (plan §6 M3.10).
//!
//! Plan acceptance line (M3.10): *"Property tests for foundations:
//! `IndexVec`, interner, spans, `Size/Align`, substitution, unification"*
//! with the `DoD` `≥ 200 properties; run in PR tier`. This module pins the
//! [`InferenceTable`] unification laws over a generated palette of
//! concrete and infer-variable types: reflexivity, symmetry, the
//! var-binding and probe contract, and the occurs check — the algebra
//! every generic-call instantiation and trait obligation rests on (the
//! hand-picked cases in `unification.rs` stay as regression pins).
//!
//! Runs in the default `cargo test` PR tier; no LLVM dependency.

use crate::InferenceTable;
use glyim_core::primitives::{FloatTy, IntTy, UintTy};
use glyim_test::test_ty_ctx;
use glyim_type::{InferVar, Ty, TyCtxMut, TyKind};
use proptest::prelude::*;

/// The concrete-type palette: distinct primitive kinds and widths the
/// pairwise unification laws range over. `Int`/`Uint`/`Float` widths are
/// the interesting axis (same-kind-different-width must refuse); the
/// unit-shaped kinds (`Unit`, `Bool`, `Char`, `String`, `Never`) cover the
/// nullary arms.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Concrete {
    I8,
    I32,
    I64,
    U8,
    U32,
    U64,
    F32,
    F64,
    Bool,
    Char,
    String,
    Never,
}

/// A generated type: either a palette kind or a fresh inference variable.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Pal {
    Concrete(Concrete),
    FreshVar,
    Error,
}

/// Mint the palette type in `ctx` (fresh handles each call; the *kind*
/// identity is what the laws are about — handle equality is only ever
/// compared between values minted for the same law run).
fn pal_ty(ctx: &mut TyCtxMut, pal: Pal, infer: &mut InferenceTable) -> Ty {
    match pal {
        Pal::Concrete(Concrete::I8) => ctx.mk_ty(TyKind::Int(IntTy::I8)),
        Pal::Concrete(Concrete::I32) => ctx.mk_ty(TyKind::Int(IntTy::I32)),
        Pal::Concrete(Concrete::I64) => ctx.mk_ty(TyKind::Int(IntTy::I64)),
        Pal::Concrete(Concrete::U8) => ctx.mk_ty(TyKind::Uint(UintTy::U8)),
        Pal::Concrete(Concrete::U32) => ctx.mk_ty(TyKind::Uint(UintTy::U32)),
        Pal::Concrete(Concrete::U64) => ctx.mk_ty(TyKind::Uint(UintTy::U64)),
        Pal::Concrete(Concrete::F32) => ctx.mk_ty(TyKind::Float(FloatTy::F32)),
        Pal::Concrete(Concrete::F64) => ctx.mk_ty(TyKind::Float(FloatTy::F64)),
        Pal::Concrete(Concrete::Bool) => ctx.mk_ty(TyKind::Bool),
        Pal::Concrete(Concrete::Char) => ctx.mk_ty(TyKind::Char),
        Pal::Concrete(Concrete::String) => ctx.mk_ty(TyKind::String),
        Pal::Concrete(Concrete::Never) => ctx.mk_ty(TyKind::Never),
        Pal::FreshVar => {
            let var = infer.new_ty_var(ctx);
            ctx.mk_ty(TyKind::Infer(InferVar::Ty(var)))
        }
        Pal::Error => Ty::ERROR,
    }
}

/// The strictly-bindable subset: concrete kinds that install a value into
/// a general var. `Never`/`ERROR` are excluded — they take the
/// context-free absorb arm and never bind.
fn bindable() -> impl Strategy<Value = Pal> {
    prop_oneof![
        1 => Just(Pal::Concrete(Concrete::I8)),
        1 => Just(Pal::Concrete(Concrete::I32)),
        1 => Just(Pal::Concrete(Concrete::I64)),
        1 => Just(Pal::Concrete(Concrete::U8)),
        1 => Just(Pal::Concrete(Concrete::U32)),
        1 => Just(Pal::Concrete(Concrete::U64)),
        1 => Just(Pal::Concrete(Concrete::F32)),
        1 => Just(Pal::Concrete(Concrete::F64)),
        1 => Just(Pal::Concrete(Concrete::Bool)),
        1 => Just(Pal::Concrete(Concrete::Char)),
        1 => Just(Pal::Concrete(Concrete::String)),
    ]
}

fn palette() -> impl Strategy<Value = Pal> {
    prop_oneof![
        1 => Just(Pal::Concrete(Concrete::I8)),
        1 => Just(Pal::Concrete(Concrete::I32)),
        1 => Just(Pal::Concrete(Concrete::I64)),
        1 => Just(Pal::Concrete(Concrete::U8)),
        1 => Just(Pal::Concrete(Concrete::U32)),
        1 => Just(Pal::Concrete(Concrete::U64)),
        1 => Just(Pal::Concrete(Concrete::F32)),
        1 => Just(Pal::Concrete(Concrete::F64)),
        1 => Just(Pal::Concrete(Concrete::Bool)),
        1 => Just(Pal::Concrete(Concrete::Char)),
        1 => Just(Pal::Concrete(Concrete::String)),
        1 => Just(Pal::Concrete(Concrete::Never)),
        3 => Just(Pal::FreshVar),
        1 => Just(Pal::Error),
    ]
}

/// The kind-level verdict the laws compare against: two concrete palette
/// kinds unify iff they are the same kind (ERROR/Never are the
/// context-free arms that unify with anything; infer vars bind).
fn concrete_compat(a: Concrete, b: Concrete) -> bool {
    match (a, b) {
        (_, Concrete::Never) | (Concrete::Never, _) => true,
        (x, y) => x == y,
    }
}

proptest! {
    // ------------------------------------------------------------------
    // Laws over the concrete palette.
    // ------------------------------------------------------------------

    /// Reflexivity: every palette type unifies with itself — minted
    /// handles, sentinels and ERROR included (`a == b` short-circuit and
    /// the `Error` arm both agree).
    #[test]
    fn unify_is_reflexive(pal in palette()) {
        let mut ctx = test_ty_ctx();
        let mut infer = InferenceTable::new();
        let ty = pal_ty(&mut ctx, pal, &mut infer);
        prop_assert!(infer.unify(&mut ctx, ty, ty, glyim_span::Span::DUMMY).is_ok());
    }

    /// Symmetry: for every palette pair, `a ~ b` succeeds iff `b ~ a`
    /// does (same kind ⇔ same kind; width mismatches refuse both ways;
    /// `Never`/`ERROR` absorb both ways; vars bind both ways).
    #[test]
    fn unify_is_symmetric(a in palette(), b in palette()) {
        let mut ctx = test_ty_ctx();
        let mut infer = InferenceTable::new();
        let ta = pal_ty(&mut ctx, a, &mut infer);
        let tb = pal_ty(&mut ctx, b, &mut infer);
        let forward = infer.unify(&mut ctx, ta, tb, glyim_span::Span::DUMMY).is_ok();

        let mut ctx = test_ty_ctx();
        let mut infer = InferenceTable::new();
        let ta = pal_ty(&mut ctx, a, &mut infer);
        let tb = pal_ty(&mut ctx, b, &mut infer);
        let backward = infer.unify(&mut ctx, tb, ta, glyim_span::Span::DUMMY).is_ok();

        prop_assert_eq!(forward, backward, "symmetry: {:?} ~ {:?}", a, b);
    }

    /// Concrete kind agreement is the compatibility criterion: identical
    /// kinds succeed; distinct kinds fail; `Never` and `ERROR` unify with
    /// everything (the context-free arms).
    #[test]
    fn concrete_pairs_follow_kind_identity(a in palette(), b in palette()) {
        prop_assume!(
            matches!(a, Pal::Concrete(_)) && matches!(b, Pal::Concrete(_)),
            "this law is about the concrete arms only"
        );
        let (ka, kb) = match (a, b) {
            (Pal::Concrete(ka), Pal::Concrete(kb)) => (ka, kb),
            _ => unreachable!("assumed above"),
        };
        let mut ctx = test_ty_ctx();
        let mut infer = InferenceTable::new();
        let ta = pal_ty(&mut ctx, a, &mut infer);
        let tb = pal_ty(&mut ctx, b, &mut infer);
        let verdict = infer.unify(&mut ctx, ta, tb, glyim_span::Span::DUMMY);
        prop_assert_eq!(verdict.is_ok(), concrete_compat(ka, kb), "{:?} vs {:?}", ka, kb);
    }

    // ------------------------------------------------------------------
    // Var binding: new var, bind, probe — the inference contract.
    // ------------------------------------------------------------------

    /// A fresh var is unbound (`probe_ty_var == None`); after unifying
    /// with a palette concrete the probe returns exactly that concrete —
    /// regardless of which side of the unify the var stood on.
    #[test]
    fn var_binds_to_concrete_from_either_side(concrete in bindable()) {
        prop_assume!(matches!(concrete, Pal::Concrete(_)), "bind to a concrete");
        let mut ctx = test_ty_ctx();
        let mut infer = InferenceTable::new();
        let var = infer.new_ty_var(&mut ctx);
        let var_ty = ctx.mk_ty(TyKind::Infer(InferVar::Ty(var)));
        prop_assert_eq!(infer.probe_ty_var(var), None, "fresh var is unbound");
        let concrete_ty = pal_ty(&mut ctx, concrete, &mut infer);
        infer
            .unify(&mut ctx, var_ty, concrete_ty, glyim_span::Span::DUMMY)
            .unwrap_or_else(|diags| {
                panic!("forward bind refused: {diags:?}")
            });
        prop_assert_eq!(infer.probe_ty_var(var), Some(concrete_ty));

        let mut ctx = test_ty_ctx();
        let mut infer = InferenceTable::new();
        let var = infer.new_ty_var(&mut ctx);
        let var_ty = ctx.mk_ty(TyKind::Infer(InferVar::Ty(var)));
        let concrete_ty = pal_ty(&mut ctx, concrete, &mut infer);
        infer
            .unify(&mut ctx, concrete_ty, var_ty, glyim_span::Span::DUMMY)
            .unwrap_or_else(|diags| {
                panic!("mirrored bind refused: {diags:?}")
            });
        prop_assert_eq!(infer.probe_ty_var(var), Some(concrete_ty), "mirrored side");
    }

    /// After a bind, shallow resolution of the var's type is the bound
    /// concrete (the read-back consumers use), and `probe` is a pure
    /// read: probing twice never changes the answer.
    #[test]
    fn resolve_and_probe_are_stable(concrete in bindable()) {
        prop_assume!(matches!(concrete, Pal::Concrete(_)), "bind to a concrete");
        let mut ctx = test_ty_ctx();
        let mut infer = InferenceTable::new();
        let var = infer.new_ty_var(&mut ctx);
        let var_ty = ctx.mk_ty(TyKind::Infer(InferVar::Ty(var)));
        let concrete_ty = pal_ty(&mut ctx, concrete, &mut infer);
        infer
            .unify(&mut ctx, var_ty, concrete_ty, glyim_span::Span::DUMMY)
            .unwrap_or_else(|diags| {
                panic!("bind refused: {diags:?}")
            });
        let resolved = infer.resolve_ty_shallow(&ctx, var_ty);
        prop_assert_eq!(resolved, concrete_ty);
        prop_assert_eq!(infer.probe_ty_var(var), Some(concrete_ty));
        prop_assert_eq!(infer.probe_ty_var(var), Some(concrete_ty), "probe is pure");
    }

    /// Two fresh vars unify (both stay unbound-or-aliased without a
    /// concrete), and a fresh var also unifies with `ERROR` (the
    /// context-free arm must not mint diagnostics that abort the caller).
    #[test]
    fn vars_and_error_arms_unify(a in palette()) {
        let mut ctx = test_ty_ctx();
        let mut infer = InferenceTable::new();
        let v1 = infer.new_ty_var(&mut ctx);
        let v2 = infer.new_ty_var(&mut ctx);
        let t1 = ctx.mk_ty(TyKind::Infer(InferVar::Ty(v1)));
        let t2 = ctx.mk_ty(TyKind::Infer(InferVar::Ty(v2)));
        prop_assert!(infer
            .unify(&mut ctx, t1, t2, glyim_span::Span::DUMMY)
            .is_ok());
        let err_ty = pal_ty(&mut ctx, Pal::Error, &mut infer);
        let pa = pal_ty(&mut ctx, a, &mut infer);
        prop_assert!(infer
            .unify(&mut ctx, pa, err_ty, glyim_span::Span::DUMMY)
            .is_ok());
    }

    /// Distinct int-var/float-var slots: an int var binds int-width kinds
    /// and refuses float/bool kinds, symmetrically (the narrowed var
    /// lattice the literal fallback relies on).
    #[test]
    fn int_var_lattice(
        int_width in prop_oneof![Just(IntTy::I8), Just(IntTy::I32), Just(IntTy::I64)],
        other in prop_oneof![Just(Concrete::F32), Just(Concrete::F64), Just(Concrete::Bool)],
    ) {
        let mut ctx = test_ty_ctx();
        let mut infer = InferenceTable::new();
        let ivar = infer.new_int_var(&mut ctx);
        let ivar_ty = ctx.mk_ty(TyKind::Infer(InferVar::Int(ivar)));
        let int_ty = ctx.mk_ty(TyKind::Int(int_width));
        infer
            .unify(&mut ctx, ivar_ty, int_ty, glyim_span::Span::DUMMY)
            .unwrap_or_else(|diags| {
                panic!("int bind refused: {diags:?}")
            });
        prop_assert_eq!(infer.probe_int_var(ivar), Some(int_ty), "int kind binds");

        let mut ctx = test_ty_ctx();
        let mut infer = InferenceTable::new();
        let ivar = infer.new_int_var(&mut ctx);
        let ivar_ty = ctx.mk_ty(TyKind::Infer(InferVar::Int(ivar)));
        let other_ty = match other {
            Concrete::F32 => ctx.mk_ty(TyKind::Float(FloatTy::F32)),
            Concrete::F64 => ctx.mk_ty(TyKind::Float(FloatTy::F64)),
            Concrete::Bool => ctx.mk_ty(TyKind::Bool),
            _ => unreachable!("strategy emits only the three listed kinds"),
        };
        prop_assert!(infer
            .unify(&mut ctx, ivar_ty, other_ty, glyim_span::Span::DUMMY)
            .is_err(), "{other:?} must refuse an int var");
        prop_assert_eq!(infer.probe_int_var(ivar), None, "refusal leaves it unbound");
    }

    // ------------------------------------------------------------------
    // Occurs check: infinite types are refused, never constructed.
    // ------------------------------------------------------------------

    /// Unifying a fresh var with a type *containing* that var (`?T ~
    /// Slice(?T)`) is refused with an error, and the var stays unbound —
    /// the infinite-type guard (plan §5.2, the M9 memory-model class).
    #[test]
    fn occurs_check_refuses_infinite_types(depth in 1_usize..4) {
        let mut ctx = test_ty_ctx();
        let mut infer = InferenceTable::new();
        let var = infer.new_ty_var(&mut ctx);
        let var_ty = ctx.mk_ty(TyKind::Infer(InferVar::Ty(var)));
        // The container is `Slice^k(?T)` — the var genuinely occurs inside.
        let mut container = var_ty;
        for _ in 0..depth {
            container = ctx.mk_ty(TyKind::Slice(container));
        }
        let verdict = infer.unify(&mut ctx, var_ty, container, glyim_span::Span::DUMMY);
        prop_assert!(verdict.is_err(), "occurs check must refuse ?T ~ Slice^k(?T)");
        prop_assert_eq!(infer.probe_ty_var(var), None, "refusal leaves the var unbound");
    }

    /// The occurs check only fires when the var genuinely occurs: a var
    /// against a container of an *unrelated* concrete still succeeds and
    /// binds through the container (the negative case for the guard).
    #[test]
    fn no_occurs_no_refusal(concrete in palette()) {
        prop_assume!(matches!(concrete, Pal::Concrete(_)), "container of a concrete");
        let mut ctx = test_ty_ctx();
        let mut infer = InferenceTable::new();
        let var = infer.new_ty_var(&mut ctx);
        let var_ty = ctx.mk_ty(TyKind::Infer(InferVar::Ty(var)));
        let inner = pal_ty(&mut ctx, concrete, &mut infer);
        let container = ctx.mk_ty(TyKind::Slice(inner));
        infer
            .unify(&mut ctx, var_ty, container, glyim_span::Span::DUMMY)
            .unwrap_or_else(|diags| {
                panic!("container bind refused: {diags:?}")
            });
        prop_assert_eq!(infer.probe_ty_var(var), Some(container));
    }

    /// Snapshots restore pre-snapshot slot *values* without shrinking the
    /// var space (the §3.10 dangling-handle fix): a bind made after the
    /// snapshot on a pre-existing var is undone by `rollback_to`; a var
    /// allocated *after* the snapshot stays addressable (the probe never
    /// panics — it keeps its post-snapshot value, which rollback does not
    /// cover); a pre-snapshot bind survives the rollback.
    #[test]
    fn snapshots_preserve_post_snapshot_vars(concrete in bindable()) {
        prop_assume!(matches!(concrete, Pal::Concrete(_)), "bind to a concrete");
        let mut ctx = test_ty_ctx();
        let mut infer = InferenceTable::new();
        let early = infer.new_ty_var(&mut ctx);
        let concrete_ty = pal_ty(&mut ctx, concrete, &mut infer);
        let early_ty = ctx.mk_ty(TyKind::Infer(InferVar::Ty(early)));
        infer
            .unify(&mut ctx, early_ty, concrete_ty, glyim_span::Span::DUMMY)
            .unwrap_or_else(|diags| panic!("early bind refused: {diags:?}"));
        // Snapshot captures the early bind; a second concrete mints fresh.
        let snapshot = infer.snapshot();
        let second = pal_ty(&mut ctx, concrete, &mut infer);
        infer
            .unify(&mut ctx, early_ty, second, glyim_span::Span::DUMMY)
            .unwrap_or_else(|diags| panic!("rebind refused: {diags:?}"));
        prop_assert_eq!(infer.probe_ty_var(early), Some(second));
        infer.rollback_to(snapshot);
        prop_assert_eq!(
            infer.probe_ty_var(early),
            Some(concrete_ty),
            "the pre-snapshot bind survives; the post-snapshot rebind is undone"
        );
        // A var minted after the snapshot stays addressable (the §3.10
        // dangling-handle class): probing it must not panic.
        let late = infer.new_ty_var(&mut ctx);
        prop_assert_eq!(infer.probe_ty_var(late), None, "fresh late var probes");
    }
}

/// An int var also accepts a `Uint` kind and an unbound *general* var
/// binds through to the int var (the SOLVE-1 aliasing shape), while a
/// `Param` never satisfies an int var. Pinned as a fixed-vector test: the
/// shapes are categorical, not generated.
#[test]
fn int_var_accepts_uint_and_general_var() {
    let mut ctx = test_ty_ctx();
    let mut infer = InferenceTable::new();
    let ivar = infer.new_int_var(&mut ctx);
    let ivar_ty = ctx.mk_ty(TyKind::Infer(InferVar::Int(ivar)));
    let uint_ty = ctx.mk_ty(TyKind::Uint(UintTy::U8));
    infer
        .unify(&mut ctx, ivar_ty, uint_ty, glyim_span::Span::DUMMY)
        .unwrap_or_else(|diags| panic!("uint bind refused: {diags:?}"));
    assert_eq!(infer.probe_int_var(ivar), Some(uint_ty));

    let mut ctx = test_ty_ctx();
    let mut infer = InferenceTable::new();
    let ivar = infer.new_int_var(&mut ctx);
    let ivar_ty = ctx.mk_ty(TyKind::Infer(InferVar::Int(ivar)));
    let general = infer.new_ty_var(&mut ctx);
    let general_ty = ctx.mk_ty(TyKind::Infer(InferVar::Ty(general)));
    infer
        .unify(&mut ctx, ivar_ty, general_ty, glyim_span::Span::DUMMY)
        .unwrap_or_else(|diags| panic!("general-var bind refused: {diags:?}"));
    // The general var was bound to the int var's type (the SOLVE-1
    // aliasing shape); resolving the chain ends at an integer kind — an
    // unbound int var defaults (resolve_ty_shallow's literal fallback),
    // so the resolved head must be a resolved `Int` kind, never a
    // dangling `Infer`.
    let bound = infer.resolve_ty_shallow(&ctx, general_ty);
    assert!(
        matches!(ctx.ty_kind(bound).clone(), TyKind::Int(_)),
        "the general var resolves through the int var to a defaulted Int, got {bound:?}"
    );
    assert_eq!(
        infer.probe_ty_var(general),
        Some(ivar_ty),
        "the alias installs the int var's type into the general var"
    );

    let mut ctx = test_ty_ctx();
    let mut infer = InferenceTable::new();
    let ivar = infer.new_int_var(&mut ctx);
    let ivar_ty = ctx.mk_ty(TyKind::Infer(InferVar::Int(ivar)));
    let param_name = ctx.resolver().intern("T");
    let param_ty = ctx.mk_ty(TyKind::Param(glyim_type::ParamTy {
        index: 0,
        name: param_name,
    }));
    assert!(
        infer
            .unify(&mut ctx, ivar_ty, param_ty, glyim_span::Span::DUMMY)
            .is_err(),
        "a Param never satisfies an int var"
    );
}

proptest! {
    /// `Never` and `ERROR` are absorbers: unifying a fresh var against
    /// either succeeds WITHOUT binding it (the context-free arms come
    /// before the binder in the match) — the exact semantics the
    /// `FnCtxt::unify` wrapper relies on to keep error paths flowing.
    #[test]
    fn absorbers_unify_without_binding(absorber in prop_oneof![Just(Pal::Error), Just(Pal::Concrete(Concrete::Never))]) {
        let mut ctx = test_ty_ctx();
        let mut infer = InferenceTable::new();
        let var = infer.new_ty_var(&mut ctx);
        let var_ty = ctx.mk_ty(TyKind::Infer(InferVar::Ty(var)));
        let absorb_ty = pal_ty(&mut ctx, absorber, &mut infer);
        prop_assert!(infer
            .unify(&mut ctx, var_ty, absorb_ty, glyim_span::Span::DUMMY)
            .is_ok());
        prop_assert_eq!(infer.probe_ty_var(var), None, "absorbers never bind");
        prop_assert_eq!(
            infer.resolve_ty_shallow(&ctx, var_ty),
            var_ty,
            "the var resolves to itself (still free)"
        );
    }

    /// Int-width identity matrix: two `Int` types unify iff the widths
    /// are equal, in both argument orders (the width check the literal
    /// fallback depends on).
    #[test]
    fn int_width_matrix_is_exact(
        wa in prop_oneof![Just(IntTy::I8), Just(IntTy::I16), Just(IntTy::I32), Just(IntTy::I64)],
        wb in prop_oneof![Just(IntTy::I8), Just(IntTy::I16), Just(IntTy::I32), Just(IntTy::I64)],
    ) {
        let mut ctx = test_ty_ctx();
        let mut infer = InferenceTable::new();
        let ta = ctx.mk_ty(TyKind::Int(wa));
        let tb = ctx.mk_ty(TyKind::Int(wb));
        let verdict = infer.unify(&mut ctx, ta, tb, glyim_span::Span::DUMMY);
        prop_assert_eq!(verdict.is_ok(), wa == wb, "width mismatch {:?} vs {:?}", wa, wb);
    }

    /// A float var accepts float kinds and refuses int/bool kinds; after
    /// a refusal it stays unbound (mirrors the int-var lattice).
    #[test]
    fn float_var_lattice(
        width in prop_oneof![Just(FloatTy::F32), Just(FloatTy::F64)],
        other in prop_oneof![Just(Concrete::I32), Just(Concrete::U8), Just(Concrete::Bool)],
    ) {
        let mut ctx = test_ty_ctx();
        let mut infer = InferenceTable::new();
        let fvar = infer.new_float_var(&mut ctx);
        let fvar_ty = ctx.mk_ty(TyKind::Infer(InferVar::Float(fvar)));
        let width_ty = ctx.mk_ty(TyKind::Float(width));
        infer
            .unify(&mut ctx, fvar_ty, width_ty, glyim_span::Span::DUMMY)
            .unwrap_or_else(|diags| panic!("float bind refused: {diags:?}"));
        prop_assert_eq!(infer.probe_float_var(fvar), Some(width_ty));

        let mut ctx = test_ty_ctx();
        let mut infer = InferenceTable::new();
        let fvar = infer.new_float_var(&mut ctx);
        let fvar_ty = ctx.mk_ty(TyKind::Infer(InferVar::Float(fvar)));
        let other_ty = match other {
            Concrete::I32 => ctx.mk_ty(TyKind::Int(IntTy::I32)),
            Concrete::U8 => ctx.mk_ty(TyKind::Uint(UintTy::U8)),
            Concrete::Bool => ctx.mk_ty(TyKind::Bool),
            _ => unreachable!("strategy emits the three listed kinds"),
        };
        prop_assert!(infer
            .unify(&mut ctx, fvar_ty, other_ty, glyim_span::Span::DUMMY)
            .is_err());
        prop_assert_eq!(infer.probe_float_var(fvar), None, "refusal leaves it free");
    }

    /// Cross-var-kind unification refuses: an int var never satisfies a
    /// float var or vice versa (the narrowed lattices are disjoint).
    #[test]
    fn int_and_float_vars_are_disjoint(order in 0_u8..2) {
        let mut ctx = test_ty_ctx();
        let mut infer = InferenceTable::new();
        let ivar = infer.new_int_var(&mut ctx);
        let fvar = infer.new_float_var(&mut ctx);
        let ivar_ty = ctx.mk_ty(TyKind::Infer(InferVar::Int(ivar)));
        let fvar_ty = ctx.mk_ty(TyKind::Infer(InferVar::Float(fvar)));
        let verdict = if order == 0 {
            infer.unify(&mut ctx, ivar_ty, fvar_ty, glyim_span::Span::DUMMY)
        } else {
            infer.unify(&mut ctx, fvar_ty, ivar_ty, glyim_span::Span::DUMMY)
        };
        prop_assert!(verdict.is_err(), "int and float vars must be disjoint");
    }

    /// Occurs check scales with nesting depth: `?T ~ Slice(Slice(...Slice(?T)))`
    /// is refused at every depth, and the refusal leaves the var free.
    #[test]
    fn occurs_check_at_depth(depth in 2_usize..6) {
        let mut ctx = test_ty_ctx();
        let mut infer = InferenceTable::new();
        let var = infer.new_ty_var(&mut ctx);
        let var_ty = ctx.mk_ty(TyKind::Infer(InferVar::Ty(var)));
        let mut container = var_ty;
        for _ in 0..depth {
            container = ctx.mk_ty(TyKind::Slice(container));
        }
        let verdict = infer.unify(&mut ctx, var_ty, container, glyim_span::Span::DUMMY);
        let diags = verdict.unwrap_err();
        prop_assert!(
            diags.iter().any(|d| d.to_string().contains("infinite type")),
            "the refusal names the infinite-type class: {diags:?}"
        );
        prop_assert_eq!(infer.probe_ty_var(var), None, "refusal leaves the var free");
    }

    /// A rolled-back var returns to its pre-snapshot bind and keeps its
    /// bindability: re-unifying with the *same* concrete re-succeeds, and
    /// re-unifying with an incompatible one is refused without disturbing
    /// the restored bind (the candidate-impl probing contract).
    #[test]
    fn rollback_then_rebind(kind in bindable(), other in bindable()) {
        prop_assume!(matches!(kind, Pal::Concrete(_)) && matches!(other, Pal::Concrete(_)));
        let mut ctx = test_ty_ctx();
        let mut infer = InferenceTable::new();
        let var = infer.new_ty_var(&mut ctx);
        let var_ty = ctx.mk_ty(TyKind::Infer(InferVar::Ty(var)));
        let ta = pal_ty(&mut ctx, kind, &mut infer);
        let tb = pal_ty(&mut ctx, other, &mut infer);
        infer
            .unify(&mut ctx, var_ty, ta, glyim_span::Span::DUMMY)
            .unwrap_or_else(|diags| panic!("first bind refused: {diags:?}"));
        // Snapshot captures the first bind; a post-snapshot rebind attempt
        // may refuse (incompatible kinds) but never panics.
        let snapshot = infer.snapshot();
        let _ = infer.unify(&mut ctx, var_ty, tb, glyim_span::Span::DUMMY);
        infer.rollback_to(snapshot);
        prop_assert_eq!(infer.probe_ty_var(var), Some(ta), "rolled back to the first bind");
        // Re-unifying with the same kind succeeds and restores that bind;
        // an incompatible kind is refused and leaves the bind untouched.
        let same_kind = kind == other;
        let rebind = infer.unify(&mut ctx, var_ty, tb, glyim_span::Span::DUMMY);
        if same_kind {
            rebind.unwrap_or_else(|diags| panic!("compatible rebind refused: {diags:?}"));
            prop_assert_eq!(infer.probe_ty_var(var), Some(tb));
        } else {
            prop_assert!(rebind.is_err(), "incompatible rebind must refuse");
            prop_assert_eq!(
                infer.probe_ty_var(var),
                Some(ta),
                "the restored bind survives the refusal"
            );
        }
    }
}

/// `ERROR` is a uniform absorber across var kinds: the `(_, Error)` arm
/// precedes the int-var binder in the match, so an int var — like a
/// general var — unifies with `ERROR` without binding. Pinned as a
/// fixed-vector test: the shape is categorical, not generated.
#[test]
fn int_var_absorbs_error_without_binding() {
    let mut ctx = test_ty_ctx();
    let mut infer = InferenceTable::new();
    let ivar = infer.new_int_var(&mut ctx);
    let ivar_ty = ctx.mk_ty(TyKind::Infer(InferVar::Int(ivar)));
    let err_ty = Ty::ERROR;
    let verdict = infer.unify(&mut ctx, ivar_ty, err_ty, glyim_span::Span::DUMMY);
    assert!(verdict.is_ok(), "the absorber arm accepts, got {verdict:?}");
    assert_eq!(infer.probe_int_var(ivar), None, "ERROR never binds a var");
}
