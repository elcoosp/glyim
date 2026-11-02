// §3.5.30 attribute resolution: `#[allow(unsafe)]` alias works.
// test-mode: compile-pass
// forbid-warning: call to unsafe function
unsafe fn get_value() -> i32 {
    42
}

#[allow(unsafe)]
fn main() -> i32 {
    get_value()
}
