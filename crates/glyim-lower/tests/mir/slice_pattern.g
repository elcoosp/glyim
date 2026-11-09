//@ ignore
// requires full typeck/frontend (S03): array-length inference in slice patterns
//@ feature: LANG-MATCH-GUARDS, LANG-TY-SLICE, STD-CORE-SLICE, TOOL-TEST-COMPILE-PASS
//@ backends: gam
fn main(arr: [i32; 3]) -> i32 {
    match arr {
        [a, b] => a + b,
        _ => 0
    }
}
