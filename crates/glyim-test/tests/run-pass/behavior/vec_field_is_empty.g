// `w.v.is_empty()` — same shape as vec_field_len; exercises the shared
// `len | is_empty` codegen arm and the interp Ref-projection path.
// test-mode: run-pass
// exit-code: 1
// compile-flags: --with-stdlib
struct W { v: Vec<i32> }
fn main() -> i32 {
    let w = W { v: [].to_vec() };
    if w.v.is_empty() { 1 } else { 0 }
}
