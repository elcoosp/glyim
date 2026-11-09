// Nested-struct field write through a `static mut` — Field(0) then
// Field(0) on a StaticPtr.
//@ mode: run-pass
//@ expect-exit: 20
//@ compile-flags: --with-stdlib
//@ feature: LANG-SYN-FIELD-ACCESS, LANG-SCOPE-STATIC-ITEM, LANG-SYN-MUT, TOOL-TEST-RUN-PASS
//@ backends: gam, llvm
struct Inner { v: i32 }
struct Outer { inner: Inner, extra: i32 }
static mut O: Outer = Outer { inner: Inner { v: 5 }, extra: 7 };
fn main() -> i32 { unsafe { O.inner.v = 13; O.inner.v + O.extra } }
