// §3.5.30 item 2: calling an unsafe fn through a module path
// (`m::get_value`) outside any `unsafe { }` block must compile (warn)
// and run. Warning presence is verified by manual probe, not by
// check-stderr (which captures program stderr, not compiler output).
// test-mode: run-pass
// exit-code: 42
mod m {
    unsafe fn get_value() -> i32 {
        42
    }
}

fn main() -> i32 {
    m::get_value()
}
