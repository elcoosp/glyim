// ignore - requires full typeck/frontend (S03)
//@ feature: COMPILER-FRONT-HIR, TOOL-TEST-COMPILE-PASS
//@ backends: gam
fn main() -> i32 {
    let arr = [10, 20, 30];
    arr[1]
}
