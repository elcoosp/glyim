use super::common::*;
use glyim_hir::*;

#[test]
fn match_integer_with_wildcard() {
    let mut exprs = Vec::new();
    let mut pats = Vec::new();
    exprs.push(Expr::Literal(Literal::Int(42, None)));
    exprs.push(Expr::Literal(Literal::Int(0, None)));
    pats.push(Pat::Wild);
    let arm = MatchArm {
        pat: PatId::from_usize(0),
        guard: None,
        body: ExprId::from_usize(1),
    };
    exprs.push(Expr::Match {
        scrutinee: ExprId::from_usize(0),
        arms: vec![arm],
    });

    let (mut hir, body_id) = make_single_body_hir(exprs);
    let body = &mut hir.bodies[body_id];
    body.pats = {
        let mut p = glyim_core::arena::IndexVec::new();
        for pat in pats {
            p.push(pat);
        }
        p
    };

    let thir_body = typeck_single_body(&hir, body_id);
    // The arm body is checked in-scope by the `Expr::Match` handler and is
    // embedded in the resulting THIR `Match`, not emitted as a separate
    // top-level statement. So the top-level driving loop produces: the
    // scrutinee literal, the match expression (which carries the arm body
    // inside it) = 2 statements.
    assert_eq!(thir_body.stmts.len(), 2);
}
