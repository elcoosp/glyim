// `include!` splices the target file's items in at the call site, so the
// helper's body is compiled as part of this crate.
// test-mode: run-pass
// exit-code: 41
include!("helper.g");
fn main() -> i32 { helper() }
