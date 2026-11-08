use crate::lower::lower_crate;
use crate::{BodyId, Expr, ExprId, ItemId, ItemKind, Literal};
use glyim_core::interner::Interner;
use glyim_core::primitives::BinOp;
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
fn test_binary_expr_chained() {
    let (hir, interner, body_id) = get_body_hir("fn f() { a + b * c }");
    let body = get_body(&hir, body_id);
    let block_id = last_expr_id(body);
    match &body.exprs[block_id] {
        Expr::Block {
            tail: Some(expr_id),
            ..
        } => {
            let expr = &body.exprs[*expr_id];
            match expr {
                Expr::Binary { op, lhs, .. } => {
                    assert_eq!(*op, BinOp::Add);
                    match &body.exprs[*lhs] {
                        Expr::Path(p) => assert_eq!(p.as_name().unwrap(), interner.intern("a")),
                        _ => panic!(),
                    }
                }
                _ => panic!(),
            }
        }
        _ => panic!(),
    }
}

#[test]
fn test_if_else_nested() {
    let (hir, _interner, body_id) =
        get_body_hir("fn f() { if a { 1 } else { if b { 2 } else { 3 } } }");
    let body = get_body(&hir, body_id);
    let block_id = last_expr_id(body);
    match &body.exprs[block_id] {
        Expr::Block {
            tail: Some(if_id), ..
        } => {
            let outer_if = &body.exprs[*if_id];
            match outer_if {
                Expr::If {
                    then_branch,
                    else_branch: Some(else_id),
                    ..
                } => {
                    // Then branch
                    let then_val = &body.exprs[*then_branch];
                    let then_lit = match then_val {
                        Expr::Block {
                            tail: Some(tail_id),
                            ..
                        } => &body.exprs[*tail_id],
                        Expr::Literal(_) => then_val,
                        _ => panic!("Unexpected then branch shape"),
                    };
                    assert!(matches!(then_lit, Expr::Literal(Literal::Int(1, None))));

                    // Else branch
                    let else_expr = &body.exprs[*else_id];
                    let inner_if = match else_expr {
                        Expr::If { .. } => else_expr,
                        Expr::Block {
                            tail: Some(tail_id),
                            ..
                        } => &body.exprs[*tail_id],
                        _ => panic!("Expected nested If or Block containing If"),
                    };
                    match inner_if {
                        Expr::If {
                            then_branch: inner_then,
                            ..
                        } => {
                            let inner_val = &body.exprs[*inner_then];
                            let inner_lit = match inner_val {
                                Expr::Block {
                                    tail: Some(tail_id),
                                    ..
                                } => &body.exprs[*tail_id],
                                Expr::Literal(_) => inner_val,
                                _ => panic!(),
                            };
                            assert!(matches!(inner_lit, Expr::Literal(Literal::Int(2, None))));
                        }
                        _ => panic!("Expected nested If"),
                    }
                }
                _ => panic!("Expected If"),
            }
        }
        _ => panic!("Expected Block"),
    }
}

#[test]
fn test_let_stmt() {
    let (hir, _interner, body_id) = get_body_hir("fn f() { let x = 5; x }");
    let body = get_body(&hir, body_id);
    let block_id = last_expr_id(body);
    match &body.exprs[block_id] {
        Expr::Block { stmts, tail } => {
            assert!(!stmts.is_empty(), "Expected at least one statement");
            let assign_found = stmts
                .iter()
                .any(|&sid| matches!(&body.exprs[sid], Expr::Let { .. }));
            assert!(assign_found, "Expected a Let for let statement");
            match tail {
                Some(tail_id) => assert!(matches!(&body.exprs[*tail_id], Expr::Path(_))),
                None => panic!("Expected tail expression"),
            }
        }
        _ => panic!(),
    }
}

#[test]
fn test_range_expr_inclusive() {
    let (hir, _interner, body_id) = get_body_hir("fn f() { 0..=10 }");
    let body = get_body(&hir, body_id);
    let block_id = last_expr_id(body);
    match &body.exprs[block_id] {
        Expr::Block {
            tail: Some(range_id),
            ..
        } => {
            let expr = &body.exprs[*range_id];
            match expr {
                Expr::Range {
                    start,
                    end,
                    inclusive,
                } => {
                    assert!(start.is_some());
                    assert!(end.is_some());
                    assert!(inclusive);
                }
                _ => panic!("Expected Range"),
            }
        }
        _ => panic!("Expected Block"),
    }
}
