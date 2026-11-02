// §3.5.30 attribute resolution: `#[allow(unsafe_fn_call)]` on the *caller*
// suppresses the diagnostic for a call to an `unsafe fn` inherent method.
// (Same semantic as free-fn attribute resolution — the attribute is honoured
// where the diagnostic fires, i.e. at the call site, not at the callee.)
// test-mode: compile-pass
// forbid-warning: call to unsafe function
struct S;

impl S {
    unsafe fn get_value() -> i32 {
        42
    }
}

#[allow(unsafe_fn_call)]
fn main() -> i32 {
    S::get_value()
}
