
// A deeply nested array type. The type parser's recursion guard (or the
// expression parser's, if the type is ever lowered through it) must
// produce a diagnostic instead of overflowing the stack.
//@ feature: LANG-TY-ARRAY, TOOL-TEST-NO-ICE
//@ backends: gam
fn main() {
    let _x: [[[[[[[[[[[[[[[[[[[[[[[[[[[[[[u8; 1]; 1]; 1]; 1]; 1]; 1]; 1]; 1]; 1]; 1]; 1]; 1]; 1]; 1]; 1]; 1]; 1]; 1]; 1]; 1]; 1]; 1]; 1]; 1]; 1]; 1]; 1]; 1]; 1]; 1];
}
