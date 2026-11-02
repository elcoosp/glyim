// §3.5.30 item 4: free-fn unsafe call outside `unsafe { }` must warn.
// test-mode: compile-pass
// expect-warning: call to unsafe function `get_value` outside of an `unsafe` block
unsafe fn get_value() -> i32 {
    42
}

fn main() -> i32 {
    get_value()
}
