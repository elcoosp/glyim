// `Vec<T>: Deref<Target = [T]>`. Without the impl, `(&v)[0]` and any other
// slice method reached through a `&Vec` failed; `v.as_slice()[0]` worked
// only because `as_slice` is inherent.
// test-mode: run-pass
// exit-code: 7
// compile-flags: --with-stdlib
fn main() -> i32 {
    let mut v = Vec::new();
    v.push(3);
    v.push(4);
    let s = &v;
    s[0] + s[1]
}
