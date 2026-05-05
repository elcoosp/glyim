#[allow(unused_imports, dead_code)]
use crate::common::*;

#[test]
fn e2e_main_42() {
    assert_eq!(pipeline::run(&temp_g("main = () => 42"), None).unwrap(), 42);
}

#[test]
fn critical_addition_must_not_be_mutated() {
    let result = glyim_compiler::pipeline::run_jit("main = () => 1 + 2").unwrap();
    assert_eq!(result, 3);
}
