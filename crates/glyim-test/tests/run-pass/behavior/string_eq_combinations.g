// `String`/`&str` equality. `String` (Adt1050) is Vec-shaped in LLVM
// (`{[3 x i64]}`), `&str` is a `{ptr, i64}` fat pointer, and the two
// previously could not be compared: `String == String` ICEd with
// "[X0000] eq: mismatched types" (is_string_shaped only accepted
// TyKind::String, never Adt1050), and `String.as_str() == "x"` compiled
// but segfaulted (as_str stored the 24-byte Vec shape into a 16-byte
// slot). Per-side shape extraction in codegen's `string_ptr_len` fixes
// all combinations. Interp path already compares via `InterpValue`
// equality.
// test-mode: run-pass
// exit-code: 15
// compile-flags: --with-stdlib
fn main() -> i32 {
    let a = String::from("x");
    let b = String::from("x");
    let c = String::from("y");
    let mut score = 0;
    if a == b { score = score + 1; }           // +1: String == String (equal)
    if a != c { score = score + 2; }           // +2: String != String (diff)
    if a == "x" { score = score + 4; }         // +4: String == &str (equal)
    if "x" == a { score = score + 8; }         // +8: &str == String (equal)
    score
}
