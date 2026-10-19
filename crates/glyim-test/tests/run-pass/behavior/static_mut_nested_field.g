// Nested-struct field write through a `static mut` — Field(0) then
// Field(0) on a StaticPtr.
// test-mode: run-pass
// exit-code: 20
// compile-flags: --with-stdlib
struct Inner { v: i32 }
struct Outer { inner: Inner, extra: i32 }
static mut O: Outer = Outer { inner: Inner { v: 5 }, extra: 7 };
fn main() -> i32 { unsafe { O.inner.v = 13; O.inner.v + O.extra } }
