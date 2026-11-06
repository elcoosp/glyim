// KNOWN_GAPS §3.9 (this session): indexing through a slice struct field
// (`b.data[i]`) ICEd — the MIR builder's Index auto-deref decided on the
// base *local's declared* type (the struct), not the type at the place
// (`&[i32]` after `Field(0)`), so no `Deref` projection was emitted and
// `Place::ty` saw `Index` over a `Ref` (fatal: "Index projection on
// non-array/slice type"). Covers the mutable variant: writes through
// `&'a mut [i32]` fields must land in the original array.
// test-mode: run-pass
// exit-code: 10
struct Buf<'a> { data: &'a mut [i32] }
fn main() -> i32 {
    let mut arr = [1, 2, 3];
    let mut b = Buf { data: &mut arr };
    b.data[1] = 9;
    b.data[0] + b.data[1]
}
