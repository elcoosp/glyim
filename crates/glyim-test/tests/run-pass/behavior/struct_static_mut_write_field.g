// Struct-typed `static mut` write through `&mut STATIC.field`, then a
// direct field read — the byte-GEP write path and the byte-GEP read
// path must agree on the field offset.
// test-mode: run-pass
// exit-code: 89
// compile-flags: --with-stdlib
struct Pair { a: i32, b: i32 }
static mut P: Pair = Pair { a: 17, b: 26 };
fn main() -> i32 {
    unsafe {
        P.a = 63;
        P.a + P.b
    }
}
