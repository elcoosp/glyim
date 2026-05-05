#[allow(unused_imports, dead_code)]
use crate::common::*;

#[test]
fn e2e_add() {
    assert_eq!(
        pipeline::run(&temp_g("main = () => 1 + 2"), None).unwrap(),
        3
    );
}

