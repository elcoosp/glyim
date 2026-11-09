//@ mode: compile-pass
// V17: Vec::extend_from_slice works
//@ feature: LANG-BUILTIN-VEC, STD-ALLOC-VEC, TOOL-TEST-COMPILE-PASS
//@ backends: gam

fn main() {
    let mut v: Vec<i32> = Vec::new();
    v.push(1);
    v.extend_from_slice(&[2, 3, 4, 5]);
    assert_eq!(v.len(), 5);
    assert_eq!(v[0], 1);
    assert_eq!(v[4], 5);
}
