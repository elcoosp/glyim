#[allow(unused_imports, dead_code)]
use crate::common::*;

#[test]
fn jit_compile_and_run_simple() {
    assert_eq!(glyim_cli::pipeline::run_jit("main = () => 42").unwrap(), 42);
}
