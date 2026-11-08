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
fn test_binary_expr() {
    let (hir, _interner, body_id) = get_body_hir("fn f() { 1 + 2 }");
    let body = get_body(&hir, body_id);
    let block_id = last_expr_id(body);
    match &body.exprs[block_id] {
        Expr::Block {
            tail: Some(bin_id), ..
        } => {
            let expr = &body.exprs[*bin_id];
            match expr {
                Expr::Binary { op, lhs, rhs } => {
                    assert_eq!(*op, BinOp::Add);
                    match &body.exprs[*lhs] {
                        Expr::Literal(lit) => assert_eq!(lit, &Literal::Int(1, None)),
                        _ => panic!(),
                    }
                    match &body.exprs[*rhs] {
                        Expr::Literal(lit) => assert_eq!(lit, &Literal::Int(2, None)),
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
fn test_if_expr() {
    let (hir, _interner, body_id) = get_body_hir("fn f() { if true { 1 } else { 0 } }");
    let body = get_body(&hir, body_id);
    let block_id = last_expr_id(body);
    match &body.exprs[block_id] {
        Expr::Block {
            tail: Some(if_id), ..
        } => {
            let expr = &body.exprs[*if_id];
            match expr {
                Expr::If {
                    cond,
                    then_branch,
                    else_branch,
                } => {
                    assert!(matches!(
                        &body.exprs[*cond],
                        Expr::Literal(Literal::Bool(true))
                    ));
                    let then_val = &body.exprs[*then_branch];
                    if let Expr::Block {
                        tail: Some(tail_id),
                        ..
                    } = then_val
                    {
                        assert!(matches!(
                            &body.exprs[*tail_id],
                            Expr::Literal(Literal::Int(1, None))
                        ));
                    } else if let Expr::Literal(lit) = then_val {
                        assert_eq!(lit, &Literal::Int(1, None));
                    } else {
                        panic!("Then branch not a literal or block");
                    }
                    let else_id = else_branch.expect("else branch missing");
                    let else_val = &body.exprs[else_id];
                    if let Expr::Block {
                        tail: Some(tail_id),
                        ..
                    } = else_val
                    {
                        assert!(matches!(
                            &body.exprs[*tail_id],
                            Expr::Literal(Literal::Int(0, None))
                        ));
                    } else if let Expr::Literal(lit) = else_val {
                        assert_eq!(lit, &Literal::Int(0, None));
                    } else {
                        panic!("Else branch not a literal or block");
                    }
                }
                _ => panic!(),
            }
        }
        _ => panic!(),
    }
}

#[test]
fn test_path_expr() {
    let (hir, interner, body_id) = get_body_hir("fn f() { x }");
    let body = get_body(&hir, body_id);
    let block_id = last_expr_id(body);
    match &body.exprs[block_id] {
        Expr::Block {
            tail: Some(path_id),
            ..
        } => match &body.exprs[*path_id] {
            Expr::Path(path) => {
                assert_eq!(path.as_name().unwrap(), interner.intern("x"));
            }
            _ => panic!(),
        },
        _ => panic!(),
    }
}

#[test]
fn test_literal_expr() {
    let (hir, _interner, body_id) = get_body_hir("fn f() { 42 }");
    let body = get_body(&hir, body_id);
    let block_id = last_expr_id(body);
    match &body.exprs[block_id] {
        Expr::Block {
            tail: Some(lit_id), ..
        } => match &body.exprs[*lit_id] {
            Expr::Literal(lit) => assert_eq!(lit, &Literal::Int(42, None)),
            _ => panic!(),
        },
        _ => panic!(),
    }
}
