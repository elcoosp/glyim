// Array-of-struct field write through a `static mut` — ConstantIndex,
// Field.
//@ mode: run-pass
//@ expect-exit: 34
//@ compile-flags: --with-stdlib
//@ feature: LANG-SYN-STRUCT, LANG-TY-ARRAY, LANG-SCOPE-STATIC-ITEM, LANG-SYN-MUT, TOOL-TEST-RUN-PASS
//@ backends: gam, llvm
struct Pair { a: i32, b: i32 }
static mut P: [Pair; 2] = [Pair { a: 1, b: 2 }, Pair { a: 3, b: 4 }];
fn main() -> i32 {
    unsafe {
        P[1].a = 27;
        P[0].a + P[0].b + P[1].a + P[1].b
    }
}
