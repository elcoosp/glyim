//@ mode: compile-pass
// V16-T04: mem::replace, mem::swap
//@ feature: COMPILER-LAYOUT-SIZE-ALIGN, STD-CORE-MEM, TOOL-TEST-COMPILE-PASS
//@ backends: gam

fn main() {
    let mut a = 10;
    let mut b = 20;
    let old = mem::replace(&mut a, 99);
    assert_eq!(old, 10);
    assert_eq!(a, 99);
    mem::swap(&mut a, &mut b);
    assert_eq!(a, 20);
    assert_eq!(b, 99);
}
