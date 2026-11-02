// §3.5.30 MVP: calling a declared-unsafe free function outside any
// `unsafe { }` block must compile (warning-only in the MVP) and run.
// test-mode: run-pass
// exit-code: 42
unsafe fn get_value() -> i32 {
    42
}

fn main() -> i32 {
    get_value()
}
