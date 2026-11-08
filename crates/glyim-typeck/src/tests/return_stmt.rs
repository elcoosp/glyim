use super::common::*;
use crate::thir;
use glyim_hir::*;

#[test]
fn return_expression() {
    let exprs = vec![
        Expr::Literal(Literal::Int(42, None)),
        Expr::Return {
            value: Some(ExprId::from_raw(0)),
        },
    ];

    let (hir, body_id) = make_single_body_hir(exprs);
    let thir_body = typeck_single_body(&hir, body_id);
    // First expression becomes stmt, Return is always stmt
    assert_eq!(thir_body.stmts.len(), 2);
    match &thir_body.stmts[1] {
        thir::Stmt::Return { value, .. } => {
            assert!(value.is_some());
        }
        _ => panic!("Expected Return as last stmt"),
    }
}
