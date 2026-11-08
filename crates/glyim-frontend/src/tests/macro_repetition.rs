use glyim_test::snapshot_cst;

#[test]
fn test_macro_repetition_star() {
    let source = r"
macro_rules! vec {
    ( $( $x:expr ),* ) => {
        { let mut v = Vec::new(); $( v.push($x); )* v }
    };
}
";
    snapshot_cst("macro_repetition_star", source);
}

#[test]
fn test_macro_repetition_plus() {
    let source = r"
macro_rules! repeat {
    ( $( $x:expr ),+ ) => { ... };
}
";
    snapshot_cst("macro_repetition_plus", source);
}

#[test]
fn test_macro_repetition_with_sep_and_op() {
    let source = r"
macro_rules! test {
    ( $($a:ident),* => $($b:expr);* ) => {};
}
";
    snapshot_cst("macro_repetition_sep_op", source);
}
