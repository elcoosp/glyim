// §3.5.30 framework: `-W unsafe` is a no-op (default already warns).
// test-mode: compile-pass
// compile-flags: -W unsafe
// expect-warning: call to unsafe function
unsafe fn get_value() -> i32 {
    42
}

fn main() -> i32 {
    get_value()
}
