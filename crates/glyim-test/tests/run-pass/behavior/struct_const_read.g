// Struct-typed constant read: exercises the structural Adt LLVM type
// so `Aggregate` const lowering can insert field 1 without out-of-range.
// test-mode: run-pass
// exit-code: 7
// compile-flags: --with-stdlib
struct Pair { a: i32, b: i32 }
const P: Pair = Pair { a: 3, b: 4 };
fn main() -> i32 { P.a + P.b }
