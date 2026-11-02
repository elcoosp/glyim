// §3.5.30 item 4: free-fn unsafe call inside `unsafe { }` must NOT warn.
// test-mode: compile-pass
// forbid-warning: call to unsafe function
unsafe fn get_value() -> i32 {
    42
}

fn main() -> i32 {
    unsafe { get_value() }
}
