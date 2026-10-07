//! Tests for block expression constant evaluation.

use crate::{ConstEvaluator, ConstValue};
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

#[test]
fn block_with_tail_expression() {
    let mut body = test_body();

    let one = alloc_lit(&mut body, Literal::Int(1, Some(IntTy::I32)));
    let two = alloc_lit(&mut body, Literal::Int(2, Some(IntTy::I32)));
    let three = alloc_lit(&mut body, Literal::Int(3, Some(IntTy::I32)));

    let block = body.alloc_expr(
        Expr::Block {
            stmts: vec![one, two],
            tail: Some(three),
        },
        dummy_span(),
    );

    let result = eval_ok(&body, block);
    assert_eq!(result, ConstValue::Int(3, IntTy::I32));
}

#[test]
fn block_without_tail_returns_unit() {
    let mut body = test_body();

    let one = alloc_lit(&mut body, Literal::Int(1, Some(IntTy::I32)));
    let two = alloc_lit(&mut body, Literal::Int(2, Some(IntTy::I32)));

    let block = body.alloc_expr(
        Expr::Block {
            stmts: vec![one, two],
            tail: None,
        },
        dummy_span(),
    );

    let result = eval_ok(&body, block);
    assert_eq!(result, ConstValue::Unit);
}

#[test]
fn empty_block_returns_unit() {
    let mut body = test_body();

    let block = body.alloc_expr(
        Expr::Block {
            stmts: vec![],
            tail: None,
        },
        dummy_span(),
    );

    let result = eval_ok(&body, block);
    assert_eq!(result, ConstValue::Unit);
}

#[test]
fn block_with_arithmetic_tail() {
    let mut body = test_body();

    let one = alloc_lit(&mut body, Literal::Int(1, Some(IntTy::I32)));
    let two = alloc_lit(&mut body, Literal::Int(2, Some(IntTy::I32)));
    let add = body.alloc_expr(
        Expr::Binary {
            op: BinOp::Add,
            lhs: one,
            rhs: two,
        },
        dummy_span(),
    );

    let block = body.alloc_expr(
        Expr::Block {
            stmts: vec![],
            tail: Some(add),
        },
        dummy_span(),
    );

    let result = eval_ok(&body, block);
    assert_eq!(result, ConstValue::Int(3, IntTy::I32));
}

#[test]
fn empty_tuple_is_unit() {
    let mut body = test_body();
    let tuple = body.alloc_expr(Expr::Tuple(vec![]), dummy_span());

    let result = eval_ok(&body, tuple);
    assert_eq!(result, ConstValue::Unit);
}
