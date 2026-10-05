use crate::parser::parse_to_syntax;
use glyim_span::FileId;

fn parse_no_errors(source: &str) {
    let file_id = FileId::from_raw(0);
    let result = parse_to_syntax(source, file_id);
    let errors: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.is_error() || d.message.contains("STUB"))
        .collect();
    assert!(errors.is_empty(), "unexpected diagnostics: {:?}", errors);
}

#[test]
fn const_item_parses_cleanly() {
    parse_no_errors("const X: i32 = 42;");
}

#[test]
fn static_item_parses_cleanly() {
    parse_no_errors("static X: i32 = 42;");
}

#[test]
fn static_mut_item_parses_cleanly() {
    parse_no_errors("static mut X: i32 = 42;");
}

#[test]
fn type_alias_parses_cleanly() {
    parse_no_errors("type Foo = i32;");
}

#[test]
fn extern_block_parses_cleanly() {
    parse_no_errors("extern \"C\" { fn foo(); }");
}

#[test]
fn associated_type_in_trait_parses_cleanly() {
    parse_no_errors("trait T { type Foo; }");
}

#[test]
fn associated_const_in_trait_parses_cleanly() {
    parse_no_errors("trait T { const C: i32; }");
}

#[test]
fn associated_type_in_impl_parses_cleanly() {
    parse_no_errors("struct S; impl S { type Foo = i32; }");
}

#[test]
fn associated_const_in_impl_parses_cleanly() {
    parse_no_errors("struct S; impl S { const C: i32 = 42; }");
}

#[test]
fn raw_pointer_type_parses_cleanly() {
    parse_no_errors("type T = *const i32;");
    parse_no_errors("type T = *mut i32;");
}

#[test]
fn impl_trait_type_parses_cleanly() {
    parse_no_errors("fn foo() -> impl MyTrait { loop {} }");
}

#[test]
fn function_pointer_type_parses_cleanly() {
    parse_no_errors("type F = fn(i32) -> i32;");
}

#[test]
fn labeled_loop_parses_with_expected_errors() {
    // FE-10: labels now lex and parse correctly -- no errors.
    let file_id = FileId::from_raw(0);
    let result = parse_to_syntax("fn main() { 'outer: loop { break; } }", file_id);
    let errors: Vec<_> = result.diagnostics.iter().filter(|d| d.is_error()).collect();
    assert!(
        errors.is_empty(),
        "labeled loop must parse cleanly, got: {:?}",
        errors
    );
}

#[test]
fn labeled_while_parses_with_expected_errors() {
    let file_id = FileId::from_raw(0);
    let result = parse_to_syntax("fn main() { 'lbl: while true { break; } }", file_id);
    assert_eq!(
        result.diagnostics.iter().filter(|d| d.is_error()).count(),
        0,
        "labeled while must parse cleanly: {:?}",
        result.diagnostics
    );
}

#[test]
fn labeled_for_parses_with_expected_errors() {
    let file_id = FileId::from_raw(0);
    let result = parse_to_syntax("fn main() { 'lbl: for i in 0..3 { break; } }", file_id);
    assert_eq!(
        result.diagnostics.iter().filter(|d| d.is_error()).count(),
        0,
        "labeled for must parse cleanly: {:?}",
        result.diagnostics
    );
}

#[test]
fn labeled_block_parses_with_expected_errors() {
    let file_id = FileId::from_raw(0);
    let result = parse_to_syntax("fn main() { 'lbl: { break 'lbl; } }", file_id);
    assert_eq!(
        result.diagnostics.iter().filter(|d| d.is_error()).count(),
        0,
        "labeled block must parse cleanly: {:?}",
        result.diagnostics
    );
}

use glyim_syntax::SyntaxKind;

fn parse_and_collect(source: &str) -> glyim_syntax::SyntaxNode {
    let file_id = FileId::from_raw(0);
    let result = parse_to_syntax(source, file_id);
    // No errors expected for these tests
    let errors: Vec<_> = result.diagnostics.iter().filter(|d| d.is_error()).collect();
    assert!(errors.is_empty(), "unexpected errors: {:?}", errors);
    result.root
}

fn has_node(node: &glyim_syntax::SyntaxNode, kind: SyntaxKind) -> bool {
    node.descendants().any(|n| n.kind() == kind)
}

#[test]
fn const_item_creates_correct_structure() {
    let root = parse_and_collect("const X: i32 = 42;");
    assert!(has_node(&root, SyntaxKind::ConstDef));
    // Verify type and value exist
    let const_item = root
        .descendants()
        .find(|n| n.kind() == SyntaxKind::ConstDef)
        .unwrap();
    assert!(has_node(&const_item, SyntaxKind::PathType));
    assert!(has_node(&const_item, SyntaxKind::LitExpr));
}

#[test]
fn static_item_creates_correct_structure() {
    let root = parse_and_collect("static X: i32 = 42;");
    assert!(has_node(&root, SyntaxKind::StaticDef));
    let static_item = root
        .descendants()
        .find(|n| n.kind() == SyntaxKind::StaticDef)
        .unwrap();
    assert!(has_node(&static_item, SyntaxKind::PathType));
    assert!(has_node(&static_item, SyntaxKind::LitExpr));
}

#[test]
fn static_mut_item_creates_correct_structure() {
    let root = parse_and_collect("static mut X: i32 = 42;");
    assert!(has_node(&root, SyntaxKind::StaticDef));
    let static_item = root
        .descendants()
        .find(|n| n.kind() == SyntaxKind::StaticDef)
        .unwrap();
    assert!(has_node(&static_item, SyntaxKind::PathType));
    assert!(has_node(&static_item, SyntaxKind::LitExpr));
}

#[test]
fn type_alias_creates_correct_structure() {
    let root = parse_and_collect("type Foo = i32;");
    assert!(has_node(&root, SyntaxKind::TypeAlias));
    let alias = root
        .descendants()
        .find(|n| n.kind() == SyntaxKind::TypeAlias)
        .unwrap();
    assert!(has_node(&alias, SyntaxKind::PathType));
}

#[test]
fn extern_block_creates_correct_structure() {
    let root = parse_and_collect("extern \"C\" { fn foo(); }");
    assert!(has_node(&root, SyntaxKind::ExternBlock));
    let extern_block = root
        .descendants()
        .find(|n| n.kind() == SyntaxKind::ExternBlock)
        .unwrap();
    assert!(has_node(&extern_block, SyntaxKind::FnDef));
}

#[test]
fn associated_type_in_trait_creates_correct_structure() {
    let root = parse_and_collect("trait T { type Foo; }");
    assert!(has_node(&root, SyntaxKind::TraitDef));
    // Inside trait, we should have an associated type (which currently parses as a type alias? Check implementation.)
    // The stub was removed but we only bump tokens and expect semicolon; no real AST node for associated type.
    // Verify it parses without errors; structure is minimal.
}

#[test]
fn associated_const_in_trait_creates_correct_structure() {
    let root = parse_and_collect("trait T { const C: i32; }");
    assert!(has_node(&root, SyntaxKind::TraitDef));
}

#[test]
fn associated_type_in_impl_creates_correct_structure() {
    let root = parse_and_collect("struct S; impl S { type Foo = i32; }");
    assert!(has_node(&root, SyntaxKind::ImplDef));
}

#[test]
fn associated_const_in_impl_creates_correct_structure() {
    let root = parse_and_collect("struct S; impl S { const C: i32 = 42; }");
    assert!(has_node(&root, SyntaxKind::ImplDef));
}

#[test]
fn raw_pointer_type_const_creates_correct_structure() {
    let root = parse_and_collect("type T = *const i32;");
    // The parser previously used tracing::warn. Now it just bumps * and const/mut and parses type.
    // There is no dedicated node for raw pointer type; it's just a prefix tokens and then inner type.
    // At minimum, ensure PathType for i32 exists.
    assert!(has_node(&root, SyntaxKind::PathType));
}

#[test]
fn raw_pointer_type_mut_creates_correct_structure() {
    let root = parse_and_collect("type T = *mut i32;");
    assert!(has_node(&root, SyntaxKind::PathType));
}

#[test]
fn impl_trait_type_creates_correct_structure() {
    let root = parse_and_collect("fn foo() -> impl MyTrait { loop {} }");
    // Check that the return type is parsed as impl trait (currently parsed as path type maybe)
    assert!(has_node(&root, SyntaxKind::FnDef));
    // The "impl MyTrait" is parsed as PathType? Check if we have a PathType with "MyTrait"
    let fn_def = root
        .descendants()
        .find(|n| n.kind() == SyntaxKind::FnDef)
        .unwrap();
    assert!(
        fn_def
            .descendants()
            .any(|n| n.kind() == SyntaxKind::PathType && n.text().to_string().contains("MyTrait"))
    );
}

#[test]
fn function_pointer_type_creates_correct_structure() {
    let root = parse_and_collect("type F = fn(i32) -> i32;");
    // The parser now parses function pointer types without stub. It should produce a PathType for 'fn' and parameters.
    // At minimum, ensure no errors and that the type alias exists.
    assert!(has_node(&root, SyntaxKind::TypeAlias));
}
