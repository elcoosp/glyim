// KNOWN FAILING — nested `Option<Option<i32>>` unwrap chain.
// `Some(Some(6)).unwrap().unwrap()` reports "expected integer type, found
// Adt1010<?ty>": the inner unwrap's return var isn't linked to the outer's
// payload var. Pre-existing; same "unlinked infer var" class.
// test-mode: run-pass
// exit-code: 6
// compile-flags: --with-stdlib
// needs-llvm
fn main() -> i32 {
    let o: Option<Option<i32>> = Option::Some(Option::Some(6));
    o.unwrap().unwrap()
}
