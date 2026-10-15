// and_then: closure returning Option<U> — nested ADT position.
// test-mode: run-pass
// exit-code: 9
// compile-flags: --with-stdlib
fn main() -> i32 {
    let o = Option::Some(3);
    o.and_then(|x| Option::Some(x + 6)).unwrap()
}
