//! Tests for Const and `ConstKind`.

use glyim_core::primitives::UintTy;

use super::helpers::with_fresh_ty_ctx;
use crate::*;

#[test]
fn const_int_kind() {
    let c = Const {
        kind: ConstKind::Int(-42i128),
        ty: Ty::from_raw(4), // placeholder
    };
    assert!(matches!(c.kind, ConstKind::Int(-42)));
}

#[test]
fn const_uint_kind() {
    let c = Const {
        kind: ConstKind::Uint(42u128),
        ty: Ty::from_raw(4),
    };
    assert!(matches!(c.kind, ConstKind::Uint(42)));
}

#[test]
fn const_float_bits_kind() {
    let c = Const {
        kind: ConstKind::FloatBits(0x4000000000000000u64), // 2.0 f64 bits
        ty: Ty::from_raw(4),
    };
    assert!(matches!(c.kind, ConstKind::FloatBits(_)));
}

#[test]
fn const_bool_kind() {
    let c = Const {
        kind: ConstKind::Bool(true),
        ty: Ty::from_raw(4),
    };
    assert!(matches!(c.kind, ConstKind::Bool(true)));
}

#[test]
fn const_char_kind() {
    let c = Const {
        kind: ConstKind::Char('x'),
        ty: Ty::from_raw(4),
    };
    assert!(matches!(c.kind, ConstKind::Char('x')));
}

#[test]
fn const_string_kind() {
    let (ctx, ()) = with_fresh_ty_ctx(|c| {
        let name = c.resolver().intern("hello");
        let cnst = Const {
            kind: ConstKind::String(name),
            ty: c.mk_ty(TyKind::String),
        };
        assert!(matches!(cnst.kind, ConstKind::String(_)));
    });
    let _ = ctx;
}

#[test]
fn const_unit_kind() {
    let c = Const {
        kind: ConstKind::Unit,
        ty: Ty::UNIT,
    };
    assert!(matches!(c.kind, ConstKind::Unit));
}

#[test]
fn const_error_kind() {
    let c = Const {
        kind: ConstKind::Error,
        ty: Ty::ERROR,
    };
    assert!(matches!(c.kind, ConstKind::Error));
}

#[test]
fn const_infer_kind() {
    let c = Const {
        kind: ConstKind::Infer(ConstVar::from_raw(0)),
        ty: Ty::from_raw(4),
    };
    assert!(matches!(c.kind, ConstKind::Infer(_)));
}

#[test]
fn const_param_kind() {
    let (ctx, ()) = with_fresh_ty_ctx(|c| {
        let name = c.resolver().intern("N");
        let param = ParamConst { index: 0, name };
        let cnst = Const {
            kind: ConstKind::Param(param),
            ty: c.mk_ty(TyKind::Uint(UintTy::Usize)),
        };
        if let ConstKind::Param(p) = &cnst.kind {
            assert_eq!(p.index, 0);
        } else {
            panic!("expected Param");
        }
    });
    let _ = ctx;
}

#[test]
fn const_in_substitution() {
    let (ctx, sub) = with_fresh_ty_ctx(|c| {
        let len_const = Const {
            kind: ConstKind::Uint(10),
            ty: c.mk_ty(TyKind::Uint(UintTy::Usize)),
        };
        let args = vec![GenericArg::Const(len_const)];
        c.intern_substitution(args)
    });
    let args = ctx.substitution_args(sub);
    assert_eq!(args.len(), 1);
    if let GenericArg::Const(cnst) = &args[0] {
        assert!(matches!(cnst.kind, ConstKind::Uint(10)));
    } else {
        panic!("expected Const");
    }
}

#[test]
fn const_equality() {
    let c1 = Const {
        kind: ConstKind::Uint(42),
        ty: Ty::BOOL,
    };
    let c2 = Const {
        kind: ConstKind::Uint(42),
        ty: Ty::BOOL,
    };
    let c3 = Const {
        kind: ConstKind::Uint(99),
        ty: Ty::BOOL,
    };
    assert_eq!(c1, c2);
    assert_ne!(c1, c3);
}

#[test]
fn const_debug_format() {
    let c = Const {
        kind: ConstKind::Bool(true),
        ty: Ty::BOOL,
    };
    let debug = format!("{c:?}");
    assert!(debug.contains("Bool(true)"));
}
