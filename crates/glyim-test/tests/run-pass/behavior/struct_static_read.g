// Struct-typed static read: same structural Adt code path, but reached
// through the statics feature (StaticRef global with an Aggregate init).
// test-mode: run-pass
// exit-code: 43
// compile-flags: --with-stdlib
struct Pair { a: i32, b: i32 }
static P: Pair = Pair { a: 17, b: 26 };
fn main() -> i32 { P.a + P.b }
