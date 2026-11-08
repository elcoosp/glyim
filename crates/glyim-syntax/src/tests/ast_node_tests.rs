//! Tests for `AstNode` `can_cast` and cast

use crate::{
    AstNode, BinaryExpr, Block, CallExpr, EnumDef, FnDef, GlyimLang, ImplDef, LitExpr, PathExpr,
    SourceFile, StructDef, SyntaxKind, SyntaxNode, TraitDef,
};
use rowan::GreenNodeBuilder;
use rowan::Language;

fn build_syntax_node(kind: SyntaxKind) -> SyntaxNode {
    let mut builder = GreenNodeBuilder::new();
    builder.start_node(GlyimLang::kind_to_raw(kind));
    builder.finish_node();
    let green = builder.finish();
    SyntaxNode::new_root(green)
}

macro_rules! test_ast_node {
    ($node_type:ident, $kind:expr) => {
        let node_type_name = stringify!($node_type);
        let node = build_syntax_node($kind);
        assert!(
            $node_type::can_cast($kind),
            "can_cast should return true for {}",
            node_type_name
        );
        let casted = $node_type::cast(node.clone());
        assert!(
            casted.is_some(),
            "cast should succeed for {}",
            node_type_name
        );
        assert_eq!(
            casted.unwrap().syntax(),
            &node,
            "syntax should return the original node"
        );

        let other_kind = if $kind == SyntaxKind::SourceFile {
            SyntaxKind::FnDef
        } else {
            SyntaxKind::SourceFile
        };
        assert!(
            !$node_type::can_cast(other_kind),
            "can_cast should be false for unrelated kind on {}",
            node_type_name
        );
        let other_node = build_syntax_node(other_kind);
        assert!(
            $node_type::cast(other_node).is_none(),
            "cast should return None for unrelated kind on {}",
            node_type_name
        );
    };
}

#[test]
fn source_file_ast_node() {
    test_ast_node!(SourceFile, SyntaxKind::SourceFile);
}

#[test]
fn fn_def_ast_node() {
    test_ast_node!(FnDef, SyntaxKind::FnDef);
}

#[test]
fn struct_def_ast_node() {
    test_ast_node!(StructDef, SyntaxKind::StructDef);
}

#[test]
fn enum_def_ast_node() {
    test_ast_node!(EnumDef, SyntaxKind::EnumDef);
}

#[test]
fn trait_def_ast_node() {
    test_ast_node!(TraitDef, SyntaxKind::TraitDef);
}

#[test]
fn impl_def_ast_node() {
    test_ast_node!(ImplDef, SyntaxKind::ImplDef);
}

#[test]
fn block_ast_node() {
    test_ast_node!(Block, SyntaxKind::Block);
}

#[test]
fn call_expr_ast_node() {
    test_ast_node!(CallExpr, SyntaxKind::CallExpr);
}

#[test]
fn binary_expr_ast_node() {
    test_ast_node!(BinaryExpr, SyntaxKind::BinaryExpr);
}

#[test]
fn path_expr_ast_node() {
    test_ast_node!(PathExpr, SyntaxKind::PathExpr);
}

#[test]
fn lit_expr_ast_node() {
    test_ast_node!(LitExpr, SyntaxKind::LitExpr);
}
