// Tuple-field write through a `static mut` — exercises Field projection
// through a StaticPtr in the interpreter's write path.
// test-mode: run-pass
// exit-code: 25
// compile-flags: --with-stdlib
static mut T: (i32, i32) = (10, 20);
fn main() -> i32 { unsafe { T.0 = 5; T.0 + T.1 } }
