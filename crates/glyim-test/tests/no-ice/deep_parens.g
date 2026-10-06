// test-mode: no-ice
// A deeply nested parenthesized expression. The parser's recursion guard
// must produce a diagnostic instead of overflowing the stack (FE-6/FE-111).
fn main() {
    let _x = ((((((((((((((((((((((((((((((((((((((((((((((1))))))))))))))))))))))))))))))))))))))))))))))))));
}
