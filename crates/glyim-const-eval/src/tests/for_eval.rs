//! Tests for `Expr::For` constant evaluation over const-evaluable iterables
//! (plan §4.4). The runtime desugaring (`IntoIterator::into_iter` + `.next()`)
//! needs `Expr::Call`/`Expr::MethodCall`, which the const evaluator does not
//! yet implement; this covers the compile-time cases that don't need it:
//! `for x in <range | array | tuple> { .. }`.

use crate::{ConstEvaluator, ConstValue};
use glyim_core::interner::{Interner, Name};
use glyim_core::primitives::{BinOp, IntTy, Mutability};
use glyim_hir::{Body, Expr, ExprId, Literal, Pat, Path};
use std::sync::OnceLock;

/// A single shared interner for the whole test module. `Interner::new()`
/// returns a *fresh, empty* interner every call, so interning two different
/// names on two different instances would both yield `Spur(0)` and collide.
/// All name/path creation below must go through this one interner so that
/// distinct identifiers remain distinct.
fn interner() -> &'static Interner {
    static I: OnceLock<Interner> = OnceLock::new();
    I.get_or_init(Interner::new)
}

fn dummy_span() -> glyim_span::Span {
    glyim_span::Span::new(
        glyim_span::FileId::BOGUS,
        glyim_span::ByteIdx::ZERO,
        glyim_span::ByteIdx::ZERO,
        glyim_span::SyntaxContext::ROOT,
    )
}

fn test_body() -> Body {
    Body {
        owner: glyim_core::def_id::LocalDefId::from_raw(0),
        exprs: glyim_core::arena::IndexVec::new(),
        pats: glyim_core::arena::IndexVec::new(),
        params: Vec::new(),
        span: dummy_span(),
        expr_spans: glyim_core::arena::IndexVec::new(),
    }
}

fn alloc_lit(body: &mut Body, lit: Literal) -> ExprId {
    body.alloc_expr(Expr::Literal(lit), dummy_span())
}

/// Bind a bare name to a path expression that can be assigned/looked up.
fn path_expr(body: &mut Body, s: &str) -> ExprId {
    let n = interner().intern(s);
    body.alloc_expr(Expr::Path(Path::from_single(n)), dummy_span())
}

fn name(s: &str) -> Name {
    interner().intern(s)
}

/// `acc = 0` — initialize the accumulator at the enclosing scope.
fn acc_init(body: &mut Body) -> ExprId {
    let lhs = path_expr(body, "acc");
    let rhs = alloc_lit(body, Literal::Int(0, Some(IntTy::I32)));
    body.alloc_expr(Expr::Assign { lhs, rhs }, dummy_span())
}

/// `acc = acc + x` — accumulate the loop variable into `acc`.
fn acc_add_x(body: &mut Body) -> ExprId {
    let lhs = path_expr(body, "acc");
    let acc_rhs = path_expr(body, "acc");
    let x = path_expr(body, "x");
    let sum = body.alloc_expr(
        Expr::Binary {
            op: BinOp::Add,
            lhs: acc_rhs,
            rhs: x,
        },
        dummy_span(),
    );
    body.alloc_expr(Expr::Assign { lhs, rhs: sum }, dummy_span())
}

/// `acc` — read the accumulator.
fn acc_read(body: &mut Body) -> ExprId {
    path_expr(body, "acc")
}

/// Build `for <pat> in <iterable> { <body> }`.
fn build_for(body: &mut Body, pat: Pat, iterable: ExprId, loop_body: ExprId) -> ExprId {
    let pat_id = body.pats.push(pat);
    body.alloc_expr(
        Expr::For {
            pat: pat_id,
            iterable,
            body: loop_body,
        },
        dummy_span(),
    )
}

fn binding(p: &str) -> Pat {
    Pat::Binding {
        name: name(p),
        mutability: Mutability::Not,
        subpattern: None,
    }
}

fn block(body: &mut Body, stmts: Vec<ExprId>, tail: Option<ExprId>) -> ExprId {
    body.alloc_expr(Expr::Block { stmts, tail }, dummy_span())
}

fn range(body: &mut Body, lo: i128, hi: i128, inclusive: bool) -> ExprId {
    let start = alloc_lit(body, Literal::Int(lo, Some(IntTy::I32)));
    let end = alloc_lit(body, Literal::Int(hi, Some(IntTy::I32)));
    body.alloc_expr(
        Expr::Range {
            start: Some(start),
            end: Some(end),
            inclusive,
        },
        dummy_span(),
    )
}

fn eval_ok(body: &Body, expr_id: ExprId) -> ConstValue {
    let mut evaluator = ConstEvaluator::new(body);
    evaluator
        .evaluate(expr_id)
        .expect("const evaluation should succeed")
}

fn eval_err(body: &Body, expr_id: ExprId) -> crate::ConstEvalError {
    let mut evaluator = ConstEvaluator::new(body);
    evaluator
        .evaluate(expr_id)
        .expect_err("const evaluation should fail")
}

/// `acc = 0; for x in 0..N { acc = acc + x }; acc` — sum of 0..N.
fn sum_range_program(lo: i128, hi: i128, inclusive: bool) -> (Body, ExprId) {
    let mut body = test_body();
    let decl = acc_init(&mut body);
    let r = range(&mut body, lo, hi, inclusive);
    let loop_body = acc_add_x(&mut body);
    let for_expr = build_for(&mut body, binding("x"), r, loop_body);
    let read = acc_read(&mut body);
    let program = block(&mut body, vec![decl, for_expr], Some(read));
    (body, program)
}

#[test]
fn for_over_exclusive_range_sums_elements() {
    // sum of 0..5 = 0+1+2+3+4 = 10
    let (body, program) = sum_range_program(0, 5, false);
    assert_eq!(eval_ok(&body, program), ConstValue::Int(10, IntTy::I32));
}

#[test]
fn for_over_inclusive_range_includes_end() {
    // sum of 0..=4 = 0+1+2+3+4 = 10 (same as 0..5, exercises inclusive flag)
    let (body, program) = sum_range_program(0, 4, true);
    assert_eq!(eval_ok(&body, program), ConstValue::Int(10, IntTy::I32));
}

#[test]
fn for_over_empty_range_iterates_zero_times() {
    // sum of 0..0 = 0
    let (body, program) = sum_range_program(0, 0, false);
    assert_eq!(eval_ok(&body, program), ConstValue::Int(0, IntTy::I32));
}

#[test]
fn for_over_array_sums_elements() {
    let mut body = test_body();
    let decl = acc_init(&mut body);
    let e10 = alloc_lit(&mut body, Literal::Int(10, Some(IntTy::I32)));
    let e20 = alloc_lit(&mut body, Literal::Int(20, Some(IntTy::I32)));
    let e30 = alloc_lit(&mut body, Literal::Int(30, Some(IntTy::I32)));
    let array = body.alloc_expr(Expr::Array(vec![e10, e20, e30]), dummy_span());
    let loop_body = acc_add_x(&mut body);
    let for_expr = build_for(&mut body, binding("x"), array, loop_body);
    let read = acc_read(&mut body);
    let program = block(&mut body, vec![decl, for_expr], Some(read));
    assert_eq!(eval_ok(&body, program), ConstValue::Int(60, IntTy::I32));
}

#[test]
fn for_over_tuple_iterates_each_field() {
    let mut body = test_body();
    let decl = acc_init(&mut body);
    let e1 = alloc_lit(&mut body, Literal::Int(1, Some(IntTy::I32)));
    let e2 = alloc_lit(&mut body, Literal::Int(2, Some(IntTy::I32)));
    let e3 = alloc_lit(&mut body, Literal::Int(3, Some(IntTy::I32)));
    let tup = body.alloc_expr(Expr::Tuple(vec![e1, e2, e3]), dummy_span());
    let loop_body = acc_add_x(&mut body);
    let for_expr = build_for(&mut body, binding("x"), tup, loop_body);
    let read = acc_read(&mut body);
    let program = block(&mut body, vec![decl, for_expr], Some(read));
    assert_eq!(eval_ok(&body, program), ConstValue::Int(6, IntTy::I32));
}

#[test]
fn for_with_wild_pattern_ignores_element() {
    // for _ in 0..3 { } should simply run three times and yield Unit.
    let mut body = test_body();
    let range = range(&mut body, 0, 3, false);
    let empty = block(&mut body, vec![], None);
    let for_expr = build_for(&mut body, Pat::Wild, range, empty);
    assert_eq!(eval_ok(&body, for_expr), ConstValue::Unit);
}

#[test]
fn for_with_break_exits_loop() {
    // acc = 0; for x in 0..10 { if x == 3 { break; } acc = acc + x }; acc
    //
    // Rust semantics: `break` only takes effect once the enclosing block
    // finishes, so on the iteration where `x == 3` the `break` is armed but
    // `acc = acc + x` (== 3) still runs before the loop driver sees the break.
    // Hence the result is 0 + 1 + 2 + 3 = 6, NOT 45 (which it would be without
    // the break). This proves the loop terminates early on `break`.
    let mut body = test_body();
    let decl = acc_init(&mut body);
    // Condition: x == 3
    let x_path = path_expr(&mut body, "x");
    let three = alloc_lit(&mut body, Literal::Int(3, Some(IntTy::I32)));
    let cond = body.alloc_expr(
        Expr::Binary {
            op: BinOp::Eq,
            lhs: x_path,
            rhs: three,
        },
        dummy_span(),
    );
    let brk = body.alloc_expr(Expr::Break { value: None }, dummy_span());
    let if_break = body.alloc_expr(
        Expr::If {
            cond,
            then_branch: brk,
            else_branch: None,
        },
        dummy_span(),
    );
    let acc_update = acc_add_x(&mut body);
    let loop_body = block(&mut body, vec![if_break, acc_update], None);
    let range = range(&mut body, 0, 10, false);
    let for_expr = build_for(&mut body, binding("x"), range, loop_body);
    let read = acc_read(&mut body);
    let program = block(&mut body, vec![decl, for_expr], Some(read));
    assert_eq!(eval_ok(&body, program), ConstValue::Int(6, IntTy::I32));
}

#[test]
fn for_over_struct_iterable_is_rejected_uniformly() {
    // Plan §13.3: the type-kind special-casing in `eval_for` was deleted in
    // favor of a single uniform `materialize_elements` helper. A value that is
    // not a collection-like iterable (here a struct) must be rejected with a
    // clear error through that uniform path, not silently mis-handled.
    let mut body = test_body();
    let s_name = name("S");
    let struct_val = body.alloc_expr(
        Expr::Struct {
            path: glyim_hir::Path::from_single(s_name),
            fields: vec![],
            spread: None,
        },
        dummy_span(),
    );
    let empty = block(&mut body, vec![], None);
    let for_expr = build_for(&mut body, Pat::Wild, struct_val, empty);
    let err = eval_err(&body, for_expr);
    assert!(
        err.message.contains("iterable kind"),
        "expected iterable-kind error, got: {}",
        err.message
    );
}

#[test]
fn for_over_open_ended_range_is_error() {
    // `for x in ..5 { }` — open-ended range has no concrete start bound.
    let mut body = test_body();
    let end = alloc_lit(&mut body, Literal::Int(5, Some(IntTy::I32)));
    let range = body.alloc_expr(
        Expr::Range {
            start: None,
            end: Some(end),
            inclusive: false,
        },
        dummy_span(),
    );
    let empty = block(&mut body, vec![], None);
    let for_expr = build_for(&mut body, Pat::Wild, range, empty);
    let err = eval_err(&body, for_expr);
    assert!(
        err.message.contains("concrete"),
        "expected concrete-bounds error, got: {}",
        err.message
    );
}
