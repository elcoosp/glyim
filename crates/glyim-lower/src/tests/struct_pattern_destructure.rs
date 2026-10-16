use crate::lower::lower_body;
use crate::tests::mock_lower_ctx::TestLowerCtx;
use crate::tests::thir_builder::ThirBuilder;
use glyim_core::def_id::AdtId;
use glyim_core::primitives::IntTy;
use glyim_test::test_ty_ctx;
use glyim_type::*;
use glyim_typeck::thir::{self, FieldPat, PatternKind};

/// S20-T02: Let-stmt with struct pattern destructures correctly
#[test]
fn struct_pattern_binds_fields() {
    let mut ctx_mut = test_ty_ctx();
    let i32_ty = ctx_mut.mk_ty(TyKind::Int(IntTy::I32));
    let adt_id = AdtId::from_raw(10);
    let subst = ctx_mut.intern_substitution(vec![]);
    let struct_ty = ctx_mut.mk_adt(adt_id, subst);
    let interner = ctx_mut.resolver().clone();
    let ctx = ctx_mut.freeze();

    let mut mock = TestLowerCtx::new(&ctx);
    // Register fields: x=0, y=1
    let field_x = interner.intern("x");
    let field_y = interner.intern("y");
    mock.add_field_index(adt_id, 0, field_x, FieldIdx::from_raw(0));
    mock.add_field_index(adt_id, 0, field_y, FieldIdx::from_raw(1));

    let mut b = ThirBuilder::new(Ty::UNIT, interner.clone());
    let mut stmts = Vec::new();

    // let s: Struct = ...;
    b.add_let_binding("s", struct_ty, None, &mut stmts);

    // let Struct { x, y } = s;
    let x_pat = thir::Pattern {
        kind: PatternKind::Binding {
            var_id: thir::LocalVarId::from_raw(0),
            name: field_x,
            mutability: glyim_core::primitives::Mutability::Not,
            binding_ty: None,
            subpattern: None,
        },
        ty: i32_ty,
        span: glyim_span::Span::DUMMY,
    };
    let y_pat = thir::Pattern {
        kind: PatternKind::Binding {
            var_id: thir::LocalVarId::from_raw(0),
            name: field_y,
            mutability: glyim_core::primitives::Mutability::Not,
            binding_ty: None,
            subpattern: None,
        },
        ty: i32_ty,
        span: glyim_span::Span::DUMMY,
    };
    let struct_pat = thir::Pattern {
        kind: PatternKind::Struct {
            adt_id,
            variant_idx: 0,
            fields: vec![
                FieldPat {
                    field: field_x,
                    pattern: x_pat,
                    span: glyim_span::Span::DUMMY,
                },
                FieldPat {
                    field: field_y,
                    pattern: y_pat,
                    span: glyim_span::Span::DUMMY,
                },
            ],
            rest: false,
        },
        ty: struct_ty,
        span: glyim_span::Span::DUMMY,
    };
    let init_local_expr = b.var_ref_expr("s", struct_ty);
    stmts.push(thir::Stmt::Let {
        name: interner.intern("_s2"),
        ty: struct_ty,
        pat: struct_pat,
        init: Some(init_local_expr),
        span: glyim_span::Span::DUMMY,
    });

    let body = b.into_body(stmts, vec![]);
    let result = lower_body(&mock, &body);

    // Should not panic and should have no errors
    assert!(!result.diagnostics.iter().any(|d| d.is_error()));
    // Should have created locals for x and y bindings
    assert!(
        result.body.locals.len() >= 5,
        "expected at least 5 locals (return, s, _s2, x, y), got {}",
        result.body.locals.len()
    );
}
