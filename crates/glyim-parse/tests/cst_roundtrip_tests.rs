use glyim_parse::ast_to_cst;
use glyim_parse::parse;
use glyim_syntax::SyntaxKind;

#[test]
fn cst_roundtrip_int_literal() {
    let out = parse("main = () => 42");
    assert!(out.errors.is_empty());
    let cst = ast_to_cst::ast_to_cst(&out.ast);
    assert_eq!(cst.kind(), SyntaxKind::SourceFile);
    assert!(cst.children().count() > 0);
}

#[test]
fn cst_roundtrip_let_binding() {
    let out = parse("let x = 42\nmain = () => x");
    assert!(out.errors.is_empty());
    let cst = ast_to_cst::ast_to_cst(&out.ast);
    assert_eq!(cst.kind(), SyntaxKind::SourceFile);
}

#[test]
fn cst_roundtrip_fn_def() {
    let out = parse("fn add(a, b) { a + b }\nmain = () => add(1, 2)");
    assert!(out.errors.is_empty());
    let cst = ast_to_cst::ast_to_cst(&out.ast);
    assert!(cst.children().count() > 0);
}

#[test]
fn cst_roundtrip_struct_and_enum() {
    let out = parse("struct Point { x, y }\nenum Color { Red, Green }\nmain = () => 1");
    assert!(out.errors.is_empty());
    let cst = ast_to_cst::ast_to_cst(&out.ast);
    assert!(cst.children().count() > 0);
}

#[test]
fn cst_roundtrip_if_else() {
    let out = parse("main = () => if 1 { 10 } else { 20 }");
    assert!(out.errors.is_empty());
    let cst = ast_to_cst::ast_to_cst(&out.ast);
    assert!(cst.children().count() > 0);
}

#[test]
fn cst_roundtrip_match() {
    let out = parse("main = () => match 1 { 1 => 10, _ => 20 }");
    assert!(out.errors.is_empty());
    let cst = ast_to_cst::ast_to_cst(&out.ast);
    assert!(cst.children().count() > 0);
}
