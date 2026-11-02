// §3.5.30 attribute resolution: `#[allow(unsafe_fn_call)]` on the enclosing
// fn suppresses the warning.
// test-mode: compile-pass
// forbid-warning: call to unsafe function
unsafe fn get_value() -> i32 {
    42
}

#[allow(unsafe_fn_call)]
fn main() -> i32 {
    get_value()
}
