//! Tests for function invocation in const evaluation (plan §4.2 / §4.3):
//! method-call syntax over builtin `const fn`s, immediately-invoked closures,
//! and user-defined `const fn`s with a body in the same `Body` arena.

use crate::{BodyFn, ConstEvaluator, ConstValue};
use glyim_core::arena::IndexVec;
use glyim_core::def_id::LocalDefId;
use glyim_core::interner::{Interner, Name};
use glyim_core::primitives::{BinOp, IntTy, Mutability, UintTy};
use glyim_hir::{Body, Expr, ExprId, Literal, Pat, Path};
use glyim_span::{ByteIdx, FileId, Span};
use std::sync::OnceLock;

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
        pat_spans: glyim_core::arena::IndexVec::new(),
        params: Vec::new(),
        span: dummy_span(),
        expr_spans: IndexVec::new(),
    }
}

fn name(s: &str) -> Name {
    interner().intern(s)
}

fn alloc_lit(body: &mut Body, lit: Literal) -> ExprId {
    body.alloc_expr(Expr::Literal(lit), dummy_span())
}

/// A path expression naming `s`.
fn path_expr(body: &mut Body, s: &str) -> ExprId {
    body.alloc_expr(Expr::Path(Path::from_single(name(s))), dummy_span())
}

/// A binary expression `lhs op rhs` allocated into `body`.
fn binary(body: &mut Body, op: BinOp, lhs: ExprId, rhs: ExprId) -> ExprId {
    body.alloc_expr(Expr::Binary { op, lhs, rhs }, dummy_span())
}

/// A `Name`-binding pattern allocated into `body`.
fn binding_pat(body: &mut Body, s: &str) -> glyim_hir::PatId {
    body.pats.push(Pat::Binding {
        name: name(s),
        mutability: Mutability::Not,
        subpattern: None,
    })
}

fn eval(body: &Body, expr_id: ExprId) -> ConstValue {
    let mut ev = ConstEvaluator::new(body).with_interner(interner());
    ev.evaluate(expr_id)
        .expect("const evaluation should succeed")
}

fn eval_err(body: &Body, expr_id: ExprId) -> crate::ConstEvalError {
    let mut ev = ConstEvaluator::new(body).with_interner(interner());
    ev.evaluate(expr_id)
        .expect_err("const evaluation should fail")
}

// ---- Method calls (§4.2): builtin const fns as methods ----

#[test]
fn method_abs_on_negative() {
    let mut body = test_body();
    let recv = alloc_lit(&mut body, Literal::Int(-7, Some(IntTy::I32)));
    let m = body.alloc_expr(
        Expr::MethodCall {
            receiver: recv,
            method: name("abs"),
            args: vec![],
            generic_args: None,
        },
        dummy_span(),
    );
    assert_eq!(eval(&body, m), ConstValue::Int(7, IntTy::I32));
}

#[test]
fn method_min_two_ints() {
    let mut body = test_body();
    let recv = alloc_lit(&mut body, Literal::Int(10, Some(IntTy::I32)));
    let arg = alloc_lit(&mut body, Literal::Int(4, Some(IntTy::I32)));
    let m = body.alloc_expr(
        Expr::MethodCall {
            receiver: recv,
            method: name("min"),
            args: vec![arg],
            generic_args: None,
        },
        dummy_span(),
    );
    assert_eq!(eval(&body, m), ConstValue::Int(4, IntTy::I32));
}

#[test]
fn method_sqrt_on_uint() {
    let mut body = test_body();
    let recv = alloc_lit(&mut body, Literal::Uint(64, Some(UintTy::U32)));
    let m = body.alloc_expr(
        Expr::MethodCall {
            receiver: recv,
            method: name("sqrt"),
            args: vec![],
            generic_args: None,
        },
        dummy_span(),
    );
    assert_eq!(eval(&body, m), ConstValue::Uint(8, UintTy::U32));
}

#[test]
fn unknown_method_errors() {
    let mut body = test_body();
    let recv = alloc_lit(&mut body, Literal::Int(1, Some(IntTy::I32)));
    let m = body.alloc_expr(
        Expr::MethodCall {
            receiver: recv,
            method: name("frobnicate"),
            args: vec![],
            generic_args: None,
        },
        dummy_span(),
    );
    let err = eval_err(&body, m);
    assert!(
        err.message.contains("unknown const method"),
        "expected unknown-method error, got: {}",
        err.message
    );
}

// ---- Immediately-invoked closures (§4.3) ----

#[test]
fn closure_iife_adds_one() {
    // (|x| x + 1)(5) == 6
    let mut body = test_body();
    let x_pat = binding_pat(&mut body, "x");
    let x = path_expr(&mut body, "x");
    let one = alloc_lit(&mut body, Literal::Int(1, Some(IntTy::I32)));
    let body_expr = binary(&mut body, BinOp::Add, x, one);
    let closure = body.alloc_expr(
        Expr::Closure {
            params: vec![x_pat],
            body: body_expr,
            is_move: false,
        },
        dummy_span(),
    );
    let arg = alloc_lit(&mut body, Literal::Int(5, Some(IntTy::I32)));
    let call = body.alloc_expr(
        Expr::Call {
            func: closure,
            args: vec![arg],
        },
        dummy_span(),
    );
    assert_eq!(eval(&body, call), ConstValue::Int(6, IntTy::I32));
}

#[test]
fn closure_iife_multi_param() {
    // (|a, b| a * b)(3, 4) == 12
    let mut body = test_body();
    let a_pat = binding_pat(&mut body, "a");
    let b_pat = binding_pat(&mut body, "b");
    let a = path_expr(&mut body, "a");
    let b = path_expr(&mut body, "b");
    let body_expr = binary(&mut body, BinOp::Mul, a, b);
    let closure = body.alloc_expr(
        Expr::Closure {
            params: vec![a_pat, b_pat],
            body: body_expr,
            is_move: false,
        },
        dummy_span(),
    );
    let a_arg = alloc_lit(&mut body, Literal::Int(3, Some(IntTy::I32)));
    let b_arg = alloc_lit(&mut body, Literal::Int(4, Some(IntTy::I32)));
    let call = body.alloc_expr(
        Expr::Call {
            func: closure,
            args: vec![a_arg, b_arg],
        },
        dummy_span(),
    );
    assert_eq!(eval(&body, call), ConstValue::Int(12, IntTy::I32));
}

#[test]
fn bare_closure_errors() {
    let mut body = test_body();
    let x_pat = binding_pat(&mut body, "x");
    let x = path_expr(&mut body, "x");
    let one = alloc_lit(&mut body, Literal::Int(1, Some(IntTy::I32)));
    let body_expr = binary(&mut body, BinOp::Add, x, one);
    let closure = body.alloc_expr(
        Expr::Closure {
            params: vec![x_pat],
            body: body_expr,
            is_move: false,
        },
        dummy_span(),
    );
    let err = eval_err(&body, closure);
    assert!(
        err.message.contains("bare closure"),
        "expected bare-closure error, got: {}",
        err.message
    );
}

// ---- User-defined const fns (§4.2) ----

#[test]
fn user_const_fn_body() {
    // const fn add_one(x) { x + 1 }; add_one(5) == 6
    let mut body = test_body();
    let x_pat = binding_pat(&mut body, "x");
    let x = path_expr(&mut body, "x");
    let one = alloc_lit(&mut body, Literal::Int(1, Some(IntTy::I32)));
    let fn_body = binary(&mut body, BinOp::Add, x, one);

    let call_arg = alloc_lit(&mut body, Literal::Int(5, Some(IntTy::I32)));
    let add_one_fn = path_expr(&mut body, "add_one");
    let call = body.alloc_expr(
        Expr::Call {
            func: add_one_fn,
            args: vec![call_arg],
        },
        dummy_span(),
    );

    let mut ev = ConstEvaluator::new(&body)
        .with_interner(interner())
        .with_const_fn(
            name("add_one"),
            BodyFn {
                params: vec![x_pat],
                body: fn_body,
            },
        );
    let result = ev.evaluate(call).expect("const fn call should succeed");
    assert_eq!(result, ConstValue::Int(6, IntTy::I32));
}

#[test]
fn user_const_fn_result_used_in_binary() {
    // const fn square(x) { x * x }; square(3) + 1 == 10
    let mut body = test_body();
    let x_pat = binding_pat(&mut body, "x");
    let x1 = path_expr(&mut body, "x");
    let x2 = path_expr(&mut body, "x");
    let fn_body = binary(&mut body, BinOp::Mul, x1, x2);

    let call_arg = alloc_lit(&mut body, Literal::Int(3, Some(IntTy::I32)));
    let square_fn = path_expr(&mut body, "square");
    let call = body.alloc_expr(
        Expr::Call {
            func: square_fn,
            args: vec![call_arg],
        },
        dummy_span(),
    );
    let one = alloc_lit(&mut body, Literal::Int(1, Some(IntTy::I32)));
    let sum = binary(&mut body, BinOp::Add, call, one);

    let mut ev = ConstEvaluator::new(&body)
        .with_interner(interner())
        .with_const_fn(
            name("square"),
            BodyFn {
                params: vec![x_pat],
                body: fn_body,
            },
        );
    let result = ev.evaluate(sum).expect("const fn call should succeed");
    assert_eq!(result, ConstValue::Int(10, IntTy::I32));
}

#[test]
fn unknown_user_const_fn_errors() {
    let mut body = test_body();
    let call_arg = alloc_lit(&mut body, Literal::Int(1, Some(IntTy::I32)));
    let nonexistent_fn = path_expr(&mut body, "nonexistent");
    let call = body.alloc_expr(
        Expr::Call {
            func: nonexistent_fn,
            args: vec![call_arg],
        },
        dummy_span(),
    );
    let err = eval_err(&body, call);
    assert!(
        err.message.contains("unknown const fn"),
        "expected unknown-const-fn error, got: {}",
        err.message
    );
}
