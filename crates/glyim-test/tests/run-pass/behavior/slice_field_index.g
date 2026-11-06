// KNOWN_GAPS §3.9 (this session): struct-literal fields never applied the
// array→slice unsize coercion. `buf: &'a [i32]` initialized from `&arr`
// stored the thin `&[i32; 3]` into the 16-byte fat-pointer field slot, and
// `s.buf.len()` read the adjacent `n: usize` bytes as the length (0, 64, 16
// — whatever followed the pointer). The aggregate-construction path now
// materializes `{ ptr, len }` per field, keyed on the declared field type.
// test-mode: run-pass
// exit-code: 4
struct Buf<'a> { data: &'a [i32] }
fn main() -> i32 {
    let arr = [1, 2, 3];
    let b = Buf { data: &arr };
    (b.data[0] + b.data[2]) as i32
}
