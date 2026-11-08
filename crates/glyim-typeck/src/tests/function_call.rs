use super::common::*;
use glyim_hir::*;

#[test]
fn function_call_with_args() {
    let exprs = vec![
        Expr::Path(glyim_hir::Path::from_single(name("f"))),
        Expr::Literal(Literal::Int(1, None)),
        Expr::Literal(Literal::Bool(true)),
        Expr::Call {
            func: ExprId::from_raw(0),
            args: vec![ExprId::from_raw(1), ExprId::from_raw(2)],
        },
    ];

    let (hir, body_id) = make_single_body_hir(exprs);
    let thir_body = typeck_single_body(&hir, body_id);
    assert_eq!(thir_body.stmts.len(), 4);
}
