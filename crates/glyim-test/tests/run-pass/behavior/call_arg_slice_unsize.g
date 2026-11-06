// KNOWN_GAPS §3.9 (this session): the call-argument unsize coercion was
// missing. `&arr` (typed `&[i32; 3]`) flowed into the `&[i32]` ABI slot as a
// bare thin pointer — the callee's fat `{ ptr, len }` parameter then read the
// register holding the *next* argument as the length, so
// `fn take(x: &[i32]) -> i32 { x.len() }` on `take(&arr)` returned an
// unpredictable number (72 on this host). The direct-ref case
// (`let s: &[i32] = &arr;`) already worked; this fixture pins the
// through-a-call case.
// test-mode: run-pass
// exit-code: 3
fn take(x: &[i32]) -> i32 { x.len() as i32 }
fn main() -> i32 {
    let arr = [1, 2, 3];
    take(&arr)
}
