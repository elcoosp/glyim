use super::common::*;
use glyim_hir::*;

#[test]
fn break_and_continue_inside_loop() {
    let exprs = vec![
        Expr::Break { value: None },
        Expr::Continue,
        Expr::Block {
            stmts: vec![ExprId::from_raw(0), ExprId::from_raw(1)],
            tail: None,
        },
        Expr::Loop {
            body: ExprId::from_raw(2),
        },
    ];

    let (hir, body_id) = make_single_body_hir(exprs);
    let thir_body = typeck_single_body(&hir, body_id);
    assert_eq!(thir_body.stmts.len(), 4);
}
