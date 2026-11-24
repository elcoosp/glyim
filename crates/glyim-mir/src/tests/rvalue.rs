use crate::*;
use glyim_core::primitives::{BinOp, UnOp};
use glyim_span::Span;
use glyim_type::Ty;

fn test_const() -> MirConst {
    MirConst {
        kind: MirConstKind::Int(42),
        ty: Ty::BOOL,
        span: Span::DUMMY,
    }
}

#[test]
fn rvalue_use_copy() {
    let place = Place::new(LocalIdx::from_raw(0));
    let rv = Rvalue::Use(Operand::Copy(place));
    assert!(matches!(rv, Rvalue::Use(Operand::Copy(_))));
}

#[test]
fn rvalue_use_move() {
    let place = Place::new(LocalIdx::from_raw(0));
    let rv = Rvalue::Use(Operand::Move(place));
    assert!(matches!(rv, Rvalue::Use(Operand::Move(_))));
}

#[test]
fn rvalue_use_constant() {
    let rv = Rvalue::Use(Operand::Constant(test_const()));
    assert!(matches!(rv, Rvalue::Use(Operand::Constant(_))));
}

#[test]
fn rvalue_ref_shared() {
    let place = Place::new(LocalIdx::from_raw(1));
    let rv = Rvalue::Ref(place, BorrowKind::Shared);
    assert!(matches!(rv, Rvalue::Ref(_, BorrowKind::Shared)));
}

#[test]
fn rvalue_ref_mut() {
    let place = Place::new(LocalIdx::from_raw(2));
    let rv = Rvalue::Ref(
        place,
        BorrowKind::Mut {
            allow_two_phase_borrow: true,
        },
    );
    assert!(matches!(
        rv,
        Rvalue::Ref(
            _,
            BorrowKind::Mut {
                allow_two_phase_borrow: true
            }
        )
    ));
}

#[test]
fn rvalue_ref_unique() {
    let place = Place::new(LocalIdx::from_raw(3));
    let rv = Rvalue::Ref(place, BorrowKind::Unique);
    assert!(matches!(rv, Rvalue::Ref(_, BorrowKind::Unique)));
}

#[test]
fn rvalue_binary_op() {
    let lhs = Operand::Copy(Place::new(LocalIdx::from_raw(0)));
    let rhs = Operand::Copy(Place::new(LocalIdx::from_raw(1)));
    let rv = Rvalue::binary_op(BinOp::Add, lhs, rhs);
    assert!(matches!(rv, Rvalue::BinaryOp(BinOp::Add, _)));
}

#[test]
fn rvalue_unary_op() {
    let operand = Operand::Copy(Place::new(LocalIdx::from_raw(0)));
    let rv = Rvalue::UnaryOp(UnOp::Not, operand);
    assert!(matches!(rv, Rvalue::UnaryOp(UnOp::Not, _)));
}

#[test]
fn rvalue_aggregate_tuple() {
    let op1 = Operand::Constant(test_const());
    let op2 = Operand::Constant(test_const());
    let rv = Rvalue::Aggregate(AggregateKind::Tuple, vec![op1, op2]);
    assert!(matches!(rv, Rvalue::Aggregate(AggregateKind::Tuple, ops) if ops.len() == 2));
}

#[test]
fn rvalue_discriminant() {
    let place = Place::new(LocalIdx::from_raw(0));
    let rv = Rvalue::Discriminant(place);
    assert!(matches!(rv, Rvalue::Discriminant(_)));
}

#[test]
fn rvalue_len() {
    let place = Place::new(LocalIdx::from_raw(0));
    let rv = Rvalue::Len(place);
    assert!(matches!(rv, Rvalue::Len(_)));
}

#[test]
fn rvalue_cast() {
    let op = Operand::Copy(Place::new(LocalIdx::from_raw(0)));
    let rv = Rvalue::Cast(CastKind::IntToInt, op, Ty::BOOL);
    assert!(matches!(rv, Rvalue::Cast(CastKind::IntToInt, _, Ty::BOOL)));
}

#[test]
fn rvalue_repeat() {
    let op = Operand::Constant(test_const());
    let len = MirConst {
        kind: MirConstKind::Uint(4),
        ty: Ty::UNIT,
        span: Span::DUMMY,
    };
    let rv = Rvalue::Repeat(op, len);
    assert!(matches!(rv, Rvalue::Repeat(_, _)));
}
