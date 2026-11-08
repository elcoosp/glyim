use super::common::*;
use glyim_hir::*;

#[test]
fn while_loop_bool_condition() {
    let exprs = vec![
        Expr::Literal(Literal::Bool(true)),
        Expr::Block {
            stmts: Vec::new(),
            tail: None,
        },
        Expr::While {
            cond: ExprId::from_raw(0),
            body: ExprId::from_raw(1),
        },
    ];

    let (hir, body_id) = make_single_body_hir(exprs);
    let thir_body = typeck_single_body(&hir, body_id);
    assert_eq!(thir_body.stmts.len(), 3);
}
