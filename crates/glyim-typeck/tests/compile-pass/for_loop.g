//@ mode: compile-pass
//@ feature: LANG-FOR-SLICE-ITER, TOOL-TEST-COMPILE-PASS
//@ backends: gam
fn main() {
    let arr = [1, 2, 3];
    let mut sum = 0;
    for x in arr {
        sum += x;
    }
}
