use crate::*;
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
fn operand_copy() {
    let place = Place::new(LocalIdx::from_usize(0));
    let op = Operand::Copy(place);
    assert!(matches!(op, Operand::Copy(p) if p.local == LocalIdx::from_usize(0)));
}

#[test]
fn operand_move() {
    let place = Place::new(LocalIdx::from_usize(3));
    let op = Operand::Move(place);
    assert!(matches!(op, Operand::Move(p) if p.local == LocalIdx::from_usize(3)));
}

#[test]
fn operand_constant() {
    let op = Operand::Constant(test_const());
    assert!(matches!(op, Operand::Constant(_)));
}

#[test]
fn operand_copy_with_projection() {
    let place = Place {
        local: LocalIdx::from_usize(1),
        projection: Box::new([
            ProjectionElem::Deref,
            ProjectionElem::Field(FieldIdx::from_usize(0)),
        ]),
    };
    let op = Operand::Copy(place);
    if let Operand::Copy(p) = &op {
        assert_eq!(p.projection.len(), 2);
    } else {
        panic!("Expected Copy");
    }
}
