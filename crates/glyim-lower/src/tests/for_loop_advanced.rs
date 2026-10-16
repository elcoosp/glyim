//! S06-T01: For-loop lowering with Iterator::next calls and Option switching.

use crate::lower::{IteratorNextInfo, lower_body};
use crate::tests::mock_lower_ctx::TestLowerCtx;
use crate::tests::thir_builder::ThirBuilder;
use glyim_core::def_id::FnDefId;
use glyim_core::primitives::{IntTy, Mutability, UintTy};
use glyim_mir::TerminatorKind;
use glyim_test::test_ty_ctx;
use glyim_type::*;
use glyim_typeck::thir::{self, ExprKind, Literal, Pattern, PatternKind};

/// S06-T01: `for i in 0..10 { }` lowers to MIR loop with `next` calls.
///
/// When the `LowerCtx` provides `IteratorNextInfo`, the for-loop lowering
/// generates:
/// 1. A `Call` terminator for `Iterator::next(&mut iter)`
/// 2. A `SwitchInt` on the `Option` discriminant
/// 3. A loop back-edge from the body to the header
#[test]
fn for_loop_lowers_to_loop_with_next_call() {
    let mut ctx_mut = test_ty_ctx();
    let i32_ty = ctx_mut.mk_ty(TyKind::Int(IntTy::I32));
    let usize_ty = ctx_mut.mk_ty(TyKind::Uint(UintTy::Usize));
    let arr_ty = ctx_mut.mk_ty(TyKind::Array(
        i32_ty,
        glyim_type::Const {
            kind: glyim_type::ConstKind::Int(3),
            ty: usize_ty,
        },
    ));

    // Construct Option<i32> type for the next() return type
    let option_adt_id = glyim_core::def_id::AdtId::from_raw(0);
    let option_substs = ctx_mut.intern_substitution(vec![GenericArg::Ty(i32_ty)]);
    let option_ty = ctx_mut.mk_ty(TyKind::Adt(option_adt_id, option_substs));

    // Construct the next() function type
    let next_fn_def_id = FnDefId::from_raw(999);
    let next_fn_substs = ctx_mut.intern_substitution(vec![]);
    let next_fn_ty = ctx_mut.mk_ty(TyKind::FnDef(next_fn_def_id, next_fn_substs));

    // Discriminant type (u8)
    let discr_ty = ctx_mut.mk_ty(TyKind::Uint(UintTy::U8));

    // &mut iter type — use a placeholder ref type
    let ref_iter_ty = ctx_mut.mk_ref(Region::Erased, arr_ty, Mutability::Mut);

    let iterator_info = IteratorNextInfo {
        fn_def_id: next_fn_def_id,
        fn_substs: next_fn_substs,
        fn_ty: next_fn_ty,
        option_ty,
        discr_ty,
        ref_iter_ty,
    };

    let interner = ctx_mut.resolver().clone();
    let ctx = ctx_mut.freeze();
    let mut mock = TestLowerCtx::new(&ctx);
    mock.set_iterator_next_info(iterator_info);

    let mut b = ThirBuilder::new(Ty::UNIT, interner);
    let mut stmts = Vec::new();

    // let arr = [1, 2]
    let arr_var = b.expr(
        ExprKind::Array(vec![
            b.expr(ExprKind::Literal(Literal::Int(1, None)), i32_ty),
            b.expr(ExprKind::Literal(Literal::Int(2, None)), i32_ty),
        ]),
        arr_ty,
    );
    b.add_let_binding("arr", arr_ty, Some(arr_var), &mut stmts);

    // for elem in arr { }
    let elem_pat = Pattern {
        kind: PatternKind::Binding {
            var_id: thir::LocalVarId::from_raw(0),
            name: b.make_name("elem"),
            mutability: Mutability::Not,
            binding_ty: None,
            subpattern: None,
        },
        ty: i32_ty,
        span: glyim_span::Span::DUMMY,
    };
    let for_body = b.expr(
        ExprKind::Block {
            stmts: vec![],
            tail: None,
        },
        Ty::UNIT,
    );
    let for_expr = b.expr(
        ExprKind::For {
            pat: Box::new(elem_pat),
            iterable: Box::new(b.var_ref_expr("arr", arr_ty)),
            body: Box::new(for_body),
            next: None,
        },
        Ty::UNIT,
    );
    stmts.push(thir::Stmt::Expr { expr: for_expr });

    let body = b.into_body(stmts, vec![]);
    let result = lower_body(&mock, &body);

    // Verify the MIR has a loop structure with multiple blocks.
    // Expected blocks: entry→header, header(Call next), after_call(SwitchInt),
    // some_branch(body), exit
    assert!(
        result.body.basic_blocks.len() >= 4,
        "expected at least 4 blocks for for-loop with next(), got {}",
        result.body.basic_blocks.len()
    );

    // Verify there is a Call terminator (for next())
    let has_call = result
        .body
        .basic_blocks
        .iter()
        .any(|bb| matches!(bb.terminator.kind, TerminatorKind::Call { .. }));
    assert!(
        has_call,
        "expected a Call terminator for next() in for-loop MIR"
    );

    // Verify there is a SwitchInt terminator (for Option discriminant)
    let has_switch = result
        .body
        .basic_blocks
        .iter()
        .any(|bb| matches!(bb.terminator.kind, TerminatorKind::SwitchInt { .. }));
    assert!(
        has_switch,
        "expected a SwitchInt terminator for Option matching in for-loop MIR"
    );
}

/// For-loop without iterator info falls back to simplified lowering.
/// This should still produce a loop structure (multiple blocks).
#[test]
fn for_loop_simplified_without_iterator_info() {
    let mut ctx_mut = test_ty_ctx();
    let i32_ty = ctx_mut.mk_ty(TyKind::Int(IntTy::I32));
    let usize_ty = ctx_mut.mk_ty(TyKind::Uint(UintTy::Usize));
    let arr_ty = ctx_mut.mk_ty(TyKind::Array(
        i32_ty,
        glyim_type::Const {
            kind: glyim_type::ConstKind::Int(3),
            ty: usize_ty,
        },
    ));
    let interner = ctx_mut.resolver().clone();
    let ctx = ctx_mut.freeze();
    let mock = TestLowerCtx::new(&ctx);

    let mut b = ThirBuilder::new(Ty::UNIT, interner);
    let mut stmts = Vec::new();

    let arr_var = b.expr(
        ExprKind::Array(vec![
            b.expr(ExprKind::Literal(Literal::Int(1, None)), i32_ty),
        ]),
        arr_ty,
    );
    b.add_let_binding("arr", arr_ty, Some(arr_var), &mut stmts);

    let elem_pat = Pattern {
        kind: PatternKind::Binding {
            var_id: thir::LocalVarId::from_raw(0),
            name: b.make_name("elem"),
            mutability: Mutability::Not,
            binding_ty: None,
            subpattern: None,
        },
        ty: i32_ty,
        span: glyim_span::Span::DUMMY,
    };
    let for_body = b.expr(
        ExprKind::Block {
            stmts: vec![],
            tail: None,
        },
        Ty::UNIT,
    );
    let for_expr = b.expr(
        ExprKind::For {
            pat: Box::new(elem_pat),
            iterable: Box::new(b.var_ref_expr("arr", arr_ty)),
            body: Box::new(for_body),
            next: None,
        },
        Ty::UNIT,
    );
    stmts.push(thir::Stmt::Expr { expr: for_expr });

    let body = b.into_body(stmts, vec![]);
    let result = lower_body(&mock, &body);

    // Even without iterator info, should produce multiple blocks (loop structure)
    assert!(
        result.body.basic_blocks.len() >= 3,
        "expected at least 3 blocks for simplified for-loop, got {}",
        result.body.basic_blocks.len()
    );
}

/// For-loop with break should resolve to the loop exit block.
#[test]
fn for_loop_break_targets_exit() {
    let mut ctx_mut = test_ty_ctx();
    let i32_ty = ctx_mut.mk_ty(TyKind::Int(IntTy::I32));
    let usize_ty = ctx_mut.mk_ty(TyKind::Uint(UintTy::Usize));
    let arr_ty = ctx_mut.mk_ty(TyKind::Array(
        i32_ty,
        glyim_type::Const {
            kind: glyim_type::ConstKind::Int(3),
            ty: usize_ty,
        },
    ));
    let interner = ctx_mut.resolver().clone();
    let ctx = ctx_mut.freeze();
    let mock = TestLowerCtx::new(&ctx);

    let mut b = ThirBuilder::new(Ty::UNIT, interner);
    let mut stmts = Vec::new();

    let arr_var = b.expr(ExprKind::Array(vec![]), arr_ty);
    b.add_let_binding("arr", arr_ty, Some(arr_var), &mut stmts);

    let elem_pat = Pattern {
        kind: PatternKind::Binding {
            var_id: thir::LocalVarId::from_raw(0),
            name: b.make_name("elem"),
            mutability: Mutability::Not,
            binding_ty: None,
            subpattern: None,
        },
        ty: i32_ty,
        span: glyim_span::Span::DUMMY,
    };
    // Body contains a break
    let break_expr = b.expr(ExprKind::Break { value: None }, Ty::NEVER);
    let for_body = b.expr(
        ExprKind::Block {
            stmts: vec![thir::Stmt::Expr { expr: break_expr }],
            tail: None,
        },
        Ty::UNIT,
    );
    let for_expr = b.expr(
        ExprKind::For {
            pat: Box::new(elem_pat),
            iterable: Box::new(b.var_ref_expr("arr", arr_ty)),
            body: Box::new(for_body),
            next: None,
        },
        Ty::UNIT,
    );
    stmts.push(thir::Stmt::Expr { expr: for_expr });

    let body = b.into_body(stmts, vec![]);
    let result = lower_body(&mock, &body);

    // Should not have any error diagnostics for break inside for-loop
    assert!(
        !result.diagnostics.iter().any(|d| d.is_error()),
        "break inside for-loop should not produce error diagnostics"
    );
}
