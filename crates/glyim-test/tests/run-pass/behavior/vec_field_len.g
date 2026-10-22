// `w.v.len()` — `Vec<T>` behind a struct field. The MIR call passes the
// receiver as `&mut Vec<T>`. Codegen's fat-pointer `len` arm now loads
// through the pointer (see commit 683948e3). The interpreter stores the
// `Ref`'s projection (see KNOWN_GAPS §8) so the `&mut _5.field0` ref
// resolves to the Vec field, not the whole struct.
// test-mode: run-pass
// exit-code: 3
// compile-flags: --with-stdlib
struct W { v: Vec<i32> }
fn main() -> i32 {
    let w = W { v: [1, 2, 3].to_vec() };
    w.v.len() as i32
}
