// Array-of-struct field write through a `static mut` — ConstantIndex,
// Field.
// test-mode: run-pass
// exit-code: 34
// compile-flags: --with-stdlib
struct Pair { a: i32, b: i32 }
static mut P: [Pair; 2] = [Pair { a: 1, b: 2 }, Pair { a: 3, b: 4 }];
fn main() -> i32 {
    unsafe {
        P[1].a = 27;
        P[0].a + P[0].b + P[1].a + P[1].b
    }
}
