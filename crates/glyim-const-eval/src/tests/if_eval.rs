//! Tests for `if` expression constant evaluation (W1-C03-T02).

use crate::{ConstEvalError, ConstEvaluator, ConstValue};
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

fn eval_ok(body: &Body, expr_id: ExprId) -> ConstValue {
    let mut evaluator = ConstEvaluator::new(body);
    evaluator
        .evaluate(expr_id)
        .expect("const evaluation should succeed")
}

fn eval_err(body: &Body, expr_id: ExprId) -> ConstEvalError {
    let mut evaluator = ConstEvaluator::new(body);
    evaluator
        .evaluate(expr_id)
        .expect_err("const evaluation should fail")
}

#[test]
fn t02_if_true_takes_then_branch() {
    let mut body = test_body();

    let cond = alloc_lit(&mut body, Literal::Bool(true));
    let then_val = alloc_lit(&mut body, Literal::Int(5, Some(IntTy::I32)));
    let else_val = alloc_lit(&mut body, Literal::Int(6, Some(IntTy::I32)));

    let if_expr = body.alloc_expr(
        Expr::If {
            cond,
            then_branch: then_val,
            else_branch: Some(else_val),
        },
        dummy_span(),
    );

    let result = eval_ok(&body, if_expr);
    assert_eq!(result, ConstValue::Int(5, IntTy::I32));
}

#[test]
fn t02_if_false_takes_else_branch() {
    let mut body = test_body();

    let cond = alloc_lit(&mut body, Literal::Bool(false));
    let then_val = alloc_lit(&mut body, Literal::Int(5, Some(IntTy::I32)));
    let else_val = alloc_lit(&mut body, Literal::Int(6, Some(IntTy::I32)));

    let if_expr = body.alloc_expr(
        Expr::If {
            cond,
            then_branch: then_val,
            else_branch: Some(else_val),
        },
        dummy_span(),
    );

    let result = eval_ok(&body, if_expr);
    assert_eq!(result, ConstValue::Int(6, IntTy::I32));
}

#[test]
fn t02_if_no_else_returns_unit() {
    let mut body = test_body();

    let cond = alloc_lit(&mut body, Literal::Bool(false));
    let then_val = alloc_lit(&mut body, Literal::Int(5, Some(IntTy::I32)));

    let if_expr = body.alloc_expr(
        Expr::If {
            cond,
            then_branch: then_val,
            else_branch: None,
        },
        dummy_span(),
    );

    let result = eval_ok(&body, if_expr);
    assert_eq!(result, ConstValue::Unit);
}

#[test]
fn t02_if_with_computed_condition() {
    let mut body = test_body();

    let one_a = alloc_lit(&mut body, Literal::Int(1, Some(IntTy::I32)));
    let one_b = alloc_lit(&mut body, Literal::Int(1, Some(IntTy::I32)));
    let cond = body.alloc_expr(
        Expr::Binary {
            op: BinOp::Eq,
            lhs: one_a,
            rhs: one_b,
        },
        dummy_span(),
    );
    let then_val = alloc_lit(&mut body, Literal::Int(42, Some(IntTy::I32)));
    let else_val = alloc_lit(&mut body, Literal::Int(0, Some(IntTy::I32)));

    let if_expr = body.alloc_expr(
        Expr::If {
            cond,
            then_branch: then_val,
            else_branch: Some(else_val),
        },
        dummy_span(),
    );

    let result = eval_ok(&body, if_expr);
    assert_eq!(result, ConstValue::Int(42, IntTy::I32));
}

#[test]
fn t02_if_non_bool_condition_is_error() {
    let mut body = test_body();

    let cond = alloc_lit(&mut body, Literal::Int(1, Some(IntTy::I32)));
    let then_val = alloc_lit(&mut body, Literal::Int(5, Some(IntTy::I32)));
    let else_val = alloc_lit(&mut body, Literal::Int(6, Some(IntTy::I32)));

    let if_expr = body.alloc_expr(
        Expr::If {
            cond,
            then_branch: then_val,
            else_branch: Some(else_val),
        },
        dummy_span(),
    );

    let err = eval_err(&body, if_expr);
    assert!(
        err.message.contains("boolean") || err.message.contains("condition"),
        "Expected boolean condition error, got: {}",
        err.message
    );
}

#[test]
fn t02_nested_if() {
    let mut body = test_body();

    let inner_cond = alloc_lit(&mut body, Literal::Bool(false));
    let inner_then = alloc_lit(&mut body, Literal::Int(1, Some(IntTy::I32)));
    let inner_else = alloc_lit(&mut body, Literal::Int(2, Some(IntTy::I32)));
    let inner_if = body.alloc_expr(
        Expr::If {
            cond: inner_cond,
            then_branch: inner_then,
            else_branch: Some(inner_else),
        },
        dummy_span(),
    );

    let outer_cond = alloc_lit(&mut body, Literal::Bool(true));
    let outer_else = alloc_lit(&mut body, Literal::Int(3, Some(IntTy::I32)));
    let outer_if = body.alloc_expr(
        Expr::If {
            cond: outer_cond,
            then_branch: inner_if,
            else_branch: Some(outer_else),
        },
        dummy_span(),
    );

    let result = eval_ok(&body, outer_if);
    assert_eq!(result, ConstValue::Int(2, IntTy::I32));
}
