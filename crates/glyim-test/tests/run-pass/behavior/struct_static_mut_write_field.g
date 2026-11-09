// Struct-typed `static mut` write through `&mut STATIC.field`, then a
// direct field read — the byte-GEP write path and the byte-GEP read
// path must agree on the field offset.
//@ mode: run-pass
//@ expect-exit: 89
//@ compile-flags: --with-stdlib
//@ feature: LANG-SYN-STRUCT, LANG-SYN-FIELD-ACCESS, LANG-SCOPE-STATIC-ITEM, LANG-SYN-MUT, TOOL-TEST-RUN-PASS
//@ backends: gam, llvm
struct Pair { a: i32, b: i32 }
static mut P: Pair = Pair { a: 17, b: 26 };
fn main() -> i32 {
    unsafe {
        P.a = 63;
        P.a + P.b
    }
}
