use crate::*;
use glyim_core::def_id::{AdtId, ClosureId};
use glyim_test::with_fresh_ty_ctx;
use glyim_type::{GenericArg, Ty, TyCtxMut};

#[test]
fn aggregate_kind_array() {
    let kind = AggregateKind::Array(Ty::BOOL);
    assert!(matches!(kind, AggregateKind::Array(Ty::BOOL)));
}

#[test]
fn aggregate_kind_tuple() {
    let kind = AggregateKind::Tuple;
    assert!(matches!(kind, AggregateKind::Tuple));
}

#[test]
fn aggregate_kind_adt() {
    let (_ctx, substs) = with_fresh_ty_ctx(|c: &mut TyCtxMut| {
        let bool_ty = c.bool_ty();
        c.intern_substitution(vec![GenericArg::Ty(bool_ty)])
    });
    let adt_id = AdtId::from_usize(42);
    let variant = VariantIdx::from_usize(0);
    let kind = AggregateKind::Adt(adt_id, variant, substs);
    assert!(matches!(kind, AggregateKind::Adt(id, v, _) if id == adt_id && v == variant));
}

#[test]
fn aggregate_kind_closure() {
    let (_ctx, substs) = with_fresh_ty_ctx(|c: &mut TyCtxMut| {
        let bool_ty = c.bool_ty();
        c.intern_substitution(vec![GenericArg::Ty(bool_ty)])
    });
    let closure_id = ClosureId::from_usize(7);
    let kind = AggregateKind::Closure(closure_id, substs);
    assert!(matches!(kind, AggregateKind::Closure(id, _) if id == closure_id));
}
