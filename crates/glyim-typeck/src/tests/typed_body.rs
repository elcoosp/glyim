//! Typed THIR boundary tests (plan `M5.2`, `docs/roadmaps/glyim-master-plan.md`
//! §6 `M5.2` `DoD`: "`TypedBody` holds `ResolvedTy` (no inference vars, no error
//! types); `zonk -> Result`; construction API private | lowering accepts only
//! `TypedBody`; error-arm sentinel lowering deleted").
//!
//! The first test pins the acceptance criterion directly: a body whose slots
//! still carry the post-zonk `Ty::ERROR` sentinel (the folded form of an
//! unbound `Infer(Ty(_))`) or a raw inference variable must be **rejected**
//! by the checked constructor instead of flowing into MIR lowering, where it
//! historically reached codegen as the `UnknownType(Ty(0))` ICE inside ADT
//! substitutions (F10 sentinel class). The second test pins `zonk -> Result`:
//! zonking a body with an unbound inference variable reports the failure
//! instead of silently folding it to `Ty::ERROR`.

use glyim_core::def_id::{CrateId, DefId, LocalDefId};
use glyim_solve::InferenceTable;
use glyim_span::Span;
use glyim_test::test_ty_ctx;
use glyim_type::{InferVar, IntVar, Ty, TyKind};

use crate::thir;
use crate::typed_body::TypedBody;
use crate::zonk;

/// `fn f() { 1; }` as a hand-built THIR body over `return_ty`.
fn body_with_return_ty(return_ty: Ty) -> thir::Body {
    thir::Body {
        owner: DefId::new(CrateId::from_raw(0), LocalDefId::from_raw(0)),
        params: Vec::new(),
        return_ty,
        stmts: vec![thir::Stmt::Expr {
            expr: thir::Expr {
                kind: thir::ExprKind::Literal(thir::Literal::Int(1, None)),
                ty: Ty::I32,
                span: Span::DUMMY,
            },
        }],
        span: Span::DUMMY,
    }
}

/// Acceptance criterion (plan `M5.2`): the post-zonk `Ty::ERROR` sentinel —
/// the folded form of an unbound inference variable, and the type the
/// lowering `error_ty()` fallbacks historically fabricated — must make
/// construction fail. A `TypedBody` cannot be built carrying it.
#[test]
fn typed_body_rejects_error_sentinel_slots() {
    let ctx_mut = test_ty_ctx();
    let frozen = ctx_mut.freeze();
    let body = body_with_return_ty(Ty::ERROR);
    let err = TypedBody::try_from_body(body, &frozen)
        .expect_err("Ty::ERROR slots must never reach lowering (plan `M5.2`: no error types)");
    assert_eq!(err.ty, Ty::ERROR);
    assert_eq!(err.span, Span::DUMMY);
}

/// A raw inference variable (never folded) is rejected the same way: an
/// unzonked body is not a `TypedBody`.
#[test]
fn typed_body_rejects_raw_inference_vars() {
    let mut ctx_mut = test_ty_ctx();
    let mut infer = InferenceTable::new();
    let var = infer.new_ty_var(&mut ctx_mut);
    let unbound = ctx_mut.mk_ty(TyKind::Infer(InferVar::Ty(var)));
    let frozen = ctx_mut.freeze();
    let body = body_with_return_ty(unbound);
    assert!(
        TypedBody::try_from_body(body, &frozen).is_err(),
        "raw inference variables must never reach lowering (plan `M5.2`: no \
         inference vars)"
    );
}

/// A fully resolved body constructs cleanly, and its signature slots are
/// exposed as `ResolvedTy` (plan `M5.2`: "`TypedBody` holds `ResolvedTy`").
#[test]
fn typed_body_accepts_fully_resolved_body() {
    let ctx_mut = test_ty_ctx();
    let frozen = ctx_mut.freeze();
    let body = body_with_return_ty(Ty::I32);
    let typed =
        TypedBody::try_from_body(body, &frozen).expect("a concrete-i32 body is fully resolved");
    assert_eq!(typed.return_ty().get(), Ty::I32);
    assert_eq!(typed.param_tys().count(), 0);
    assert_eq!(typed.get().stmts.len(), 1);
}

/// `zonk -> Result` (plan `M5.2`): zonking a body whose unbound `Infer(Ty(_))`
/// folds to `Ty::ERROR` must *report* the body instead of silently handing
/// the sentinel downstream. The `Err` payload is keyed by owner so the
/// pipeline can keep its existing reachability-gated diagnostic path.
#[test]
fn zonk_reports_unbound_type_vars_as_err() {
    let mut ctx_mut = test_ty_ctx();
    let mut infer = InferenceTable::new();
    let var = infer.new_ty_var(&mut ctx_mut);
    let unbound = ctx_mut.mk_ty(TyKind::Infer(InferVar::Ty(var)));
    let frozen = ctx_mut.freeze();
    let owner = LocalDefId::from_raw(0);
    let mut bodies = vec![(owner, body_with_return_ty(unbound))];
    let err = zonk::zonk_bodies(&infer, &frozen, &mut bodies)
        .expect_err("an unbound type variable must fail the checked zonk");
    assert!(err.contains_key(&owner), "the offending owner is reported");
    // A body with only concrete types zonks to `Ok`.
    let mut clean = vec![(LocalDefId::from_raw(1), body_with_return_ty(Ty::I32))];
    assert!(zonk::zonk_bodies(&infer, &frozen, &mut clean).is_ok());
}

/// Unbound *integer* inference variables are folded to their fallback (`i32`)
/// by zonk and must not reject the body — the existing `add_one(41)` literal
/// path depends on that fold. Only genuinely unresolved (`Ty::ERROR`) slots
/// are reported.
#[test]
fn zonk_folds_int_vars_without_rejecting() {
    let mut ctx_mut = test_ty_ctx();
    let mut infer = InferenceTable::new();
    let var = infer.new_int_var(&mut ctx_mut);
    let int_ty = ctx_mut.mk_ty(TyKind::Infer(InferVar::Int(var)));
    let frozen = ctx_mut.freeze();
    let mut bodies = vec![(LocalDefId::from_raw(2), body_with_return_ty(int_ty))];
    assert!(
        zonk::zonk_bodies(&infer, &frozen, &mut bodies).is_ok(),
        "int vars fold to their fallback; the body stays clean"
    );
    // Pin the `InferVar::Int` variant construction used above.
    let _ = InferVar::Int(IntVar::from_raw(0));
}
