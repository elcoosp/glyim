#[allow(unused_imports, dead_code)]
use crate::common::*;

#[test]
fn e2e_main_42() {
    assert_eq!(pipeline::run(&temp_g("main = () => 42"), None).unwrap(), 42);
}

