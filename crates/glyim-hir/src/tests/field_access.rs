//! Tests for field access lowering (tuple and struct).
//!
//! W3-C03-T01: Tuple field access `tuple.0` produces correct HIR
//! W3-C03-T02: Struct field access uses resolved field name

use crate::lower::lower_crate;
use crate::{BodyId, Expr, ExprId, ItemId, ItemKind};
use glyim_core::interner::Interner;
use glyim_frontend::parse_to_syntax;
use glyim_span::FileId;

fn get_body_hir(source: &str) -> (crate::CrateHir, Interner, BodyId) {
    let file_id = FileId::from_raw(0);
    let parse_result = parse_to_syntax(source, file_id);
    let mut interner = Interner::new();
    let hir = lower_crate(&parse_result.root, &mut interner, &mut Vec::new());
    let body_id = match &hir.items[ItemId::from_raw(0)].kind {
        ItemKind::Fn(fn_item) => fn_item.body.expect("no body"),
        other => panic!("expected Fn item, got {other:?}"),
    };
    (hir, interner, body_id)
}

fn get_body(hir: &crate::CrateHir, body_id: BodyId) -> &crate::Body {
    &hir.bodies[body_id]
}

/// Walk the expression tree to find the first `Expr::Field`.
fn find_field_expr(
    body: &crate::Body,
    start: ExprId,
) -> Option<(ExprId, ExprId, glyim_core::interner::Name)> {
    let mut stack = vec![start];
    while let Some(id) = stack.pop() {
        match &body.exprs[id] {
            Expr::Field { receiver, field } => {
                return Some((id, *receiver, *field));
            }
            Expr::Block { stmts, tail } => {
                for s in stmts {
                    stack.push(*s);
                }
                if let Some(t) = tail {
                    stack.push(*t);
                }
            }
            Expr::Binary { lhs, rhs, .. } => {
                stack.push(*lhs);
                stack.push(*rhs);
            }
            Expr::Assign { lhs, rhs } => {
                stack.push(*lhs);
                stack.push(*rhs);
            }
            Expr::Call { func, args } => {
                stack.push(*func);
                for a in args {
                    stack.push(*a);
                }
            }
            Expr::MethodCall { receiver, args, .. } => {
                stack.push(*receiver);
                for a in args {
                    stack.push(*a);
                }
            }
            Expr::Tuple(elems) => {
                for e in elems {
                    stack.push(*e);
                }
            }
            Expr::Ref { expr, .. } => {
                stack.push(*expr);
            }
            Expr::Unary { expr, .. } => {
                stack.push(*expr);
            }
            Expr::Index { base, index } => {
                stack.push(*base);
                stack.push(*index);
            }
            Expr::Cast { expr, .. } => {
                stack.push(*expr);
            }
            _ => {}
        }
    }
    None
}

#[test]
fn test_tuple_field_access_zero() {
    // W3-C03-T01: Tuple field access `tuple.0` produces correct HIR
    let source = r"fn f() { (1, true).0 }";
    let (hir, interner, body_id) = get_body_hir(source);
    let body = get_body(&hir, body_id);

    let last_id =
        ExprId::from_raw(body.exprs.len().checked_sub(1).expect("body has no exprs") as u32);
    let result = find_field_expr(body, last_id);
    assert!(
        result.is_some(),
        "Expected to find a Field expression for tuple.0"
    );
    let (_, receiver_id, field_name) = result.unwrap();

    assert_eq!(
        interner.resolve(field_name),
        "0",
        "Tuple field access .0 should have field name '0'"
    );

    match &body.exprs[receiver_id] {
        Expr::Tuple(elems) => {
            assert_eq!(elems.len(), 2, "Tuple should have 2 elements");
        }
        other => panic!("Expected Tuple expression as receiver, got {other:?}"),
    }
}

#[test]
fn test_tuple_field_access_index_one() {
    let source = r"fn f() { (1, true, 3).1 }";
    let (hir, interner, body_id) = get_body_hir(source);
    let body = get_body(&hir, body_id);

    let last_id =
        ExprId::from_raw(body.exprs.len().checked_sub(1).expect("body has no exprs") as u32);
    let result = find_field_expr(body, last_id);
    assert!(
        result.is_some(),
        "Expected to find a Field expression for tuple.1"
    );
    let (_, _, field_name) = result.unwrap();
    assert_eq!(
        interner.resolve(field_name),
        "1",
        "Tuple field access .1 should have field name '1'"
    );
}

#[test]
fn test_struct_field_access() {
    // W3-C03-T02: Struct field access uses resolved field name
    let source = r"fn f() { p.x }";
    let (hir, interner, body_id) = get_body_hir(source);
    let body = get_body(&hir, body_id);

    let last_id =
        ExprId::from_raw(body.exprs.len().checked_sub(1).expect("body has no exprs") as u32);
    let result = find_field_expr(body, last_id);
    assert!(
        result.is_some(),
        "Expected to find a Field expression for p.x"
    );
    let (_, _, field_name) = result.unwrap();
    assert_eq!(
        interner.resolve(field_name),
        "x",
        "Struct field access should have correct field name"
    );
}

#[test]
fn test_tuple_field_access_with_path_receiver() {
    // Named variable tuple access: t.0
    let source = r"fn f() { t.0 }";
    let (hir, interner, body_id) = get_body_hir(source);
    let body = get_body(&hir, body_id);

    let last_id =
        ExprId::from_raw(body.exprs.len().checked_sub(1).expect("body has no exprs") as u32);
    let result = find_field_expr(body, last_id);
    assert!(
        result.is_some(),
        "Expected to find a Field expression for t.0"
    );
    let (_, receiver_id, field_name) = result.unwrap();
    assert_eq!(
        interner.resolve(field_name),
        "0",
        "Tuple field access t.0 should have field name '0'"
    );

    match &body.exprs[receiver_id] {
        Expr::Path(path) => {
            assert_eq!(path.as_name(), Some(interner.intern("t")));
        }
        other => panic!("Expected Path expression as receiver, got {other:?}"),
    }
}
