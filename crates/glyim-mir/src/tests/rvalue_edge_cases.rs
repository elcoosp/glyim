use crate::*;
use glyim_core::def_id::{AdtId, ClosureId};
use glyim_core::primitives::{BinOp, UnOp};
use glyim_span::Span;
use glyim_test::with_fresh_ty_ctx;
use glyim_type::{GenericArg, Ty, TyCtxMut};

fn si() -> SourceInfo {
    SourceInfo::new(Span::DUMMY)
}

fn test_const_val() -> MirConst {
    MirConst {
        kind: MirConstKind::Int(1),
        ty: Ty::BOOL,
        span: Span::DUMMY,
    }
}

#[test]
fn rvalue_all_binary_ops() {
    let ops = [
        BinOp::Add,
        BinOp::Sub,
        BinOp::Mul,
        BinOp::Div,
        BinOp::Rem,
        BinOp::BitAnd,
        BinOp::BitOr,
        BinOp::BitXor,
        BinOp::Shl,
        BinOp::Shr,
        BinOp::Eq,
        BinOp::Ne,
        BinOp::Lt,
        BinOp::Gt,
    ];
    for op in ops {
        let lhs = Operand::Copy(Place::new(LocalIdx::from_usize(0)));
        let rhs = Operand::Copy(Place::new(LocalIdx::from_usize(1)));
        let rv = Rvalue::binary_op(op, lhs, rhs);
        assert!(matches!(rv, Rvalue::BinaryOp(o, _) if o == op));
    }
}

#[test]
fn rvalue_both_unary_ops() {
    let rv_not = Rvalue::UnaryOp(
        UnOp::Not,
        Operand::Copy(Place::new(LocalIdx::from_usize(0))),
    );
    assert!(matches!(rv_not, Rvalue::UnaryOp(UnOp::Not, _)));

    let rv_neg = Rvalue::UnaryOp(
        UnOp::Neg,
        Operand::Copy(Place::new(LocalIdx::from_usize(0))),
    );
    assert!(matches!(rv_neg, Rvalue::UnaryOp(UnOp::Neg, _)));
}

#[test]
fn rvalue_aggregate_array() {
    let rv = Rvalue::Aggregate(
        AggregateKind::Array(Ty::BOOL),
        vec![
            Operand::Constant(test_const_val()),
            Operand::Constant(test_const_val()),
        ],
    );
    if let Rvalue::Aggregate(AggregateKind::Array(ty), ops) = &rv {
        assert_eq!(*ty, Ty::BOOL);
        assert_eq!(ops.len(), 2);
    } else {
        panic!("Expected Aggregate Array");
    }
}

#[test]
fn rvalue_aggregate_adt() {
    let (_, substs) = with_fresh_ty_ctx(|c: &mut TyCtxMut| {
        let bool_ty = c.bool_ty();
        c.intern_substitution(vec![GenericArg::Ty(bool_ty)])
    });
    let adt_id = AdtId::from_usize(1);
    let variant = VariantIdx::from_usize(0);
    let rv = Rvalue::Aggregate(
        AggregateKind::Adt(adt_id, variant, substs),
        vec![Operand::Constant(test_const_val())],
    );
    assert!(
        matches!(rv, Rvalue::Aggregate(AggregateKind::Adt(id, v, _), _) if id == adt_id && v == variant)
    );
}

#[test]
fn rvalue_aggregate_closure() {
    let (_, substs) = with_fresh_ty_ctx(|c: &mut TyCtxMut| {
        let bool_ty = c.bool_ty();
        c.intern_substitution(vec![GenericArg::Ty(bool_ty)])
    });
    let closure_id = ClosureId::from_usize(3);
    let rv = Rvalue::Aggregate(AggregateKind::Closure(closure_id, substs), vec![]);
    assert!(matches!(rv, Rvalue::Aggregate(AggregateKind::Closure(id, _), _) if id == closure_id));
}

#[test]
fn rvalue_all_cast_kinds() {
    let kinds = [
        CastKind::IntToInt,
        CastKind::FloatToInt,
        CastKind::IntToFloat,
        CastKind::PtrToPtr,
        CastKind::FnPtrToPtr,
    ];
    for kind in kinds {
        let rv = Rvalue::Cast(
            kind,
            Operand::Copy(Place::new(LocalIdx::from_usize(0))),
            Ty::BOOL,
        );
        assert!(matches!(rv, Rvalue::Cast(k, _, _) if k == kind));
    }
}

#[test]
fn rvalue_nop_statement() {
    let stmt = Statement {
        kind: StatementKind::Nop,
        source_info: si(),
    };
    assert!(matches!(stmt.kind, StatementKind::Nop));
}
