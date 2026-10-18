// `&STATIC` yields the address of a real LLVM global, so a `*p` read
// through that pointer observes the same value the initializer set.
// test-mode: run-pass
// exit-code: 7
// compile-flags: --with-stdlib
static VALUE: i32 = 7;
fn main() -> i32 { let p = &VALUE; *p }
