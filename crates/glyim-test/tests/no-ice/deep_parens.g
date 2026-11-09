
// A deeply nested parenthesized expression. The parser's recursion guard
// must produce a diagnostic instead of overflowing the stack (FE-6/FE-111).
//@ feature: LANG-SYN-OPERATORS, TOOL-TEST-NO-ICE
//@ backends: gam
fn main() {
    let _x = ((((((((((((((((((((((((((((((((((((((((((((((1))))))))))))))))))))))))))))))))))))))))))))))))));
}
