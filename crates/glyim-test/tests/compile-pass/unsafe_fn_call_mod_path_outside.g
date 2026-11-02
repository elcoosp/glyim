// §3.5.30 item 4: `mod::f` unsafe call outside `unsafe { }` must warn.
// test-mode: compile-pass
// expect-warning: call to unsafe function
mod m {
    unsafe fn get_value() -> i32 {
        42
    }
}

fn main() -> i32 {
    m::get_value()
}
