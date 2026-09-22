// A `mod foo;` whose file does not exist must produce a real diagnostic
// with a location, not an ICE.
// test-mode: compile-fail
mod does_not_exist; //~ ERROR cannot find module file
fn main() {}
