use super::common::*;
use glyim_hir::*;

#[test]
fn struct_field_access() {
    let name_x = name("x");
    let name_field = name("field");
    let exprs = vec![
        Expr::Path(glyim_hir::Path::from_single(name_x)),
        Expr::Field {
            receiver: ExprId::from_raw(0),
            field: name_field,
        },
    ];

    let (hir, body_id) = make_single_body_hir(exprs);
    let thir_body = typeck_single_body(&hir, body_id);
    assert_eq!(thir_body.stmts.len(), 2);
}
