//! Tests for literal constant evaluation (W1-C03-T01 related).

use crate::{ConstEvalError, ConstEvaluator, ConstValue};
use glyim_core::arena::IndexVec;
use glyim_core::def_id::LocalDefId;
use glyim_core::interner::Interner;
use glyim_core::primitives::{FloatTy, IntTy, UintTy};
use glyim_hir::{Body, Expr, ExprId, Literal, Path};
use glyim_span::{ByteIdx, FileId, Span};

/// Shared interner for tests that build path-named const fns.
fn interner() -> &'static Interner {
    static I: std::sync::OnceLock<Interner> = std::sync::OnceLock::new();
    I.get_or_init(Interner::new)
}

/// Helper to create a dummy span.
fn dummy_span() -> Span {
    Span::new(
        FileId::BOGUS,
        ByteIdx::ZERO,
        ByteIdx::ZERO,
        glyim_span::SyntaxContext::ROOT,
    )
}

/// Helper to create a fresh Body for testing.
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

/// Helper: allocate a literal expression into the body, returning its ExprId.
fn alloc_lit(body: &mut Body, lit: Literal) -> ExprId {
    body.alloc_expr(Expr::Literal(lit), dummy_span())
}

/// Helper: allocate a binary expression into the body.
fn alloc_binary(
    body: &mut Body,
    op: glyim_core::primitives::BinOp,
    lhs: ExprId,
    rhs: ExprId,
) -> ExprId {
    body.alloc_expr(Expr::Binary { op, lhs, rhs }, dummy_span())
}

/// Helper: evaluate an expression and unwrap the result.
fn eval_ok(body: &Body, expr_id: ExprId) -> ConstValue {
    let mut evaluator = ConstEvaluator::new(body);
    evaluator
        .evaluate(expr_id)
        .expect("const evaluation should succeed")
}

/// Helper: evaluate an expression expecting an error.
fn eval_err(body: &Body, expr_id: ExprId) -> ConstEvalError {
    let mut evaluator = ConstEvaluator::new(body);
    evaluator
        .evaluate(expr_id)
        .expect_err("const evaluation should fail")
}

// ---- T01: const { 1 + 2 } evaluates to 3 ----

#[test]
fn t01_add_integers_evaluates_to_sum() {
    let mut body = test_body();
    let one = alloc_lit(&mut body, Literal::Int(1, Some(IntTy::I32)));
    let two = alloc_lit(&mut body, Literal::Int(2, Some(IntTy::I32)));
    let add = alloc_binary(&mut body, glyim_core::primitives::BinOp::Add, one, two);

    let result = eval_ok(&body, add);
    assert_eq!(result, ConstValue::Int(3, IntTy::I32));
}

#[test]
fn t01_add_untyped_integers_defaults_to_i32() {
    let mut body = test_body();
    let one = alloc_lit(&mut body, Literal::Int(1, None));
    let two = alloc_lit(&mut body, Literal::Int(2, None));
    let add = alloc_binary(&mut body, glyim_core::primitives::BinOp::Add, one, two);

    let result = eval_ok(&body, add);
    assert_eq!(result, ConstValue::Int(3, IntTy::I32));
}

#[test]
fn t01_subtract_integers() {
    let mut body = test_body();
    let ten = alloc_lit(&mut body, Literal::Int(10, Some(IntTy::I32)));
    let three = alloc_lit(&mut body, Literal::Int(3, Some(IntTy::I32)));
    let sub = alloc_binary(&mut body, glyim_core::primitives::BinOp::Sub, ten, three);

    let result = eval_ok(&body, sub);
    assert_eq!(result, ConstValue::Int(7, IntTy::I32));
}

#[test]
fn t01_multiply_integers() {
    let mut body = test_body();
    let four = alloc_lit(&mut body, Literal::Int(4, Some(IntTy::I32)));
    let five = alloc_lit(&mut body, Literal::Int(5, Some(IntTy::I32)));
    let mul = alloc_binary(&mut body, glyim_core::primitives::BinOp::Mul, four, five);

    let result = eval_ok(&body, mul);
    assert_eq!(result, ConstValue::Int(20, IntTy::I32));
}

#[test]
fn t01_divide_integers() {
    let mut body = test_body();
    let ten = alloc_lit(&mut body, Literal::Int(10, Some(IntTy::I32)));
    let two = alloc_lit(&mut body, Literal::Int(2, Some(IntTy::I32)));
    let div = alloc_binary(&mut body, glyim_core::primitives::BinOp::Div, ten, two);

    let result = eval_ok(&body, div);
    assert_eq!(result, ConstValue::Int(5, IntTy::I32));
}

#[test]
fn t01_remainder_integers() {
    let mut body = test_body();
    let ten = alloc_lit(&mut body, Literal::Int(10, Some(IntTy::I32)));
    let three = alloc_lit(&mut body, Literal::Int(3, Some(IntTy::I32)));
    let rem = alloc_binary(&mut body, glyim_core::primitives::BinOp::Rem, ten, three);

    let result = eval_ok(&body, rem);
    assert_eq!(result, ConstValue::Int(1, IntTy::I32));
}

#[test]
fn t01_division_by_zero_is_error() {
    let mut body = test_body();
    let ten = alloc_lit(&mut body, Literal::Int(10, Some(IntTy::I32)));
    let zero = alloc_lit(&mut body, Literal::Int(0, Some(IntTy::I32)));
    let div = alloc_binary(&mut body, glyim_core::primitives::BinOp::Div, ten, zero);

    let err = eval_err(&body, div);
    assert!(
        err.message.contains("zero"),
        "Expected zero-related error, got: {}",
        err.message
    );
}

#[test]
fn t01_remainder_by_zero_is_error() {
    let mut body = test_body();
    let ten = alloc_lit(&mut body, Literal::Int(10, Some(IntTy::I32)));
    let zero = alloc_lit(&mut body, Literal::Int(0, Some(IntTy::I32)));
    let rem = alloc_binary(&mut body, glyim_core::primitives::BinOp::Rem, ten, zero);

    let err = eval_err(&body, rem);
    assert!(
        err.message.contains("zero"),
        "Expected zero-related error, got: {}",
        err.message
    );
}

#[test]
fn t01_uint_addition() {
    let mut body = test_body();
    let a = alloc_lit(&mut body, Literal::Uint(100, Some(UintTy::U32)));
    let b = alloc_lit(&mut body, Literal::Uint(200, Some(UintTy::U32)));
    let add = alloc_binary(&mut body, glyim_core::primitives::BinOp::Add, a, b);

    let result = eval_ok(&body, add);
    assert_eq!(result, ConstValue::Uint(300, UintTy::U32));
}

#[test]
fn t01_untyped_uint_defaults_to_u32() {
    let mut body = test_body();
    let a = alloc_lit(&mut body, Literal::Uint(42, None));
    let result = eval_ok(&body, a);
    assert_eq!(result, ConstValue::Uint(42, UintTy::U32));
}

#[test]
fn t01_mismatched_int_types_cannot_add() {
    let mut body = test_body();
    let i32_val = alloc_lit(&mut body, Literal::Int(1, Some(IntTy::I32)));
    let i64_val = alloc_lit(&mut body, Literal::Int(2, Some(IntTy::I64)));
    let add = alloc_binary(
        &mut body,
        glyim_core::primitives::BinOp::Add,
        i32_val,
        i64_val,
    );

    let err = eval_err(&body, add);
    assert!(
        err.message.contains("incompatible") || err.message.contains("overflow"),
        "Expected type mismatch error, got: {}",
        err.message
    );
}

// ---- Literal evaluation ----

#[test]
fn literal_bool_true() {
    let mut body = test_body();
    let lit = alloc_lit(&mut body, Literal::Bool(true));
    let result = eval_ok(&body, lit);
    assert_eq!(result, ConstValue::Bool(true));
}

#[test]
fn literal_bool_false() {
    let mut body = test_body();
    let lit = alloc_lit(&mut body, Literal::Bool(false));
    let result = eval_ok(&body, lit);
    assert_eq!(result, ConstValue::Bool(false));
}

#[test]
fn literal_char() {
    let mut body = test_body();
    let lit = alloc_lit(&mut body, Literal::Char('A'));
    let result = eval_ok(&body, lit);
    assert_eq!(result, ConstValue::Char('A'));
}

#[test]
fn literal_unit() {
    let mut body = test_body();
    let lit = alloc_lit(&mut body, Literal::Unit);
    let result = eval_ok(&body, lit);
    assert_eq!(result, ConstValue::Unit);
}

#[test]
fn literal_i64() {
    let mut body = test_body();
    let lit = alloc_lit(&mut body, Literal::Int(1_000_000_000_000, Some(IntTy::I64)));
    let result = eval_ok(&body, lit);
    assert_eq!(result, ConstValue::Int(1_000_000_000_000, IntTy::I64));
}

#[test]
fn literal_u8() {
    let mut body = test_body();
    let lit = alloc_lit(&mut body, Literal::Uint(255, Some(UintTy::U8)));
    let result = eval_ok(&body, lit);
    assert_eq!(result, ConstValue::Uint(255, UintTy::U8));
}

#[test]
fn literal_float() {
    let mut body = test_body();
    let bits = std::f64::consts::PI.to_bits();
    let lit = alloc_lit(&mut body, Literal::Float(bits, FloatTy::F64));
    let result = eval_ok(&body, lit);
    assert_eq!(result, ConstValue::FloatBits(bits, FloatTy::F64));
}

// ---- Unary operations ----

#[test]
fn unary_negate_int() {
    let mut body = test_body();
    let five = alloc_lit(&mut body, Literal::Int(5, Some(IntTy::I32)));
    let neg = body.alloc_expr(
        Expr::Unary {
            op: glyim_core::primitives::UnOp::Neg,
            expr: five,
        },
        dummy_span(),
    );

    let result = eval_ok(&body, neg);
    assert_eq!(result, ConstValue::Int(-5, IntTy::I32));
}

#[test]
fn unary_not_bool() {
    let mut body = test_body();
    let val = alloc_lit(&mut body, Literal::Bool(true));
    let not = body.alloc_expr(
        Expr::Unary {
            op: glyim_core::primitives::UnOp::Not,
            expr: val,
        },
        dummy_span(),
    );

    let result = eval_ok(&body, not);
    assert_eq!(result, ConstValue::Bool(false));
}

#[test]
fn unary_not_int() {
    let mut body = test_body();
    let val = alloc_lit(&mut body, Literal::Int(0, Some(IntTy::I32)));
    let not = body.alloc_expr(
        Expr::Unary {
            op: glyim_core::primitives::UnOp::Not,
            expr: val,
        },
        dummy_span(),
    );

    let result = eval_ok(&body, not);
    assert_eq!(result, ConstValue::Int(!0i128, IntTy::I32));
}

// ---- Comparison operations ----

#[test]
fn compare_eq_same() {
    let mut body = test_body();
    let a = alloc_lit(&mut body, Literal::Int(5, Some(IntTy::I32)));
    let b = alloc_lit(&mut body, Literal::Int(5, Some(IntTy::I32)));
    let cmp = alloc_binary(&mut body, glyim_core::primitives::BinOp::Eq, a, b);

    let result = eval_ok(&body, cmp);
    assert_eq!(result, ConstValue::Bool(true));
}

#[test]
fn compare_eq_different() {
    let mut body = test_body();
    let a = alloc_lit(&mut body, Literal::Int(5, Some(IntTy::I32)));
    let b = alloc_lit(&mut body, Literal::Int(6, Some(IntTy::I32)));
    let cmp = alloc_binary(&mut body, glyim_core::primitives::BinOp::Eq, a, b);

    let result = eval_ok(&body, cmp);
    assert_eq!(result, ConstValue::Bool(false));
}

#[test]
fn compare_lt_true() {
    let mut body = test_body();
    let a = alloc_lit(&mut body, Literal::Int(3, Some(IntTy::I32)));
    let b = alloc_lit(&mut body, Literal::Int(5, Some(IntTy::I32)));
    let cmp = alloc_binary(&mut body, glyim_core::primitives::BinOp::Lt, a, b);

    let result = eval_ok(&body, cmp);
    assert_eq!(result, ConstValue::Bool(true));
}

#[test]
fn compare_lt_false() {
    let mut body = test_body();
    let a = alloc_lit(&mut body, Literal::Int(5, Some(IntTy::I32)));
    let b = alloc_lit(&mut body, Literal::Int(3, Some(IntTy::I32)));
    let cmp = alloc_binary(&mut body, glyim_core::primitives::BinOp::Lt, a, b);

    let result = eval_ok(&body, cmp);
    assert_eq!(result, ConstValue::Bool(false));
}

#[test]
fn compare_gt() {
    let mut body = test_body();
    let a = alloc_lit(&mut body, Literal::Int(5, Some(IntTy::I32)));
    let b = alloc_lit(&mut body, Literal::Int(3, Some(IntTy::I32)));
    let cmp = alloc_binary(&mut body, glyim_core::primitives::BinOp::Gt, a, b);

    let result = eval_ok(&body, cmp);
    assert_eq!(result, ConstValue::Bool(true));
}

// ---- Logical operations ----

#[test]
fn logical_and_true_true() {
    let mut body = test_body();
    let a = alloc_lit(&mut body, Literal::Bool(true));
    let b = alloc_lit(&mut body, Literal::Bool(true));
    let and = alloc_binary(&mut body, glyim_core::primitives::BinOp::And, a, b);

    let result = eval_ok(&body, and);
    assert_eq!(result, ConstValue::Bool(true));
}

#[test]
fn logical_and_true_false() {
    let mut body = test_body();
    let a = alloc_lit(&mut body, Literal::Bool(true));
    let b = alloc_lit(&mut body, Literal::Bool(false));
    let and = alloc_binary(&mut body, glyim_core::primitives::BinOp::And, a, b);

    let result = eval_ok(&body, and);
    assert_eq!(result, ConstValue::Bool(false));
}

#[test]
fn logical_or_false_true() {
    let mut body = test_body();
    let a = alloc_lit(&mut body, Literal::Bool(false));
    let b = alloc_lit(&mut body, Literal::Bool(true));
    let or = alloc_binary(&mut body, glyim_core::primitives::BinOp::Or, a, b);

    let result = eval_ok(&body, or);
    assert_eq!(result, ConstValue::Bool(true));
}

#[test]
fn logical_or_false_false() {
    let mut body = test_body();
    let a = alloc_lit(&mut body, Literal::Bool(false));
    let b = alloc_lit(&mut body, Literal::Bool(false));
    let or = alloc_binary(&mut body, glyim_core::primitives::BinOp::Or, a, b);

    let result = eval_ok(&body, or);
    assert_eq!(result, ConstValue::Bool(false));
}

// ---- Bitwise operations ----

#[test]
fn bitwise_and_int() {
    let mut body = test_body();
    let a = alloc_lit(&mut body, Literal::Int(0b1100, Some(IntTy::I32)));
    let b = alloc_lit(&mut body, Literal::Int(0b1010, Some(IntTy::I32)));
    let and = alloc_binary(&mut body, glyim_core::primitives::BinOp::BitAnd, a, b);

    let result = eval_ok(&body, and);
    assert_eq!(result, ConstValue::Int(0b1000, IntTy::I32));
}

#[test]
fn bitwise_or_int() {
    let mut body = test_body();
    let a = alloc_lit(&mut body, Literal::Int(0b1100, Some(IntTy::I32)));
    let b = alloc_lit(&mut body, Literal::Int(0b1010, Some(IntTy::I32)));
    let or = alloc_binary(&mut body, glyim_core::primitives::BinOp::BitOr, a, b);

    let result = eval_ok(&body, or);
    assert_eq!(result, ConstValue::Int(0b1110, IntTy::I32));
}

#[test]
fn bitwise_xor_int() {
    let mut body = test_body();
    let a = alloc_lit(&mut body, Literal::Int(0b1100, Some(IntTy::I32)));
    let b = alloc_lit(&mut body, Literal::Int(0b1010, Some(IntTy::I32)));
    let xor = alloc_binary(&mut body, glyim_core::primitives::BinOp::BitXor, a, b);

    let result = eval_ok(&body, xor);
    assert_eq!(result, ConstValue::Int(0b0110, IntTy::I32));
}

#[test]
fn shift_left() {
    let mut body = test_body();
    let a = alloc_lit(&mut body, Literal::Int(1, Some(IntTy::I32)));
    let b = alloc_lit(&mut body, Literal::Int(4, Some(IntTy::I32)));
    let shl = alloc_binary(&mut body, glyim_core::primitives::BinOp::Shl, a, b);

    let result = eval_ok(&body, shl);
    assert_eq!(result, ConstValue::Int(16, IntTy::I32));
}

#[test]
fn shift_right() {
    let mut body = test_body();
    let a = alloc_lit(&mut body, Literal::Int(16, Some(IntTy::I32)));
    let b = alloc_lit(&mut body, Literal::Int(2, Some(IntTy::I32)));
    let shr = alloc_binary(&mut body, glyim_core::primitives::BinOp::Shr, a, b);

    let result = eval_ok(&body, shr);
    assert_eq!(result, ConstValue::Int(4, IntTy::I32));
}

// ---- Nested expressions ----

#[test]
fn nested_arithmetic() {
    let mut body = test_body();
    let one = alloc_lit(&mut body, Literal::Int(1, Some(IntTy::I32)));
    let two = alloc_lit(&mut body, Literal::Int(2, Some(IntTy::I32)));
    let three = alloc_lit(&mut body, Literal::Int(3, Some(IntTy::I32)));
    let add = alloc_binary(&mut body, glyim_core::primitives::BinOp::Add, one, two);
    let mul = alloc_binary(&mut body, glyim_core::primitives::BinOp::Mul, add, three);

    let result = eval_ok(&body, mul);
    assert_eq!(result, ConstValue::Int(9, IntTy::I32));
}

// ---- Unsupported expressions ----

/// Calls to an unknown/unregistered const fn are rejected (not silently
/// dropped). This replaced the old "calls not supported" assertion once
/// `Expr::Call` const-evaluation (plan §4.2) was implemented — a *known*
/// builtin now evaluates, so the error path is reserved for unknown names.
#[test]
fn unknown_const_fn_is_error() {
    let mut body = test_body();
    let one = alloc_lit(&mut body, Literal::Int(1, Some(IntTy::I32)));
    let func = body.alloc_expr(
        Expr::Path(Path::from_single(interner().intern("nonexistent_fn"))),
        dummy_span(),
    );
    let call = body.alloc_expr(
        Expr::Call {
            func,
            args: vec![one],
        },
        dummy_span(),
    );

    let err = ConstEvaluator::new(&body)
        .with_interner(interner())
        .evaluate(call)
        .expect_err("const evaluation should fail");
    assert!(
        err.message.contains("unknown const fn"),
        "Expected unknown-const-fn error, got: {}",
        err.message
    );
}

#[test]
fn missing_expression_is_error() {
    let mut body = test_body();
    let missing = body.alloc_expr(Expr::Missing, dummy_span());

    let err = eval_err(&body, missing);
    assert!(
        err.message.contains("missing"),
        "Expected missing error, got: {}",
        err.message
    );
}
