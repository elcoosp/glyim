// §3.5.30 MVP: same call inside an `unsafe { }` block must compile with
// no warning (unsafe_depth > 0 suppresses it) and run identically.
// test-mode: run-pass
// exit-code: 42
unsafe fn get_value() -> i32 {
    42
}

fn main() -> i32 {
    unsafe { get_value() }
}
