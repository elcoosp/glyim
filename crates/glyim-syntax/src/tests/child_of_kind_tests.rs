//! Tests for `child_of_kind` utility

use crate::{GlyimLang, SyntaxKind, SyntaxNode, child_of_kind};
use rowan::GreenNodeBuilder;
use rowan::Language;

fn build_test_tree() -> SyntaxNode {
    let mut builder = GreenNodeBuilder::new();
    builder.start_node(GlyimLang::kind_to_raw(SyntaxKind::Block));
    builder.start_node(GlyimLang::kind_to_raw(SyntaxKind::LetStmt));
    builder.token(GlyimLang::kind_to_raw(SyntaxKind::KwLet), "let");
    builder.token(GlyimLang::kind_to_raw(SyntaxKind::Ident), "x");
    builder.token(GlyimLang::kind_to_raw(SyntaxKind::Eq), "=");
    builder.start_node(GlyimLang::kind_to_raw(SyntaxKind::LitExpr));
    builder.token(GlyimLang::kind_to_raw(SyntaxKind::IntLit), "42");
    builder.finish_node();
    builder.token(GlyimLang::kind_to_raw(SyntaxKind::Semicolon), ";");
    builder.finish_node();
    builder.start_node(GlyimLang::kind_to_raw(SyntaxKind::ReturnExpr));
    builder.token(GlyimLang::kind_to_raw(SyntaxKind::KwReturn), "return");
    builder.token(GlyimLang::kind_to_raw(SyntaxKind::IntLit), "0");
    builder.finish_node();
    builder.finish_node();
    let green = builder.finish();
    SyntaxNode::new_root(green)
}

#[test]
fn child_of_kind_finds_first_child() {
    let root = build_test_tree();
    let child = child_of_kind(&root, SyntaxKind::LetStmt);
    assert!(child.is_some());
    assert_eq!(child.unwrap().kind(), SyntaxKind::LetStmt);
}

#[test]
fn child_of_kind_returns_none_if_not_found() {
    let root = build_test_tree();
    let child = child_of_kind(&root, SyntaxKind::FnDef);
    assert!(child.is_none());
}

#[test]
fn child_of_kind_finds_subsequent_sibling() {
    let root = build_test_tree();
    let child = child_of_kind(&root, SyntaxKind::ReturnExpr);
    assert!(child.is_some());
    assert_eq!(child.unwrap().kind(), SyntaxKind::ReturnExpr);
}

#[test]
fn child_of_kind_does_not_descend_into_grandchildren() {
    let root = build_test_tree();
    let child = child_of_kind(&root, SyntaxKind::LitExpr);
    assert!(child.is_none());
}
