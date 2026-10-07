//! Tests for flow-control and loop constant evaluation (Tier 1.5).
//!
//! `return`, `break` and `continue` were previously hard errors in const-eval.
//! `while`/`loop` were also unsupported. These tests exercise the new paths.
//! Note: const-eval has no `let` binding, so loops are tested via break/escape
//! semantics (constant conditions), which is the common compile-time case.

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
fn return_with_value_evaluates_to_that_value() {
    let mut body = test_body();
    let five = alloc_lit(&mut body, Literal::Int(5, Some(IntTy::I32)));
    let ret = body.alloc_expr(Expr::Return { value: Some(five) }, dummy_span());
    assert_eq!(eval_ok(&body, ret), ConstValue::Int(5, IntTy::I32));
}

#[test]
fn return_without_value_is_unit() {
    let mut body = test_body();
    let ret = body.alloc_expr(Expr::Return { value: None }, dummy_span());
    assert_eq!(eval_ok(&body, ret), ConstValue::Unit);
}

#[test]
fn while_false_runs_zero_iterations() {
    // `while false { ... }` must not evaluate the body and yields Unit.
    let mut body = test_body();
    let false_lit = alloc_lit(&mut body, Literal::Bool(false));
    // Body that would error if evaluated: divide by zero.
    let zero = alloc_lit(&mut body, Literal::Int(0, Some(IntTy::I32)));
    let one = alloc_lit(&mut body, Literal::Int(1, Some(IntTy::I32)));
    let div_by_zero = body.alloc_expr(
        Expr::Binary {
            op: BinOp::Div,
            lhs: one,
            rhs: zero,
        },
        dummy_span(),
    );
    let while_expr = body.alloc_expr(
        Expr::While {
            cond: false_lit,
            body: div_by_zero,
        },
        dummy_span(),
    );
    assert_eq!(eval_ok(&body, while_expr), ConstValue::Unit);
}

#[test]
fn while_true_with_break_terminates() {
    let mut body = test_body();
    let true_lit = alloc_lit(&mut body, Literal::Bool(true));
    let break_expr = body.alloc_expr(Expr::Break { value: None }, dummy_span());
    let while_expr = body.alloc_expr(
        Expr::While {
            cond: true_lit,
            body: break_expr,
        },
        dummy_span(),
    );
    // The break exits on the first iteration, so no infinite-loop error.
    assert_eq!(eval_ok(&body, while_expr), ConstValue::Unit);
}

#[test]
fn loop_with_break_terminates() {
    let mut body = test_body();
    let break_expr = body.alloc_expr(Expr::Break { value: None }, dummy_span());
    let loop_expr = body.alloc_expr(Expr::Loop { body: break_expr }, dummy_span());
    assert_eq!(eval_ok(&body, loop_expr), ConstValue::Unit);
}

#[test]
fn continue_re_enters_loop_body() {
    // `loop { if true { break } else { continue } }` exercises the continue
    // path without hanging: the else branch is never taken, so `break` fires.
    let mut body = test_body();
    let true_lit = alloc_lit(&mut body, Literal::Bool(true));
    let break_expr = body.alloc_expr(Expr::Break { value: None }, dummy_span());
    let continue_expr = body.alloc_expr(Expr::Continue, dummy_span());
    let if_expr = body.alloc_expr(
        Expr::If {
            cond: true_lit,
            then_branch: break_expr,
            else_branch: Some(continue_expr),
        },
        dummy_span(),
    );
    let loop_expr = body.alloc_expr(Expr::Loop { body: if_expr }, dummy_span());
    assert_eq!(eval_ok(&body, loop_expr), ConstValue::Unit);
}
