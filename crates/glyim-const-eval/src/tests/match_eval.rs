//! Tests for `match` expression constant evaluation (W1-C03-T03).

use crate::{ConstEvalError, ConstEvaluator, ConstValue};
use glyim_core::arena::IndexVec;
use glyim_core::def_id::LocalDefId;
use glyim_core::primitives::{IntTy, UintTy};
use glyim_hir::{Body, Expr, ExprId, Literal, MatchArm, Pat, PatId};
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

fn alloc_pat(body: &mut Body, pat: Pat) -> PatId {
    body.pats.push(pat)
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
fn t03_match_literal_pattern_matches() {
    let mut body = test_body();

    let scrutinee = alloc_lit(&mut body, Literal::Int(3, Some(IntTy::I32)));

    let pat_1 = alloc_pat(&mut body, Pat::Literal(Literal::Int(1, Some(IntTy::I32))));
    let val_10 = alloc_lit(&mut body, Literal::Int(10, Some(IntTy::I32)));

    let pat_2 = alloc_pat(&mut body, Pat::Literal(Literal::Int(2, Some(IntTy::I32))));
    let val_20 = alloc_lit(&mut body, Literal::Int(20, Some(IntTy::I32)));

    let pat_3 = alloc_pat(&mut body, Pat::Literal(Literal::Int(3, Some(IntTy::I32))));
    let val_30 = alloc_lit(&mut body, Literal::Int(30, Some(IntTy::I32)));

    let match_expr = body.alloc_expr(
        Expr::Match {
            scrutinee,
            arms: vec![
                MatchArm {
                    pat: pat_1,
                    guard: None,
                    body: val_10,
                },
                MatchArm {
                    pat: pat_2,
                    guard: None,
                    body: val_20,
                },
                MatchArm {
                    pat: pat_3,
                    guard: None,
                    body: val_30,
                },
            ],
        },
        dummy_span(),
    );

    let result = eval_ok(&body, match_expr);
    assert_eq!(result, ConstValue::Int(30, IntTy::I32));
}

#[test]
fn t03_match_const_expr_pattern() {
    let mut body = test_body();

    let scrutinee = alloc_lit(&mut body, Literal::Int(3, Some(IntTy::I32)));

    let pat_3 = alloc_pat(&mut body, Pat::Literal(Literal::Int(3, Some(IntTy::I32))));
    let result_val = alloc_lit(&mut body, Literal::Int(42, Some(IntTy::I32)));

    let match_expr = body.alloc_expr(
        Expr::Match {
            scrutinee,
            arms: vec![MatchArm {
                pat: pat_3,
                guard: None,
                body: result_val,
            }],
        },
        dummy_span(),
    );

    let result = eval_ok(&body, match_expr);
    assert_eq!(result, ConstValue::Int(42, IntTy::I32));
}

#[test]
fn t03_match_wildcard_pattern() {
    let mut body = test_body();

    let scrutinee = alloc_lit(&mut body, Literal::Int(99, Some(IntTy::I32)));

    let pat_1 = alloc_pat(&mut body, Pat::Literal(Literal::Int(1, Some(IntTy::I32))));
    let val_10 = alloc_lit(&mut body, Literal::Int(10, Some(IntTy::I32)));

    let pat_wild = alloc_pat(&mut body, Pat::Wild);
    let val_20 = alloc_lit(&mut body, Literal::Int(20, Some(IntTy::I32)));

    let match_expr = body.alloc_expr(
        Expr::Match {
            scrutinee,
            arms: vec![
                MatchArm {
                    pat: pat_1,
                    guard: None,
                    body: val_10,
                },
                MatchArm {
                    pat: pat_wild,
                    guard: None,
                    body: val_20,
                },
            ],
        },
        dummy_span(),
    );

    let result = eval_ok(&body, match_expr);
    assert_eq!(result, ConstValue::Int(20, IntTy::I32));
}

#[test]
fn t03_match_or_pattern() {
    let mut body = test_body();

    let scrutinee = alloc_lit(&mut body, Literal::Int(2, Some(IntTy::I32)));

    let pat_1 = alloc_pat(&mut body, Pat::Literal(Literal::Int(1, Some(IntTy::I32))));
    let pat_2 = alloc_pat(&mut body, Pat::Literal(Literal::Int(2, Some(IntTy::I32))));
    let or_pat = alloc_pat(&mut body, Pat::Or(vec![pat_1, pat_2]));
    let val_100 = alloc_lit(&mut body, Literal::Int(100, Some(IntTy::I32)));

    let pat_wild = alloc_pat(&mut body, Pat::Wild);
    let val_0 = alloc_lit(&mut body, Literal::Int(0, Some(IntTy::I32)));

    let match_expr = body.alloc_expr(
        Expr::Match {
            scrutinee,
            arms: vec![
                MatchArm {
                    pat: or_pat,
                    guard: None,
                    body: val_100,
                },
                MatchArm {
                    pat: pat_wild,
                    guard: None,
                    body: val_0,
                },
            ],
        },
        dummy_span(),
    );

    let result = eval_ok(&body, match_expr);
    assert_eq!(result, ConstValue::Int(100, IntTy::I32));
}

#[test]
fn t03_match_non_exhaustive_is_error() {
    let mut body = test_body();

    let scrutinee = alloc_lit(&mut body, Literal::Int(5, Some(IntTy::I32)));

    let pat_1 = alloc_pat(&mut body, Pat::Literal(Literal::Int(1, Some(IntTy::I32))));
    let val_10 = alloc_lit(&mut body, Literal::Int(10, Some(IntTy::I32)));

    let pat_2 = alloc_pat(&mut body, Pat::Literal(Literal::Int(2, Some(IntTy::I32))));
    let val_20 = alloc_lit(&mut body, Literal::Int(20, Some(IntTy::I32)));

    let match_expr = body.alloc_expr(
        Expr::Match {
            scrutinee,
            arms: vec![
                MatchArm {
                    pat: pat_1,
                    guard: None,
                    body: val_10,
                },
                MatchArm {
                    pat: pat_2,
                    guard: None,
                    body: val_20,
                },
            ],
        },
        dummy_span(),
    );

    let err = eval_err(&body, match_expr);
    assert!(
        err.message.contains("non-exhaustive") || err.message.contains("exhaustive"),
        "Expected non-exhaustive match error, got: {}",
        err.message
    );
}

#[test]
fn t03_match_bool_scrutinee() {
    let mut body = test_body();

    let scrutinee = alloc_lit(&mut body, Literal::Bool(true));

    let pat_true = alloc_pat(&mut body, Pat::Literal(Literal::Bool(true)));
    let val_1 = alloc_lit(&mut body, Literal::Int(1, Some(IntTy::I32)));

    let pat_false = alloc_pat(&mut body, Pat::Literal(Literal::Bool(false)));
    let val_0 = alloc_lit(&mut body, Literal::Int(0, Some(IntTy::I32)));

    let match_expr = body.alloc_expr(
        Expr::Match {
            scrutinee,
            arms: vec![
                MatchArm {
                    pat: pat_true,
                    guard: None,
                    body: val_1,
                },
                MatchArm {
                    pat: pat_false,
                    guard: None,
                    body: val_0,
                },
            ],
        },
        dummy_span(),
    );

    let result = eval_ok(&body, match_expr);
    assert_eq!(result, ConstValue::Int(1, IntTy::I32));
}

#[test]
fn t03_match_uint_pattern() {
    let mut body = test_body();

    let scrutinee = alloc_lit(&mut body, Literal::Uint(42, Some(UintTy::U32)));

    let pat_0 = alloc_pat(&mut body, Pat::Literal(Literal::Uint(0, Some(UintTy::U32))));
    let val_1 = alloc_lit(&mut body, Literal::Int(1, Some(IntTy::I32)));

    let pat_42 = alloc_pat(
        &mut body,
        Pat::Literal(Literal::Uint(42, Some(UintTy::U32))),
    );
    let val_99 = alloc_lit(&mut body, Literal::Int(99, Some(IntTy::I32)));

    let match_expr = body.alloc_expr(
        Expr::Match {
            scrutinee,
            arms: vec![
                MatchArm {
                    pat: pat_0,
                    guard: None,
                    body: val_1,
                },
                MatchArm {
                    pat: pat_42,
                    guard: None,
                    body: val_99,
                },
            ],
        },
        dummy_span(),
    );

    let result = eval_ok(&body, match_expr);
    assert_eq!(result, ConstValue::Int(99, IntTy::I32));
}
