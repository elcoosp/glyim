//! Tests for `Expr::Call` constant evaluation (plan §4.2).
//!
//! The evaluator resolves a path-named callee to a builtin `const fn` (see
//! `ConstFn`) and evaluates its const arguments. These tests pin down the
//! supported builtins and the error path for unknown names.

use crate::{ConstEvalError, ConstEvaluator, ConstValue};
use glyim_core::arena::IndexVec;
use glyim_core::def_id::LocalDefId;
use glyim_core::interner::{Interner, Name};
use glyim_core::primitives::{IntTy, UintTy};
use glyim_hir::{Body, Expr, ExprId, Literal, Path};
use glyim_span::{ByteIdx, FileId, Span};
use std::sync::OnceLock;

/// A single shared interner for the module so distinct identifiers stay
/// distinct (see `for_eval.rs` for why a per-call `Interner::new` collides).
fn interner() -> &'static Interner {
    static I: OnceLock<Interner> = OnceLock::new();
    I.get_or_init(Interner::new)
}

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

fn name(s: &str) -> Name {
    interner().intern(s)
}

/// Build `name(arg0, arg1, ...)` as an `Expr::Call` over literal args.
fn call_lits(body: &mut Body, fn_name: &str, arg_lits: Vec<Literal>) -> ExprId {
    let func = body.alloc_expr(Expr::Path(Path::from_single(name(fn_name))), dummy_span());
    let args = arg_lits
        .into_iter()
        .map(|l| alloc_lit(body, l))
        .collect::<Vec<_>>();
    body.alloc_expr(Expr::Call { func, args }, dummy_span())
}

fn eval_ok(body: &Body, expr_id: ExprId) -> ConstValue {
    let mut ev = ConstEvaluator::new(body).with_interner(interner());
    ev.evaluate(expr_id)
        .expect("const evaluation of the call should succeed")
}

fn eval_err(body: &Body, expr_id: ExprId) -> ConstEvalError {
    let mut ev = ConstEvaluator::new(body).with_interner(interner());
    ev.evaluate(expr_id)
        .expect_err("const evaluation of the call should fail")
}

#[test]
fn abs_of_negative() {
    let mut body = test_body();
    let c = call_lits(&mut body, "abs", vec![Literal::Int(-7, Some(IntTy::I32))]);
    assert_eq!(eval_ok(&body, c), ConstValue::Int(7, IntTy::I32));
}

#[test]
fn min_two_ints() {
    let mut body = test_body();
    let c = call_lits(
        &mut body,
        "min",
        vec![
            Literal::Int(3, Some(IntTy::I32)),
            Literal::Int(8, Some(IntTy::I32)),
        ],
    );
    assert_eq!(eval_ok(&body, c), ConstValue::Int(3, IntTy::I32));
}

#[test]
fn max_two_ints() {
    let mut body = test_body();
    let c = call_lits(
        &mut body,
        "max",
        vec![
            Literal::Int(3, Some(IntTy::I32)),
            Literal::Int(8, Some(IntTy::I32)),
        ],
    );
    assert_eq!(eval_ok(&body, c), ConstValue::Int(8, IntTy::I32));
}

#[test]
fn sqrt_of_unsigned() {
    let mut body = test_body();
    let c = call_lits(
        &mut body,
        "sqrt",
        vec![Literal::Uint(144, Some(UintTy::U32))],
    );
    assert_eq!(eval_ok(&body, c), ConstValue::Uint(12, UintTy::U32));
}

#[test]
fn is_power_of_two_true() {
    let mut body = test_body();
    let c = call_lits(
        &mut body,
        "is_power_of_two",
        vec![Literal::Uint(16, Some(UintTy::U32))],
    );
    assert_eq!(eval_ok(&body, c), ConstValue::Bool(true));
}

#[test]
fn is_power_of_two_false() {
    let mut body = test_body();
    let c = call_lits(
        &mut body,
        "is_power_of_two",
        vec![Literal::Uint(15, Some(UintTy::U32))],
    );
    assert_eq!(eval_ok(&body, c), ConstValue::Bool(false));
}

#[test]
fn call_result_used_in_binary() {
    // `min(10, 4) + 1 == 5`
    let mut body = test_body();
    let inner = call_lits(
        &mut body,
        "min",
        vec![
            Literal::Int(10, Some(IntTy::I32)),
            Literal::Int(4, Some(IntTy::I32)),
        ],
    );
    let one = alloc_lit(&mut body, Literal::Int(1, Some(IntTy::I32)));
    let sum = body.alloc_expr(
        Expr::Binary {
            op: glyim_core::primitives::BinOp::Add,
            lhs: inner,
            rhs: one,
        },
        dummy_span(),
    );
    assert_eq!(eval_ok(&body, sum), ConstValue::Int(5, IntTy::I32));
}

#[test]
fn unknown_const_fn_errors() {
    let mut body = test_body();
    let c = call_lits(
        &mut body,
        "frobnicate",
        vec![Literal::Int(1, Some(IntTy::I32))],
    );
    let err = eval_err(&body, c);
    assert!(
        err.message.contains("unknown const fn"),
        "expected unknown-const-fn error, got: {}",
        err.message
    );
}
