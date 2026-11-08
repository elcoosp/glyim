use crate::*;
use glyim_core::interner::Interner;
use glyim_span::Span;
use glyim_type::Ty;

#[test]
fn mir_const_int_zero() {
    let c = MirConst {
        kind: MirConstKind::Int(0),
        ty: Ty::BOOL,
        span: Span::DUMMY,
    };
    assert!(matches!(c.kind, MirConstKind::Int(0)));
}

#[test]
fn mir_const_int_min() {
    let c = MirConst {
        kind: MirConstKind::Int(i128::MIN),
        ty: Ty::BOOL,
        span: Span::DUMMY,
    };
    assert!(matches!(c.kind, MirConstKind::Int(i128::MIN)));
}

#[test]
fn mir_const_int_max() {
    let c = MirConst {
        kind: MirConstKind::Int(i128::MAX),
        ty: Ty::BOOL,
        span: Span::DUMMY,
    };
    assert!(matches!(c.kind, MirConstKind::Int(i128::MAX)));
}

#[test]
fn mir_const_uint_zero() {
    let c = MirConst {
        kind: MirConstKind::Uint(0),
        ty: Ty::BOOL,
        span: Span::DUMMY,
    };
    assert!(matches!(c.kind, MirConstKind::Uint(0)));
}

#[test]
fn mir_const_uint_max() {
    let c = MirConst {
        kind: MirConstKind::Uint(u128::MAX),
        ty: Ty::BOOL,
        span: Span::DUMMY,
    };
    assert!(matches!(c.kind, MirConstKind::Uint(u128::MAX)));
}

#[test]
fn mir_const_float_bits_zero() {
    let c = MirConst {
        kind: MirConstKind::FloatBits(0),
        ty: Ty::BOOL,
        span: Span::DUMMY,
    };
    assert!(matches!(c.kind, MirConstKind::FloatBits(0)));
}

#[test]
fn mir_const_bool_false() {
    let c = MirConst {
        kind: MirConstKind::Bool(false),
        ty: Ty::BOOL,
        span: Span::DUMMY,
    };
    assert!(matches!(c.kind, MirConstKind::Bool(false)));
}

#[test]
fn mir_const_char_unicode() {
    let c = MirConst {
        kind: MirConstKind::Char('\u{1F600}'),
        ty: Ty::BOOL,
        span: Span::DUMMY,
    };
    assert!(matches!(c.kind, MirConstKind::Char('\u{1F600}')));
}

#[test]
fn mir_const_string_with_interner() {
    let interner = Interner::new();
    let name = interner.intern("test_string");
    let c = MirConst {
        kind: MirConstKind::String(name),
        ty: Ty::BOOL,
        span: Span::DUMMY,
    };
    assert!(matches!(c.kind, MirConstKind::String(_)));
}

#[test]
fn mir_const_clone() {
    let c = MirConst {
        kind: MirConstKind::Int(42),
        ty: Ty::BOOL,
        span: Span::DUMMY,
    };
    let cloned = c.clone();
    assert!(matches!(cloned.kind, MirConstKind::Int(42)));
}

#[test]
fn mir_const_debug_format() {
    let c = MirConst {
        kind: MirConstKind::Bool(true),
        ty: Ty::BOOL,
        span: Span::DUMMY,
    };
    let s = format!("{c:?}");
    assert!(s.contains("Bool"));
}
