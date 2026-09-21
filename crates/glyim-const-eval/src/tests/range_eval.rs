//! Tests for `Expr::Range` constant evaluation (plan §4.4).

use crate::{ConstEvaluator, ConstValue};
use glyim_core::arena::IndexVec;
use glyim_core::def_id::LocalDefId;
use glyim_core::primitives::IntTy;
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
fn range_exclusive_evaluates_bounds() {
    let mut body = test_body();
    let start = alloc_lit(&mut body, Literal::Int(0, Some(IntTy::I32)));
    let end = alloc_lit(&mut body, Literal::Int(10, Some(IntTy::I32)));
    let range = body.alloc_expr(
        Expr::Range {
            start: Some(start),
            end: Some(end),
            inclusive: false,
        },
        dummy_span(),
    );

    let val = eval_ok(&body, range);
    match val {
        ConstValue::Range(Some(s), Some(e), incl) => {
            assert_eq!(s.as_ref(), &ConstValue::Int(0, IntTy::I32));
            assert_eq!(e.as_ref(), &ConstValue::Int(10, IntTy::I32));
            assert!(!incl, "exclusive range must have inclusive=false");
        }
        other => panic!("expected ConstValue::Range, got {:?}", other),
    }
}

#[test]
fn range_inclusive_sets_flag() {
    let mut body = test_body();
    let start = alloc_lit(&mut body, Literal::Int(0, Some(IntTy::I32)));
    let end = alloc_lit(&mut body, Literal::Int(5, Some(IntTy::I32)));
    let range = body.alloc_expr(
        Expr::Range {
            start: Some(start),
            end: Some(end),
            inclusive: true,
        },
        dummy_span(),
    );

    let val = eval_ok(&body, range);
    match val {
        ConstValue::Range(_, _, incl) => assert!(incl, "inclusive range must have inclusive=true"),
        other => panic!("expected ConstValue::Range, got {:?}", other),
    }
}

#[test]
fn range_open_ended_has_no_bounds() {
    let mut body = test_body();
    // `..5` — no start, end present.
    let end = alloc_lit(&mut body, Literal::Int(5, Some(IntTy::I32)));
    let range = body.alloc_expr(
        Expr::Range {
            start: None,
            end: Some(end),
            inclusive: false,
        },
        dummy_span(),
    );

    let val = eval_ok(&body, range);
    match val {
        ConstValue::Range(None, Some(_), false) => {}
        other => panic!(
            "expected open-ended range with only an end bound, got {:?}",
            other
        ),
    }
}
