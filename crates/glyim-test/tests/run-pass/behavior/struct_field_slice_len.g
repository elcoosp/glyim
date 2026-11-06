// KNOWN_GAPS §3.9 (this session): a struct field of slice type
// (`buf: &'a [i32]`) initialized from an array-literal reference
// (`&arr`, typed `&[i32; 3]`) must undergo the same array-to-slice
// unsizing that call arguments get. The aggregate-construction path
// previously stored the thin `&[i32; 3]` into the fat `&[i32]` field
// slot, so `s.buf.len()` read the adjacent struct bytes as the length
// (0, 64, 16 — whatever followed the pointer).
// test-mode: run-pass
// exit-code: 3
fn main() -> i32 {
    let arr = [1, 2, 3];
    let s = S { buf: &arr, n: 0 };
    s.buf.len() as i32
}
struct S<'a> { buf: &'a [i32], n: usize }
