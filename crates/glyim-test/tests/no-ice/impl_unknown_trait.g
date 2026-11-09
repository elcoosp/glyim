//@ feature: LANG-SYN-IMPL, LANG-TRAIT-IMPL, TOOL-TEST-NO-ICE
//@ backends: gam
struct S;
impl Missing for S {}
fn main() {}
