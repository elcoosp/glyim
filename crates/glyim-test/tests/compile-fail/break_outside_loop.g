//@ mode: compile-fail
//@ feature: LANG-SYN-LOOP-BREAK, TOOL-TEST-COMPILE-FAIL
//@ backends: gam
fn main() {
    break; //~ ERROR break outside of loop
}
