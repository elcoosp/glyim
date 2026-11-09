// ignore - requires full typeck/frontend (S03)
//@ feature: LANG-MATCH-GUARDS, TOOL-TEST-COMPILE-PASS
//@ backends: gam
fn main(x: i32) -> bool {
    match x {
        0 | 1 => true,
        _ => false
    }
}
