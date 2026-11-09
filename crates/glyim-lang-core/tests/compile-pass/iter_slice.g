//@ mode: compile-pass
// V16-T02: Iterate over slice using for loop (requires IntoIterator)
//@ feature: LANG-TY-SLICE, STD-CORE-SLICE, STD-CORE-ITER, TOOL-TEST-COMPILE-PASS
//@ backends: gam

fn main() {
    let data: [i32; 3] = [1, 2, 3];
    let mut sum = 0;
    for item in &data {
        sum += *item;
    }
    assert_eq!(sum, 6);
}
