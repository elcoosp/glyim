//! Tests for recursion depth limits and error conditions.

use crate::{ConstEvalError, ConstEvaluator};
use glyim_core::arena::IndexVec;
use glyim_core::def_id::LocalDefId;
use glyim_core::primitives::{BinOp, IntTy};
use glyim_hir::{Body, Expr, ExprId, Literal};
use glyim_span::{ByteIdx, FileId, Span};

fn dummy_span() -> Span {
    Span::new(
        FileId::BOGUS,
        ByteIdx::ZERO,
        ByteIdx::ZERO,
        glyim_span::SyntaxContext::ROOT,
    )
}

fn test_body() -> Body {
    Body {
        owner: LocalDefId::from_raw(0),
        exprs: IndexVec::new(),
        pats: IndexVec::new(),
        pat_spans: glyim_core::arena::IndexVec::new(),
        params: Vec::new(),
        span: dummy_span(),
        expr_spans: IndexVec::new(),
    }
}

fn alloc_lit(body: &mut Body, lit: Literal) -> ExprId {
    body.alloc_expr(Expr::Literal(lit), dummy_span())
}

fn eval_err(body: &Body, expr_id: ExprId) -> ConstEvalError {
    let mut evaluator = ConstEvaluator::new(body);
    evaluator
        .evaluate(expr_id)
        .expect_err("const evaluation should fail")
}

#[test]
fn overflow_on_add_produces_error() {
    let mut body = test_body();
    let max_val = alloc_lit(&mut body, Literal::Int(i32::MAX as i128, Some(IntTy::I32)));
    let one = alloc_lit(&mut body, Literal::Int(1, Some(IntTy::I32)));
    let add = body.alloc_expr(
        Expr::Binary {
            op: BinOp::Add,
            lhs: max_val,
            rhs: one,
        },
        dummy_span(),
    );

    let err = eval_err(&body, add);
    assert!(
        err.message.contains("overflow"),
        "Expected overflow error, got: {}",
        err.message
    );
}

#[test]
fn overflow_on_negate_min_int_produces_error() {
    let mut body = test_body();
    let min_val = alloc_lit(&mut body, Literal::Int(i32::MIN as i128, Some(IntTy::I32)));
    let neg = body.alloc_expr(
        Expr::Unary {
            op: glyim_core::primitives::UnOp::Neg,
            expr: min_val,
        },
        dummy_span(),
    );

    let err = eval_err(&body, neg);
    assert!(
        err.message.contains("overflow"),
        "Expected overflow error, got: {}",
        err.message
    );
}

#[test]
fn deeply_nested_expression_hits_depth_limit() {
    let mut body = test_body();

    let max_depth = 130;
    let one = alloc_lit(&mut body, Literal::Int(1, Some(IntTy::I32)));

    let mut current = one;
    for _ in 0..max_depth {
        let next_one = alloc_lit(&mut body, Literal::Int(1, Some(IntTy::I32)));
        current = body.alloc_expr(
            Expr::Binary {
                op: BinOp::Add,
                lhs: current,
                rhs: next_one,
            },
            dummy_span(),
        );
    }

    let err = eval_err(&body, current);
    assert!(
        err.message.contains("recursion") || err.message.contains("limit"),
        "Expected recursion limit error, got: {}",
        err.message
    );
}

#[test]
fn error_expression_is_rejected() {
    let mut body = test_body();
    let err_expr = body.alloc_expr(Expr::Err, dummy_span());

    let err = eval_err(&body, err_expr);
    assert!(
        err.message.contains("error"),
        "Expected error expression rejection, got: {}",
        err.message
    );
}

#[test]
fn deref_is_not_supported() {
    let mut body = test_body();
    let val = alloc_lit(&mut body, Literal::Int(1, Some(IntTy::I32)));
    let deref = body.alloc_expr(
        Expr::Unary {
            op: glyim_core::primitives::UnOp::Deref,
            expr: val,
        },
        dummy_span(),
    );

    let err = eval_err(&body, deref);
    assert!(
        err.message.contains("dereference") || err.message.contains("not supported"),
        "Expected deref not supported error, got: {}",
        err.message
    );
}
