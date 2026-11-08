//! Exhaustive `TyKind` variant construction tests.
//! Ensures every `TyKind` variant can be constructed and roundtripped through the context.

use glyim_core::def_id::*;
use glyim_core::primitives::*;

use super::helpers::with_fresh_ty_ctx;
use crate::*;

#[test]
fn ty_kind_bool() {
    let (ctx, ty) = with_fresh_ty_ctx(|c| c.mk_ty(TyKind::Bool));
    assert!(matches!(ctx.ty_kind(ty), TyKind::Bool));
}

#[test]
fn ty_kind_never() {
    let (ctx, ty) = with_fresh_ty_ctx(|c| c.mk_ty(TyKind::Never));
    assert!(matches!(ctx.ty_kind(ty), TyKind::Never));
}

#[test]
fn ty_kind_unit() {
    let (ctx, ty) = with_fresh_ty_ctx(|c| c.mk_ty(TyKind::Unit));
    assert!(matches!(ctx.ty_kind(ty), TyKind::Unit));
}

#[test]
fn ty_kind_int_i8() {
    let (ctx, ty) = with_fresh_ty_ctx(|c| c.mk_ty(TyKind::Int(IntTy::I8)));
    assert!(matches!(ctx.ty_kind(ty), TyKind::Int(IntTy::I8)));
}

#[test]
fn ty_kind_int_i16() {
    let (ctx, ty) = with_fresh_ty_ctx(|c| c.mk_ty(TyKind::Int(IntTy::I16)));
    assert!(matches!(ctx.ty_kind(ty), TyKind::Int(IntTy::I16)));
}

#[test]
fn ty_kind_int_i32() {
    let (ctx, ty) = with_fresh_ty_ctx(|c| c.mk_ty(TyKind::Int(IntTy::I32)));
    assert!(matches!(ctx.ty_kind(ty), TyKind::Int(IntTy::I32)));
}

#[test]
fn ty_kind_int_i64() {
    let (ctx, ty) = with_fresh_ty_ctx(|c| c.mk_ty(TyKind::Int(IntTy::I64)));
    assert!(matches!(ctx.ty_kind(ty), TyKind::Int(IntTy::I64)));
}

#[test]
fn ty_kind_int_isize() {
    let (ctx, ty) = with_fresh_ty_ctx(|c| c.mk_ty(TyKind::Int(IntTy::Isize)));
    assert!(matches!(ctx.ty_kind(ty), TyKind::Int(IntTy::Isize)));
}

#[test]
fn ty_kind_uint_u8() {
    let (ctx, ty) = with_fresh_ty_ctx(|c| c.mk_ty(TyKind::Uint(UintTy::U8)));
    assert!(matches!(ctx.ty_kind(ty), TyKind::Uint(UintTy::U8)));
}

#[test]
fn ty_kind_uint_u16() {
    let (ctx, ty) = with_fresh_ty_ctx(|c| c.mk_ty(TyKind::Uint(UintTy::U16)));
    assert!(matches!(ctx.ty_kind(ty), TyKind::Uint(UintTy::U16)));
}

#[test]
fn ty_kind_uint_u32() {
    let (ctx, ty) = with_fresh_ty_ctx(|c| c.mk_ty(TyKind::Uint(UintTy::U32)));
    assert!(matches!(ctx.ty_kind(ty), TyKind::Uint(UintTy::U32)));
}

#[test]
fn ty_kind_uint_u64() {
    let (ctx, ty) = with_fresh_ty_ctx(|c| c.mk_ty(TyKind::Uint(UintTy::U64)));
    assert!(matches!(ctx.ty_kind(ty), TyKind::Uint(UintTy::U64)));
}

#[test]
fn ty_kind_uint_usize() {
    let (ctx, ty) = with_fresh_ty_ctx(|c| c.mk_ty(TyKind::Uint(UintTy::Usize)));
    assert!(matches!(ctx.ty_kind(ty), TyKind::Uint(UintTy::Usize)));
}

#[test]
fn ty_kind_float_f32() {
    let (ctx, ty) = with_fresh_ty_ctx(|c| c.mk_ty(TyKind::Float(FloatTy::F32)));
    assert!(matches!(ctx.ty_kind(ty), TyKind::Float(FloatTy::F32)));
}

#[test]
fn ty_kind_float_f64() {
    let (ctx, ty) = with_fresh_ty_ctx(|c| c.mk_ty(TyKind::Float(FloatTy::F64)));
    assert!(matches!(ctx.ty_kind(ty), TyKind::Float(FloatTy::F64)));
}

#[test]
fn ty_kind_char() {
    let (ctx, ty) = with_fresh_ty_ctx(|c| c.mk_ty(TyKind::Char));
    assert!(matches!(ctx.ty_kind(ty), TyKind::Char));
}

#[test]
fn ty_kind_string() {
    let (ctx, ty) = with_fresh_ty_ctx(|c| c.mk_ty(TyKind::String));
    assert!(matches!(ctx.ty_kind(ty), TyKind::String));
}

#[test]
fn ty_kind_error() {
    let (ctx, ty) = with_fresh_ty_ctx(|c| c.mk_ty(TyKind::Error));
    assert!(matches!(ctx.ty_kind(ty), TyKind::Error));
}

#[test]
fn ty_kind_infer_ty_var() {
    let (ctx, ty) = with_fresh_ty_ctx(|c| c.mk_ty(TyKind::Infer(InferVar::Ty(TyVar::from_raw(0)))));
    assert!(matches!(ctx.ty_kind(ty), TyKind::Infer(InferVar::Ty(_))));
}

#[test]
fn ty_kind_infer_int_var() {
    let (ctx, ty) =
        with_fresh_ty_ctx(|c| c.mk_ty(TyKind::Infer(InferVar::Int(IntVar::from_raw(0)))));
    assert!(matches!(ctx.ty_kind(ty), TyKind::Infer(InferVar::Int(_))));
}

#[test]
fn ty_kind_infer_float_var() {
    let (ctx, ty) =
        with_fresh_ty_ctx(|c| c.mk_ty(TyKind::Infer(InferVar::Float(FloatVar::from_raw(0)))));
    assert!(matches!(ctx.ty_kind(ty), TyKind::Infer(InferVar::Float(_))));
}

#[test]
fn ty_kind_adt() {
    let (ctx, ty) = with_fresh_ty_ctx(|c| {
        let substs = c.intern_substitution(vec![]);
        c.mk_adt(AdtId::from_raw(1), substs)
    });
    assert!(matches!(ctx.ty_kind(ty), TyKind::Adt(_, _)));
}

#[test]
fn ty_kind_fn_def() {
    let (ctx, ty) = with_fresh_ty_ctx(|c| {
        let substs = c.intern_substitution(vec![]);
        c.mk_ty(TyKind::FnDef(FnDefId::from_raw(1), substs))
    });
    assert!(matches!(ctx.ty_kind(ty), TyKind::FnDef(_, _)));
}

#[test]
fn ty_kind_closure() {
    let (ctx, ty) = with_fresh_ty_ctx(|c| {
        let substs = c.intern_substitution(vec![]);
        c.mk_ty(TyKind::Closure(ClosureId::from_raw(1), substs))
    });
    assert!(matches!(ctx.ty_kind(ty), TyKind::Closure(_, _)));
}

#[test]
fn ty_kind_fn_ptr() {
    let (ctx, ty) = with_fresh_ty_ctx(|c| {
        let inputs = c.intern_substitution(vec![]);
        let sig = FnSig {
            inputs,
            output: c.unit_ty(),
            c_variadic: false,
            unsafety: Safety::Safe,
            abi: Abi::Glyim,
        };
        c.mk_fn_ptr(sig)
    });
    assert!(matches!(ctx.ty_kind(ty), TyKind::FnPtr(_)));
}

#[test]
fn ty_kind_ref() {
    let (ctx, ty) = with_fresh_ty_ctx(|c| c.mk_ref(Region::Erased, c.bool_ty(), Mutability::Not));
    assert!(matches!(ctx.ty_kind(ty), TyKind::Ref(_, _, _)));
}

#[test]
fn ty_kind_raw_ptr() {
    let (ctx, ty) = with_fresh_ty_ctx(|c| c.mk_ty(TyKind::RawPtr(c.bool_ty(), Mutability::Mut)));
    assert!(matches!(ctx.ty_kind(ty), TyKind::RawPtr(_, _)));
}

#[test]
fn ty_kind_slice() {
    let (ctx, ty) = with_fresh_ty_ctx(|c| c.mk_ty(TyKind::Slice(c.bool_ty())));
    assert!(matches!(ctx.ty_kind(ty), TyKind::Slice(_)));
}

#[test]
fn ty_kind_array() {
    let (ctx, ty) = with_fresh_ty_ctx(|c| {
        let len = Const {
            kind: ConstKind::Uint(5),
            ty: c.mk_ty(TyKind::Uint(UintTy::Usize)),
        };
        c.mk_ty(TyKind::Array(c.bool_ty(), len))
    });
    assert!(matches!(ctx.ty_kind(ty), TyKind::Array(_, _)));
}

#[test]
fn ty_kind_tuple() {
    let (ctx, ty) = with_fresh_ty_ctx(|c| {
        let substs = c.intern_substitution(vec![GenericArg::Ty(c.bool_ty())]);
        c.mk_tuple(substs)
    });
    assert!(matches!(ctx.ty_kind(ty), TyKind::Tuple(_)));
}

#[test]
fn ty_kind_dynamic() {
    let (ctx, ty) = with_fresh_ty_ctx(|c| {
        let trait_substs = c.intern_substitution(vec![]);
        let pred = Predicate::Trait(TraitPredicate {
            trait_ref: TraitRef {
                def_id: TraitDefId::from_raw(1),
                substs: trait_substs,
            },
            polarity: ImplPolarity::Positive,
        });
        let preds: Box<[Predicate]> = Box::new([pred]);
        let binder = Binder::bind(preds, Box::new([BoundVariableKind::Ty(BoundTyKind::Anon)]));
        c.mk_ty(TyKind::Dynamic(binder, Region::Erased))
    });
    assert!(matches!(ctx.ty_kind(ty), TyKind::Dynamic(_, _)));
}

#[test]
fn ty_kind_opaque() {
    let (ctx, ty) = with_fresh_ty_ctx(|c| {
        let substs = c.intern_substitution(vec![]);
        c.mk_ty(TyKind::Opaque(OpaqueTyId::from_raw(1), substs))
    });
    assert!(matches!(ctx.ty_kind(ty), TyKind::Opaque(_, _)));
}

#[test]
fn ty_kind_param() {
    let (ctx, ty) = with_fresh_ty_ctx(|c| {
        let name = c.resolver().intern("T");
        c.mk_ty(TyKind::Param(ParamTy { index: 0, name }))
    });
    assert!(matches!(ctx.ty_kind(ty), TyKind::Param(_)));
}

#[test]
fn ty_kind_bound() {
    let (ctx, ty) = with_fresh_ty_ctx(|c| {
        let bound = BoundTy {
            var: 0,
            kind: BoundTyKind::Anon,
        };
        c.mk_ty(TyKind::Bound(0, bound))
    });
    assert!(matches!(ctx.ty_kind(ty), TyKind::Bound(_, _)));
}

#[test]
fn ty_kind_debug_format() {
    let kind = TyKind::Int(IntTy::I32);
    let debug = format!("{kind:?}");
    assert!(debug.contains("Int"));
    assert!(debug.contains("I32"));
}

#[test]
fn ty_kind_equality() {
    assert_eq!(TyKind::Bool, TyKind::Bool);
    assert_eq!(TyKind::Int(IntTy::I32), TyKind::Int(IntTy::I32));
    assert_ne!(TyKind::Int(IntTy::I32), TyKind::Int(IntTy::I64));
    assert_ne!(TyKind::Bool, TyKind::Never);
}
