// KNOWN_GAPS §3.9 (this session): tuple literals are the third
// aggregate-construction site needing the array→slice unsize coercion — a
// tuple field declared `&[T]` receiving a `&[T; N]` operand must
// materialize the fat `{ ptr, len }` value.
// test-mode: run-pass
// exit-code: 3
fn main() -> i32 {
    let arr = [1, 2, 3];
    let t: (&[i32], usize) = (&arr, 0);
    (t.0.len() as i32) + (t.1 as i32)
}
