// `Vec<T>` indexing. Typeck accepts it (`get_index_info` special-cases AdtId
// 1020), but neither `Place::ty` nor codegen had a Vec arm — the former
// returned `ERROR`, the latter rejected the projection outright with an
// internal error. A Vec header is `{[3 x i64]}` (data ptr, capacity, len),
// so indexing reads word 0 and GEPs by the index.
// test-mode: run-pass
// exit-code: 50
// compile-flags: --with-stdlib
fn main() -> i32 {
    let mut v = Vec::new();
    v.push(10);
    v.push(20);
    v.push(30);
    v[1] + v[2]
}
