use crate::lower::lower_crate;
use crate::{BodyId, Expr, ExprId, ItemId, ItemKind, Literal, Pat};
use glyim_core::interner::Interner;
use glyim_core::primitives::*;
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

fn last_expr_id(body: &crate::Body) -> ExprId {
    ExprId::from_raw(body.exprs.len() as u32 - 1)
}

fn get_body(hir: &crate::CrateHir, body_id: BodyId) -> &crate::Body {
    &hir.bodies[body_id]
}

#[test]
fn test_call_expr() {
    let (hir, _inter, body_id) = get_body_hir("fn f() { foo(1, 2) }");
    let body = get_body(&hir, body_id);
    let block_id = last_expr_id(body);
    match &body.exprs[block_id] {
        Expr::Block {
            tail: Some(call_id),
            ..
        } => match &body.exprs[*call_id] {
            Expr::Call { args, .. } => assert_eq!(args.len(), 2),
            _ => panic!(),
        },
        _ => panic!(),
    }
}

#[test]
fn test_unary_expr_not() {
    let (hir, _inter, body_id) = get_body_hir("fn f() { !true }");
    let body = get_body(&hir, body_id);
    let block_id = last_expr_id(body);
    match &body.exprs[block_id] {
        Expr::Block {
            tail: Some(unary_id),
            ..
        } => match &body.exprs[*unary_id] {
            Expr::Unary { op, .. } => assert_eq!(*op, UnOp::Not),
            _ => panic!(),
        },
        _ => panic!(),
    }
}

#[test]
fn test_unary_expr_neg() {
    let (hir, _inter, body_id) = get_body_hir("fn f() { -42 }");
    let body = get_body(&hir, body_id);
    let block_id = last_expr_id(body);
    match &body.exprs[block_id] {
        Expr::Block {
            tail: Some(unary_id),
            ..
        } => match &body.exprs[*unary_id] {
            Expr::Unary { op, .. } => assert_eq!(*op, UnOp::Neg),
            _ => panic!(),
        },
        _ => panic!(),
    }
}

#[test]
fn test_ref_expr() {
    let (hir, _inter, body_id) = get_body_hir("fn f() { &x }");
    let body = get_body(&hir, body_id);
    let block_id = last_expr_id(body);
    match &body.exprs[block_id] {
        Expr::Block {
            tail: Some(ref_id), ..
        } => match &body.exprs[*ref_id] {
            Expr::Ref { mutability, .. } => assert!(!mutability.is_mut()),
            _ => panic!(),
        },
        _ => panic!(),
    }
}

#[test]
fn test_match_expr() {
    let (hir, _inter, body_id) = get_body_hir("fn f(x: i32) { match x { 0 => 1, _ => 0 } }");
    let body = get_body(&hir, body_id);
    let block_id = last_expr_id(body);
    match &body.exprs[block_id] {
        Expr::Block {
            tail: Some(match_id),
            ..
        } => match &body.exprs[*match_id] {
            Expr::Match { arms, .. } => {
                assert_eq!(arms.len(), 2);
                if let Pat::Literal(Literal::Int(0, _)) = &body.pats[arms[0].pat] {
                } else {
                    panic!()
                }
                assert!(matches!(&body.pats[arms[1].pat], Pat::Wild));
            }
            _ => panic!(),
        },
        _ => panic!(),
    }
}

#[test]
fn test_while_expr() {
    let (hir, _inter, body_id) = get_body_hir("fn f() { while true { 1; } }");
    let body = get_body(&hir, body_id);
    let block_id = last_expr_id(body);
    match &body.exprs[block_id] {
        Expr::Block {
            tail: Some(while_id),
            ..
        } => {
            assert!(matches!(&body.exprs[*while_id], Expr::While { .. }));
        }
        _ => panic!(),
    }
}

#[test]
fn test_loop_expr() {
    let (hir, _inter, body_id) = get_body_hir("fn f() { loop { break; } }");
    let body = get_body(&hir, body_id);
    let block_id = last_expr_id(body);
    match &body.exprs[block_id] {
        Expr::Block {
            tail: Some(loop_id),
            ..
        } => {
            assert!(matches!(&body.exprs[*loop_id], Expr::Loop { .. }));
        }
        _ => panic!(),
    }
}

#[test]
fn test_for_expr() {
    let (hir, _inter, body_id) = get_body_hir("fn f() { for i in 0..10 { 1; } }");
    let body = get_body(&hir, body_id);
    let block_id = last_expr_id(body);
    match &body.exprs[block_id] {
        Expr::Block {
            tail: Some(for_id), ..
        } => {
            assert!(matches!(&body.exprs[*for_id], Expr::For { .. }));
        }
        _ => panic!(),
    }
}

#[test]
fn test_assign_expr() {
    let (hir, _inter, body_id) = get_body_hir("fn f() { x = 5; 0 }");
    let body = get_body(&hir, body_id);
    let block_id = last_expr_id(body);
    match &body.exprs[block_id] {
        Expr::Block { stmts, .. } => {
            assert_ne!(stmts.as_slice(), []);
            assert!(matches!(&body.exprs[stmts[0]], Expr::Assign { .. }));
        }
        _ => panic!(),
    }
}

#[test]
fn test_break_continue() {
    let (hir, _inter, body_id) = get_body_hir("fn f() { loop { break; continue; } }");
    let body = get_body(&hir, body_id);
    let block_id = last_expr_id(body);
    let while_id = match &body.exprs[block_id] {
        Expr::Block { tail: Some(id), .. } => *id,
        _ => panic!(),
    };
    let while_body_id = match &body.exprs[while_id] {
        Expr::Loop { body: b, .. } => b,
        _ => panic!(),
    };
    let (stmts, tail) = match &body.exprs[*while_body_id] {
        Expr::Block { stmts, tail } => (stmts.clone(), tail),
        _ => panic!(),
    };
    let mut saw_break = false;
    let mut saw_continue = false;
    for &sid in &stmts {
        match &body.exprs[sid] {
            Expr::Break { .. } => saw_break = true,
            Expr::Continue => saw_continue = true,
            _ => {}
        }
    }
    if let Some(tail_id) = tail {
        match &body.exprs[*tail_id] {
            Expr::Break { .. } => saw_break = true,
            Expr::Continue => saw_continue = true,
            _ => {}
        }
    }
    assert!(saw_break, "Expected Break expression");
    assert!(saw_continue, "Expected Continue expression");
}

#[test]
fn test_cast_expr() {
    let (hir, _inter, body_id) = get_body_hir("fn f() { x as i32 }");
    let body = get_body(&hir, body_id);
    let block_id = last_expr_id(body);
    match &body.exprs[block_id] {
        Expr::Block {
            tail: Some(cast_id),
            ..
        } => {
            assert!(matches!(&body.exprs[*cast_id], Expr::Cast { .. }));
        }
        _ => panic!(),
    }
}

#[test]
fn test_field_expr() {
    let (hir, _inter, body_id) = get_body_hir("fn f() { a.b }");
    let body = get_body(&hir, body_id);
    let block_id = last_expr_id(body);
    match &body.exprs[block_id] {
        Expr::Block {
            tail: Some(field_id),
            ..
        } => {
            assert!(matches!(&body.exprs[*field_id], Expr::Field { .. }));
        }
        _ => panic!(),
    }
}

#[test]
fn test_index_expr() {
    let (hir, _inter, body_id) = get_body_hir("fn f() { a[0] }");
    let body = get_body(&hir, body_id);
    let block_id = last_expr_id(body);
    match &body.exprs[block_id] {
        Expr::Block {
            tail: Some(index_id),
            ..
        } => {
            assert!(matches!(&body.exprs[*index_id], Expr::Index { .. }));
        }
        _ => panic!(),
    }
}

#[test]
fn test_array_expr() {
    let (hir, _inter, body_id) = get_body_hir("fn f() { [1, 2, 3] }");
    let body = get_body(&hir, body_id);
    let block_id = last_expr_id(body);
    match &body.exprs[block_id] {
        Expr::Block {
            tail: Some(arr_id), ..
        } => match &body.exprs[*arr_id] {
            Expr::Array(elems) => assert_eq!(elems.len(), 3),
            _ => panic!(),
        },
        _ => panic!(),
    }
}

#[test]
fn test_tuple_expr() {
    let (hir, _inter, body_id) = get_body_hir("fn f() { (1, 2) }");
    let body = get_body(&hir, body_id);
    let block_id = last_expr_id(body);
    match &body.exprs[block_id] {
        Expr::Block {
            tail: Some(tup_id), ..
        } => match &body.exprs[*tup_id] {
            Expr::Tuple(elems) => assert_eq!(elems.len(), 2),
            _ => panic!(),
        },
        _ => panic!(),
    }
}

#[test]
fn test_range_expr() {
    let (hir, _inter, body_id) = get_body_hir("fn f() { 0..10 }");
    let body = get_body(&hir, body_id);
    let block_id = last_expr_id(body);
    match &body.exprs[block_id] {
        Expr::Block {
            tail: Some(range_id),
            ..
        } => match &body.exprs[*range_id] {
            Expr::Range {
                start,
                end,
                inclusive,
            } => {
                assert!(start.is_some());
                assert!(end.is_some());
                assert!(!inclusive);
            }
            _ => panic!(),
        },
        _ => panic!(),
    }
}

#[test]
fn test_method_call_expr() {
    // §3.3: the receiver must be lowered separately and MUST NOT appear in
    // `args`. A regression here feeds the receiver as a leading explicit
    // argument and corrupts the call's argument list.
    let (hir, _inter, body_id) = get_body_hir("fn f() { a.b(1) }");
    let body = get_body(&hir, body_id);
    let block_id = last_expr_id(body);
    match &body.exprs[block_id] {
        Expr::Block {
            tail: Some(call_id),
            ..
        } => match &body.exprs[*call_id] {
            Expr::MethodCall { receiver, args, .. } => {
                assert_eq!(args.len(), 1, "single explicit arg expected");
                assert!(
                    !args.contains(receiver),
                    "receiver must NOT be duplicated into args"
                );
            }
            _ => panic!(),
        },
        _ => panic!(),
    }
}

#[test]
fn test_method_call_multi_arg_receiver_excluded() {
    // §3.3: a 3-arg method call keeps the receiver separate and lists exactly
    // the explicit arguments (no receiver, no dropped args).
    let (hir, _inter, body_id) = get_body_hir("fn f() { a.b(1, 2) }");
    let body = get_body(&hir, body_id);
    let block_id = last_expr_id(body);
    match &body.exprs[block_id] {
        Expr::Block {
            tail: Some(call_id),
            ..
        } => match &body.exprs[*call_id] {
            Expr::MethodCall { receiver, args, .. } => {
                assert_eq!(args.len(), 2, "both explicit args must be present");
                assert!(
                    !args.contains(receiver),
                    "receiver must NOT be duplicated into args"
                );
            }
            _ => panic!(),
        },
        _ => panic!(),
    }
}

#[test]
fn test_pattern_or() {
    let (hir, _inter, body_id) = get_body_hir("fn f(x: i32) { match x { 0 | 1 => 2, _ => 3 } }");
    let body = get_body(&hir, body_id);
    let block_id = last_expr_id(body);
    match &body.exprs[block_id] {
        Expr::Block {
            tail: Some(match_id),
            ..
        } => match &body.exprs[*match_id] {
            Expr::Match { arms, .. } => {
                assert_eq!(arms.len(), 2);
                if let Pat::Or(pats) = &body.pats[arms[0].pat] {
                    assert_eq!(pats.len(), 2);
                } else {
                    panic!();
                }
            }
            _ => panic!(),
        },
        _ => panic!(),
    }
}

#[test]
fn test_float_literal_type() {
    let (hir, _inter, body_id) = get_body_hir("fn f() { 3.14 }");
    let body = get_body(&hir, body_id);
    let block_id = last_expr_id(body);
    match &body.exprs[block_id] {
        Expr::Block {
            tail: Some(lit_id), ..
        } => match &body.exprs[*lit_id] {
            Expr::Literal(lit) => assert!(matches!(lit, Literal::Float(..))),
            _ => panic!(),
        },
        _ => panic!(),
    }
}
