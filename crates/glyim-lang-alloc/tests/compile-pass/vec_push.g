//@ mode: compile-pass
// V17-T01: Vec::push and indexing works
//@ feature: LANG-BUILTIN-VEC, STD-ALLOC-VEC, STD-ALLOC-STRING, TOOL-TEST-COMPILE-PASS
//@ backends: gam

fn main() {
    let mut v: Vec<i32> = Vec::new();
    v.push(10);
    v.push(20);
    v.push(30);
    assert_eq!(v[0], 10);
    assert_eq!(v[1], 20);
    assert_eq!(v[2], 30);
}
