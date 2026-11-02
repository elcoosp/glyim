// §3.5.30 framework: `-D unsafe` promotes the call-to-unsafe-fn lint to
// a compilation error. Verifies that the Deny path emits an Error-severity
// diagnostic (not a Warning) and that the pipeline aborts.
// test-mode: compile-fail
// compile-flags: -D unsafe
unsafe fn get_value() -> i32 {
    42
}

fn main() -> i32 {
    get_value() //~ ERROR call to unsafe function
}
