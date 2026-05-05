#[allow(unused_imports, dead_code)]
use crate::common::*;

#[test]
#[ignore = "nested generics: 0 as T in struct literals needs rewriting"]
fn stress_nest_vec() {
    let src = include_str!("../../../../tests/stress/nest_vec.g");
    assert_eq!(glyim_cli::pipeline::run_jit(src).unwrap(), 0);
}

#[test]
#[ignore = "nested generics: type annotations need full concretization pass"]
fn stress_nest_option() {
    let src = include_str!("../../../../tests/stress/nest_option.g");
    assert_eq!(glyim_cli::pipeline::run_jit(src).unwrap(), 42);
}

