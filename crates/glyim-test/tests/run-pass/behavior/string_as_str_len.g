// `String::as_str` must extract the (ptr, len) from the Vec-shaped
// `{[3 x i64]}` and rebuild a `{ptr, i64}` fat pointer — not store the
// 24-byte value into a 16-byte slot (which is what the old "identity"
// arm did, causing `s.as_str().len()` to read a garbage length, and
// `s.as_str() == "x"` to segfault).
//@ mode: run-pass
//@ expect-exit: 5
//@ compile-flags: --with-stdlib
//@ feature: LANG-BUILTIN-STRING, STD-ALLOC-STRING, LANG-TY-STR, TOOL-TEST-RUN-PASS
//@ backends: gam, llvm
fn main() -> i32 {
    let s = String::from("hello");
    s.as_str().len() as i32
}
