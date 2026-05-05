#[allow(unused_imports, dead_code)]
use crate::common::*;

#[test]
fn e2e_let_binding() {
    assert_eq!(
        pipeline::run(&temp_g("main = () => { let x = 42 }"), None).unwrap(),
        0
    );
}

#[test]
fn e2e_let_mut_assign() {
    assert_eq!(
        pipeline::run(
            &temp_g("main = () => { let mut x = 10\nx = x + 5\nx }"),
            None
        )
        .unwrap(),
        15
    );
}

