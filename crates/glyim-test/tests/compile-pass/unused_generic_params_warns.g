// §3.5.30 framework: unused generic parameter on a monomorphized fn
// emits a warning by default.
// test-mode: compile-pass
// expect-warning: unused generic parameter
fn unused<T>() -> i32 {
    42
}

fn main() -> i32 {
    unused::<i32>() - 42
}
