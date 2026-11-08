use super::common::*;
use glyim_hir::*;

#[test]
fn tuple_index() {
    let exprs = vec![
        Expr::Literal(Literal::Int(1, None)),
        Expr::Literal(Literal::Bool(true)),
        Expr::Tuple(vec![ExprId::from_raw(0), ExprId::from_raw(1)]),
        Expr::Literal(Literal::Int(0, None)),
        Expr::Index {
            base: ExprId::from_raw(2),
            index: ExprId::from_raw(3),
        },
    ];

    let (hir, body_id) = make_single_body_hir(exprs);
    let thir_body = typeck_single_body(&hir, body_id);
    assert_eq!(thir_body.stmts.len(), 5);
}
