// §3.5.30 framework: `-A unsafe` suppresses the warning.
// test-mode: compile-pass
// compile-flags: -A unsafe
// forbid-warning: call to unsafe function
unsafe fn get_value() -> i32 {
    42
}

fn main() -> i32 {
    get_value()
}
