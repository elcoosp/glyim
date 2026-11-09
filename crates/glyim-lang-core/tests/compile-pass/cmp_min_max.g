//@ mode: compile-pass
// V16-T05: cmp::min, cmp::max
//@ feature: STD-CORE-CMP, TOOL-TEST-COMPILE-PASS
//@ backends: gam

fn main() {
    let a = 5;
    let b = 10;
    assert_eq!(cmp::min(a, b), 5);
    assert_eq!(cmp::max(a, b), 10);
    assert_eq!(cmp::min(b, a), 5);
    assert_eq!(cmp::max(b, a), 10);
}
