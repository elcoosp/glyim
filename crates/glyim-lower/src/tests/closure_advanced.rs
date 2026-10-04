//! S06-T02: Closure lowering with environment Aggregate.

use crate::lower::lower_body;
use crate::tests::mock_lower_ctx::TestLowerCtx;
use crate::tests::thir_builder::ThirBuilder;
use glyim_core::def_id::{ClosureId, CrateId, DefId, LocalDefId};
use glyim_core::primitives::{IntTy, Mutability};
use glyim_mir::{AggregateKind, Rvalue, StatementKind};
use glyim_test::test_ty_ctx;
use glyim_type::*;
use glyim_typeck::thir::CaptureKind;
use glyim_typeck::thir::{self, ExprKind};

/// S06-T02: Closure `|x| x+1` lowers to `Aggregate` with captured vars.
///
/// The closure lowering should generate an `Aggregate` rvalue with
/// `AggregateKind::Closure(closure_id, substs)` and operands for
/// each captured variable.
#[test]
fn closure_lowers_to_aggregate() {
    let mut ctx_mut = test_ty_ctx();
    let i32_ty = ctx_mut.mk_ty(TyKind::Int(IntTy::I32));

    // Construct a closure type: Closure(0, [])
    let closure_id = ClosureId::from_raw(0);
    let closure_substs = ctx_mut.intern_substitution(vec![]);
    let closure_ty = ctx_mut.mk_ty(TyKind::Closure(closure_id, closure_substs));

    let interner = ctx_mut.resolver().clone();
    let ctx = ctx_mut.freeze();
    let mock = TestLowerCtx::new(&ctx);

    let b = ThirBuilder::new(closure_ty, interner);
    let closure_body = thir::Body {
        owner: DefId::new(CrateId::from_raw(0), LocalDefId::from_raw(1)),
        params: vec![],
        return_ty: i32_ty,
        stmts: vec![],
        span: glyim_span::Span::DUMMY,
    };
    let closure_expr = b.expr(
        ExprKind::Closure {
            body: Box::new(closure_body),
            captures: vec![],
            is_move: false,
        },
        closure_ty,
    );
    let body = b.into_body(vec![thir::Stmt::Expr { expr: closure_expr }], vec![]);
    let result = lower_body(&mock, &body);

    // Should not have error diagnostics
    assert!(
        !result.diagnostics.iter().any(|d| d.is_error()),
        "closure lowering should not produce error diagnostics"
    );

    // Should have at least one Aggregate rvalue with AggregateKind::Closure
    let has_closure_aggregate = result.body.basic_blocks.iter().any(|bb| {
        bb.statements.iter().any(|stmt| {
            if let StatementKind::Assign(_, ref rvalue) = stmt.kind {
                matches!(rvalue, Rvalue::Aggregate(AggregateKind::Closure(_, _), _))
            } else {
                false
            }
        })
    });
    assert!(
        has_closure_aggregate,
        "expected an Aggregate rvalue with AggregateKind::Closure in closure lowering MIR"
    );
}

/// Closure that captures a variable by shared reference.
#[test]
fn closure_with_by_ref_capture() {
    let mut ctx_mut = test_ty_ctx();
    let i32_ty = ctx_mut.mk_ty(TyKind::Int(IntTy::I32));

    // &i32 type for the capture
    let ref_i32_ty = ctx_mut.mk_ref(Region::Erased, i32_ty, Mutability::Not);

    // Closure type
    let closure_id = ClosureId::from_raw(1);
    let closure_substs = ctx_mut.intern_substitution(vec![GenericArg::Ty(i32_ty)]);
    let closure_ty = ctx_mut.mk_ty(TyKind::Closure(closure_id, closure_substs));

    let interner = ctx_mut.resolver().clone();
    let ctx = ctx_mut.freeze();
    let mock = TestLowerCtx::new(&ctx);

    let mut b = ThirBuilder::new(closure_ty, interner.clone());
    let mut stmts = Vec::new();

    // let x = 42
    b.add_let_binding(
        "x",
        i32_ty,
        Some(b.expr(ExprKind::Literal(thir::Literal::Int(42, None)), i32_ty)),
        &mut stmts,
    );

    // Closure that captures x by shared reference
    let x_name = interner.intern("x");
    let x_var_id = *b.var_names.get(&x_name).expect("x should be in var_names");
    let closure_body = thir::Body {
        owner: DefId::new(CrateId::from_raw(0), LocalDefId::from_raw(1)),
        params: vec![],
        return_ty: i32_ty,
        stmts: vec![],
        span: glyim_span::Span::DUMMY,
    };
    let closure_expr = b.expr(
        ExprKind::Closure {
            body: Box::new(closure_body),
            captures: vec![thir::Capture {
                local: x_var_id,
                kind: CaptureKind::ByRef(Mutability::Not),
                ty: ref_i32_ty,
            }],
            is_move: false,
        },
        closure_ty,
    );
    stmts.push(thir::Stmt::Expr { expr: closure_expr });

    let body = b.into_body(stmts, vec![]);
    let result = lower_body(&mock, &body);

    // Should not have error diagnostics
    assert!(
        !result.diagnostics.iter().any(|d| d.is_error()),
        "closure with by-ref capture should not produce error diagnostics"
    );

    // Should have an Aggregate rvalue with Closure kind and 1 operand
    let closure_agg = result.body.basic_blocks.iter().find_map(|bb| {
        bb.statements.iter().find_map(|stmt| {
            if let StatementKind::Assign(
                _,
                Rvalue::Aggregate(AggregateKind::Closure(_, _), ref ops),
            ) = stmt.kind
            {
                Some(ops.len())
            } else {
                None
            }
        })
    });
    assert_eq!(
        closure_agg,
        Some(1),
        "expected closure Aggregate with 1 captured operand, got {:?}",
        closure_agg
    );
}

/// Closure that captures a variable by value.
#[test]
fn closure_with_by_value_capture() {
    let mut ctx_mut = test_ty_ctx();
    let i32_ty = ctx_mut.mk_ty(TyKind::Int(IntTy::I32));

    let closure_id = ClosureId::from_raw(2);
    let closure_substs = ctx_mut.intern_substitution(vec![GenericArg::Ty(i32_ty)]);
    let closure_ty = ctx_mut.mk_ty(TyKind::Closure(closure_id, closure_substs));

    let interner = ctx_mut.resolver().clone();
    let ctx = ctx_mut.freeze();
    let mock = TestLowerCtx::new(&ctx);

    let mut b = ThirBuilder::new(closure_ty, interner.clone());
    let mut stmts = Vec::new();

    b.add_let_binding(
        "x",
        i32_ty,
        Some(b.expr(ExprKind::Literal(thir::Literal::Int(42, None)), i32_ty)),
        &mut stmts,
    );

    let x_name = interner.intern("x");
    let x_var_id = *b.var_names.get(&x_name).expect("x should be in var_names");
    let closure_body = thir::Body {
        owner: DefId::new(CrateId::from_raw(0), LocalDefId::from_raw(1)),
        params: vec![],
        return_ty: i32_ty,
        stmts: vec![],
        span: glyim_span::Span::DUMMY,
    };
    let closure_expr = b.expr(
        ExprKind::Closure {
            body: Box::new(closure_body),
            captures: vec![thir::Capture {
                local: x_var_id,
                kind: CaptureKind::ByValue,
                ty: i32_ty,
            }],
            is_move: false,
        },
        closure_ty,
    );
    stmts.push(thir::Stmt::Expr { expr: closure_expr });

    let body = b.into_body(stmts, vec![]);
    let result = lower_body(&mock, &body);

    assert!(
        !result.diagnostics.iter().any(|d| d.is_error()),
        "closure with by-value capture should not produce error diagnostics"
    );

    // Should have an Aggregate rvalue with Closure kind and 1 operand
    let closure_agg = result.body.basic_blocks.iter().find_map(|bb| {
        bb.statements.iter().find_map(|stmt| {
            if let StatementKind::Assign(
                _,
                Rvalue::Aggregate(AggregateKind::Closure(_, _), ref ops),
            ) = stmt.kind
            {
                Some(ops.len())
            } else {
                None
            }
        })
    });
    assert_eq!(
        closure_agg,
        Some(1),
        "expected closure Aggregate with 1 by-value captured operand, got {:?}",
        closure_agg
    );
}

/// Closure with multiple captures produces aggregate with matching operand count.
#[test]
fn closure_with_multiple_captures() {
    let mut ctx_mut = test_ty_ctx();
    let i32_ty = ctx_mut.mk_ty(TyKind::Int(IntTy::I32));
    let bool_ty = ctx_mut.bool_ty();
    let ref_bool_ty = ctx_mut.mk_ref(Region::Erased, bool_ty, Mutability::Not);

    let closure_id = ClosureId::from_raw(3);
    let closure_substs =
        ctx_mut.intern_substitution(vec![GenericArg::Ty(i32_ty), GenericArg::Ty(bool_ty)]);
    let closure_ty = ctx_mut.mk_ty(TyKind::Closure(closure_id, closure_substs));

    let interner = ctx_mut.resolver().clone();
    let ctx = ctx_mut.freeze();
    let mock = TestLowerCtx::new(&ctx);

    let mut b = ThirBuilder::new(closure_ty, interner.clone());
    let mut stmts = Vec::new();

    b.add_let_binding(
        "x",
        i32_ty,
        Some(b.expr(ExprKind::Literal(thir::Literal::Int(1, None)), i32_ty)),
        &mut stmts,
    );
    b.add_let_binding(
        "y",
        bool_ty,
        Some(b.expr(ExprKind::Literal(thir::Literal::Bool(true)), bool_ty)),
        &mut stmts,
    );

    let x_name = interner.intern("x");
    let x_var_id = *b.var_names.get(&x_name).expect("x should be in var_names");
    let y_name = interner.intern("y");
    let y_var_id = *b.var_names.get(&y_name).expect("y should be in var_names");

    let closure_body = thir::Body {
        owner: DefId::new(CrateId::from_raw(0), LocalDefId::from_raw(1)),
        params: vec![],
        return_ty: i32_ty,
        stmts: vec![],
        span: glyim_span::Span::DUMMY,
    };
    let closure_expr = b.expr(
        ExprKind::Closure {
            body: Box::new(closure_body),
            captures: vec![
                thir::Capture {
                    local: x_var_id,
                    kind: CaptureKind::ByValue,
                    ty: i32_ty,
                },
                thir::Capture {
                    local: y_var_id,
                    kind: CaptureKind::ByRef(Mutability::Not),
                    ty: ref_bool_ty,
                },
            ],
            is_move: false,
        },
        closure_ty,
    );
    stmts.push(thir::Stmt::Expr { expr: closure_expr });

    let body = b.into_body(stmts, vec![]);
    let result = lower_body(&mock, &body);

    assert!(
        !result.diagnostics.iter().any(|d| d.is_error()),
        "closure with multiple captures should not produce error diagnostics"
    );

    let closure_agg = result.body.basic_blocks.iter().find_map(|bb| {
        bb.statements.iter().find_map(|stmt| {
            if let StatementKind::Assign(
                _,
                Rvalue::Aggregate(AggregateKind::Closure(_, _), ref ops),
            ) = stmt.kind
            {
                Some(ops.len())
            } else {
                None
            }
        })
    });
    assert_eq!(
        closure_agg,
        Some(2),
        "expected closure Aggregate with 2 captured operands, got {:?}",
        closure_agg
    );
}

/// HIR-31 (operand-local): a `ByRef` capture must reference the *captured
/// variable's* MIR local, not `LocalIdx::from_raw(capture.local)`.
///
/// `capture.local` is a THIR `LocalVarId`, which is not aligned with the MIR
/// `LocalIdx` space. Before the fix the operand was
/// `Copy(Place { local: LocalIdx(0), .. })` — the function's *return place* —
/// so the closure captured the wrong storage entirely. Resolve through
/// `local_for_var` (which honours `local_var_map`) instead.
#[test]
fn closure_by_ref_capture_operand_uses_real_local() {
    let mut ctx_mut = test_ty_ctx();
    let i32_ty = ctx_mut.mk_ty(TyKind::Int(IntTy::I32));
    let ref_i32_ty = ctx_mut.mk_ref(Region::Erased, i32_ty, Mutability::Not);

    let closure_id = ClosureId::from_raw(9);
    let closure_substs = ctx_mut.intern_substitution(vec![GenericArg::Ty(i32_ty)]);
    let closure_ty = ctx_mut.mk_ty(TyKind::Closure(closure_id, closure_substs));

    let interner = ctx_mut.resolver().clone();
    let ctx = ctx_mut.freeze();
    let mock = TestLowerCtx::new(&ctx);

    let mut b = ThirBuilder::new(closure_ty, interner.clone());
    let mut stmts = Vec::new();
    b.add_let_binding(
        "x",
        i32_ty,
        Some(b.expr(ExprKind::Literal(thir::Literal::Int(42, None)), i32_ty)),
        &mut stmts,
    );
    let x_name = interner.intern("x");
    let x_var_id = *b.var_names.get(&x_name).expect("x should be in var_names");

    let closure_body = thir::Body {
        owner: DefId::new(CrateId::from_raw(0), LocalDefId::from_raw(1)),
        params: vec![],
        return_ty: i32_ty,
        stmts: vec![],
        span: glyim_span::Span::DUMMY,
    };
    let closure_expr = b.expr(
        ExprKind::Closure {
            body: Box::new(closure_body),
            captures: vec![thir::Capture {
                local: x_var_id,
                kind: CaptureKind::ByRef(Mutability::Not),
                ty: ref_i32_ty,
            }],
            is_move: false,
        },
        closure_ty,
    );
    stmts.push(thir::Stmt::Expr { expr: closure_expr });
    let body = b.into_body(stmts, vec![]);
    let result = lower_body(&mock, &body);

    let capture_local = result.body.basic_blocks.iter().find_map(|bb| {
        bb.statements.iter().find_map(|stmt| {
            if let StatementKind::Assign(
                _,
                Rvalue::Aggregate(AggregateKind::Closure(_, _), ref ops),
            ) = stmt.kind
                && let Some(glyim_mir::Operand::Copy(place)) = ops.first()
            {
                return Some(place.local);
            }
            None
        })
    });
    let local = capture_local.expect("closure aggregate with one Copy operand");
    assert_ne!(
        local,
        glyim_mir::LocalIdx::from_raw(0),
        "ByRef capture must not reference the return place (LocalIdx(0))"
    );
}
