use crate::lower::lower_crate;
use crate::{BodyId, Expr, ExprId, ItemId, ItemKind};
use glyim_core::interner::Interner;
use glyim_core::primitives::{BinOp, UnOp};
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
fn test_binary_expr_with_lt() {
    let (hir, _interner, body_id) = get_body_hir("fn f() { 1 < 2 }");
    let body = get_body(&hir, body_id);
    let block_id = last_expr_id(body);
    match &body.exprs[block_id] {
        Expr::Block {
            tail: Some(bin_id), ..
        } => match &body.exprs[*bin_id] {
            Expr::Binary { op, .. } => assert_eq!(*op, BinOp::Lt),
            _ => panic!(),
        },
        _ => panic!(),
    }
}

#[test]
fn test_assign_inside_block() {
    let (hir, _interner, body_id) = get_body_hir("fn f() { x = 42; x }");
    let body = get_body(&hir, body_id);
    let block_id = last_expr_id(body);
    match &body.exprs[block_id] {
        Expr::Block { stmts, tail } => {
            assert_eq!(stmts.len(), 1);
            assert!(matches!(&body.exprs[stmts[0]], Expr::Assign { .. }));
            match tail {
                Some(tail_id) => assert!(matches!(&body.exprs[*tail_id], Expr::Path(_))),
                _ => panic!(),
            }
        }
        _ => panic!(),
    }
}

#[test]
fn test_deref_unary() {
    let (hir, _interner, body_id) = get_body_hir("fn f() { *x }");
    let body = get_body(&hir, body_id);
    let block_id = last_expr_id(body);
    match &body.exprs[block_id] {
        Expr::Block {
            tail: Some(unary_id),
            ..
        } => match &body.exprs[*unary_id] {
            Expr::Unary { op, .. } => assert_eq!(*op, UnOp::Deref),
            _ => panic!(),
        },
        _ => panic!(),
    }
}

#[test]
fn test_return_expr() {
    let (hir, _interner, body_id) = get_body_hir("fn f() { return 5; }");
    let body = get_body(&hir, body_id);
    let block_id = last_expr_id(body);
    match &body.exprs[block_id] {
        Expr::Block { stmts, .. } => {
            assert_eq!(stmts.len(), 1);
            assert!(matches!(&body.exprs[stmts[0]], Expr::Return { .. }));
        }
        _ => panic!(),
    }
}
