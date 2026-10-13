// test-mode: no-ice
// A deeply nested array type. The type parser's recursion guard (or the
// expression parser's, if the type is ever lowered through it) must
// produce a diagnostic instead of overflowing the stack.
fn main() {
    let _x: [[[[[[[[[[[[[[[[[[[[[[[[[[[[[[u8; 1]; 1]; 1]; 1]; 1]; 1]; 1]; 1]; 1]; 1]; 1]; 1]; 1]; 1]; 1]; 1]; 1]; 1]; 1]; 1]; 1]; 1]; 1]; 1]; 1]; 1]; 1]; 1]; 1]; 1];
}
