// §3.5.30 attribute resolution: `#[allow(warnings)]` (blanket group
// suppression) suppresses the unsafe-fn-call warning without naming the
// lint. Matches rustc's short form.
//@ mode: compile-pass
//@ forbid-warning: call to unsafe function
//@ feature: LANG-SYN-UNSAFE-BLOCK, LANG-SCOPE-ATTR-ALLOW, LANG-SYN-FN-DEF, TOOL-TEST-COMPILE-PASS
//@ backends: gam
unsafe fn get_value() -> i32 {
    42
}

#[allow(warnings)]
fn main() -> i32 {
    get_value()
}
