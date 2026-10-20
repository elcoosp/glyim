// The stdlib's actual shape: a unit struct with inherent methods, used
// through a `const` of that type.
// test-mode: run-pass
// exit-code: 29
// compile-flags: --with-stdlib
struct Counter;
impl Counter {
    fn value(&self) -> i32 { 29 }
}
const C: Counter = Counter;
fn main() -> i32 { C.value() }
