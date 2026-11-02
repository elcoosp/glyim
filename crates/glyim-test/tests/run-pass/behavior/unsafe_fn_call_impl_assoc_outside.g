// §3.5.30 item 2: calling an inherent associated `unsafe fn`
// (`S::get_value()`) outside any `unsafe { }` block must compile
// (warn) and run.
// test-mode: run-pass
// exit-code: 42
struct S;

impl S {
    unsafe fn get_value() -> i32 {
        42
    }
}

fn main() -> i32 {
    S::get_value()
}
