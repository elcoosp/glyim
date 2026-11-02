// §3.5.30 attribute resolution: `#[deny(..)]` on the enclosing fn promotes
// the unsafe-fn-call diagnostic to error. The error fires at the call site
// (inside `main`), not at the callee declaration — the diagnostic's span
// is the call expression, matching rustc's behavior.
// test-mode: compile-fail
unsafe fn get_value() -> i32 {
    42
}

#[deny(unsafe_fn_call)]
fn main() -> i32 {
    get_value() //~ ERROR call to unsafe function
}
