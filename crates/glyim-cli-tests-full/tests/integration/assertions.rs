#[allow(unused_imports, dead_code)]
use crate::common::*;

#[test]
fn e2e_assert_pass() {
    let _ = pipeline::run(&temp_g("main = () => { assert(1 == 1) }"), None).unwrap();
}

#[test]
#[ignore = "assert(0) calls abort which kills the test process; requires subprocess with SIGABRT handling"]
fn e2e_assert_fail() {
    // When JIT subprocess isolation is available, test that assert(0) produces
    // non-zero exit and stderr contains "assertion failed"
    let _ = pipeline::run(&temp_g("main = () => { assert(0) }"), None);
    // Can't check result because abort kills the process
}

#[test]
#[ignore = "assert(0) calls abort which kills the test process; requires subprocess with SIGABRT handling"]
fn e2e_assert_fail_msg() {
    // When JIT subprocess isolation is available, test that assert(0, "oops")
    // produces stderr containing "oops"
    let _ = pipeline::run(&temp_g(r#"main = () => { assert(0, "oops") }"#), None);
    // Can't check result because abort kills the process
}

