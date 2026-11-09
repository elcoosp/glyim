// A `mod foo;` whose file does not exist must produce a real diagnostic
// with a location, not an ICE.
//@ mode: compile-fail
//@ feature: LANG-SCOPE-MOD-DECL, COMPILER-FRONT-DEFTABLE, TOOL-TEST-COMPILE-FAIL
//@ backends: gam
mod does_not_exist; //~ ERROR cannot find module file
fn main() {}
