// §3.5.30 attribute resolution: `#[deny(unsafe_fn_call)]` on the *caller*
// promotes the diagnostic for a call to an `unsafe fn` inherent method.
// test-mode: compile-fail
struct S;

impl S {
    unsafe fn get_value() -> i32 {
        42
    }
}

#[deny(unsafe_fn_call)]
fn main() -> i32 {
    S::get_value() //~ ERROR call to unsafe function
}
