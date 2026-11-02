// §3.5.30 attribute resolution: explicit `#[warn(..)]` still warns.
// test-mode: compile-pass
// expect-warning: call to unsafe function
unsafe fn get_value() -> i32 {
    42
}

#[warn(unsafe_fn_call)]
fn main() -> i32 {
    get_value()
}
