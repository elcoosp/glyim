#[allow(unused_imports, dead_code)]
use crate::common::*;

#[test]
fn e2e_macro_identity() {
    assert_eq!(
        pipeline::run(&temp_g(
            "@identity fn transform(expr: Expr) -> Expr { return expr } main = () => @identity(99)"
        ), None)
        .unwrap(),
        99
    );
}

