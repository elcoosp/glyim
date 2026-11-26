use super::common::*;
use glyim_hir::*;

#[test]
fn array_literal_and_index() {
    let mut exprs = vec![
        Expr::Literal(Literal::Int(1, None)),
        Expr::Literal(Literal::Int(2, None)),
        Expr::Literal(Literal::Int(3, None)),
    ];
    exprs.push(Expr::Array(vec![
        ExprId::from_usize(0),
        ExprId::from_usize(1),
        ExprId::from_usize(2),
    ]));
    exprs.push(Expr::Literal(Literal::Int(1, None)));
    exprs.push(Expr::Index {
        base: ExprId::from_usize(3),
        index: ExprId::from_usize(4),
    });

    let (hir, body_id) = make_single_body_hir(exprs);
    let thir_body = typeck_single_body(&hir, body_id);
    assert_eq!(thir_body.stmts.len(), 6);
}
