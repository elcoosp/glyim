use super::common::*;
use glyim_core::arena::IndexVec;
use glyim_core::def_id::AdtId;
use glyim_core::primitives::{FloatTy, IntTy, UintTy};
use glyim_hir::*;
use glyim_type::{AdtDef, AdtKind, Substitution, TyKind, VariantDef};

#[test]
fn cast_i32_to_f64() {
    let mut exprs = Vec::new();
    exprs.push(Expr::Literal(Literal::Int(42, None)));
    exprs.push(Expr::Cast {
        expr: ExprId::from_raw(0),
        ty: TypeRef::Path(glyim_hir::Path::from_single(name("f64"))),
    });

    let (hir, body_id) = make_single_body_hir(exprs);
    let thir_body = typeck_single_body(&hir, body_id);
    assert_eq!(thir_body.stmts.len(), 2);
}

/// Plan §13.2: `is_valid_cast` is the single source of truth for cast legality.
/// It must allow legal primitive casts and fieldless-enum → int, and reject
/// illegal casts (e.g. int → String, struct → int).
#[test]
fn is_valid_cast_rules() {
    use glyim_type::is_valid_cast;

    let mut ctx = make_ty_ctx();
    let i32_ty = ctx.mk_ty(TyKind::Int(IntTy::I32));
    let u8_ty = ctx.mk_ty(TyKind::Uint(UintTy::U8));
    let f64_ty = ctx.mk_ty(TyKind::Float(FloatTy::F64));
    let string_ty = ctx.mk_ty(TyKind::String);
    let bool_ty = ctx.mk_ty(TyKind::Bool);

    // Legal primitive casts.
    assert!(is_valid_cast(&ctx, i32_ty, f64_ty), "i32 -> f64 legal");
    assert!(is_valid_cast(&ctx, i32_ty, u8_ty), "i32 -> u8 legal");
    assert!(is_valid_cast(&ctx, f64_ty, i32_ty), "f64 -> i32 legal");
    assert!(is_valid_cast(&ctx, bool_ty, u8_ty), "bool -> u8 legal");
    assert!(is_valid_cast(&ctx, i32_ty, i32_ty), "identity cast legal");

    // Illegal: int -> String is not a numeric cast.
    assert!(
        !is_valid_cast(&ctx, i32_ty, string_ty),
        "i32 -> String must be rejected"
    );

    // Fieldless enum -> int is legal.
    let enum_id = AdtId::from_raw(0);
    let fieldless_enum = AdtDef {
        kind: AdtKind::Enum,
        fields: IndexVec::new(),
        variants: vec![VariantDef {
            name: name("E0"),
            style: glyim_type::adt_def::VariantStyle::Unit,
            fields: IndexVec::new(),
        }],
        generic_params: vec![],
    };
    ctx.register_adt(enum_id, fieldless_enum);
    let enum_ty = ctx.mk_adt(enum_id, Substitution::empty());
    assert!(
        is_valid_cast(&ctx, enum_ty, i32_ty),
        "fieldless enum -> int legal"
    );

    // A struct (with a field) -> int must be rejected.
    let struct_id = AdtId::from_raw(1);
    let struct_def = AdtDef {
        kind: AdtKind::Struct,
        fields: IndexVec::new(),
        variants: vec![VariantDef {
            name: name("S0"),
            style: glyim_type::adt_def::VariantStyle::Unit,
            fields: IndexVec::new(),
        }],
        generic_params: vec![],
    };
    ctx.register_adt(struct_id, struct_def);
    let struct_ty = ctx.mk_adt(struct_id, Substitution::empty());
    assert!(
        !is_valid_cast(&ctx, struct_ty, i32_ty),
        "struct -> int must be rejected"
    );
}
