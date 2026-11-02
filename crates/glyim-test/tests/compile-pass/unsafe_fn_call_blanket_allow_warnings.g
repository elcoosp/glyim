// §3.5.30 attribute resolution: `#[allow(warnings)]` (blanket group
// suppression) suppresses the unsafe-fn-call warning without naming the
// lint. Matches rustc's short form.
// test-mode: compile-pass
// forbid-warning: call to unsafe function
unsafe fn get_value() -> i32 {
    42
}

#[allow(warnings)]
fn main() -> i32 {
    get_value()
}
