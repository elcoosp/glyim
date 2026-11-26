use super::common::*;
use crate::thir;
use glyim_hir::*;

#[test]
fn assign_to_local() {
    let exprs = vec![
        Expr::Path(glyim_hir::Path::from_single(name("x"))),
        Expr::Literal(Literal::Int(5, None)),
        Expr::Assign {
            lhs: ExprId::from_usize(0),
            rhs: ExprId::from_usize(1),
        },
    ];

    let (hir, body_id) = make_single_body_hir(exprs);
    let thir_body = typeck_single_body(&hir, body_id);
    // All three expressions become statements (Assign is always a Stmt::Assign)
    assert_eq!(thir_body.stmts.len(), 3);
    // Check the last is Assign
    match &thir_body.stmts[2] {
        thir::Stmt::Assign { .. } => {}
        _ => panic!("Expected Assign"),
    }
}
