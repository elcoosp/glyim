//@ mode: compile-fail
// T-ICE-5 regression: a const index of 2^64 + 1 must be reported as
// out of bounds, not silently aliased to element 1 by a usize truncation.
//@ feature: LANG-SYN-CONST-ITEM, COMPILER-INTERP-CONST-EVAL, TOOL-TEST-COMPILE-FAIL
//@ backends: gam
const BAD: i32 = [10, 20, 30][18446744073709551617]; //~ ERROR index out of bounds

fn main() {}
