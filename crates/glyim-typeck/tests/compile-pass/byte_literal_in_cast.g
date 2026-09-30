// Regression: a byte literal inside a cast expression (`b'0' as i32`) used to
// be silently dropped from the HIR. `LitExpr` lowering filters its leaf token
// through `SyntaxKind::is_literal()`, which omitted `ByteLit`, so `lower_lit_expr`
// returned `None`; the enclosing `let` was then discarded by the `?` operator
// and the *next* statement reported `[T0001] unresolved name` for the binding
// the dropped `let` was supposed to introduce.
//
// The minimal shape that used to fail was any `let NAME: i32 = (..) - (b'0' as i32);`
// followed by a use of `NAME`.
fn digit_value(ch: u8) -> i32 {
    let digit: i32 = (ch as i32) - (b'0' as i32);
    digit
}

fn main() {
    let d = digit_value(b'7');
    let _ = d;
}
