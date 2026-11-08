//! W1-C04: concat! and stringify! builtin macro tests

use crate::{BuiltinMacro, Expander, MacroDef, MacroKind};
use glyim_span::{ByteIdx, FileId, HygieneCtx, Span, SyntaxContext};
use glyim_syntax::{GlyimLang, SyntaxKind, SyntaxNode};
use rowan::Language;

fn dummy_span(file_id: FileId, lo: u32) -> Span {
    Span::new(
        file_id,
        ByteIdx::from_raw(lo),
        ByteIdx::from_raw(lo + 1),
        SyntaxContext::ROOT,
    )
}

fn register_builtin(expander: &mut Expander, name: &str, handler: BuiltinMacro) {
    let macro_def = MacroDef {
        name: expander.interner().intern(name),
        kind: MacroKind::Builtin {
            name: expander.interner().intern(name),
            handler,
        },
        span: Span::DUMMY,
    };
    expander.register_macro(macro_def);
}

/// Build a `SyntaxNode` argument for concat! containing mixed tokens.
/// E.g. concat!("a", 1, "+") produces tokens: `StringLit` "a", Comma, `IntLit` 1, Comma, `StringLit` "+"
fn build_concat_args_mixed() -> SyntaxNode {
    let mut builder = rowan::GreenNodeBuilder::new();
    builder.start_node(GlyimLang::kind_to_raw(SyntaxKind::TokenTree));
    builder.token(GlyimLang::kind_to_raw(SyntaxKind::LParen), "(");
    builder.token(GlyimLang::kind_to_raw(SyntaxKind::StringLit), "\"a\"");
    builder.token(GlyimLang::kind_to_raw(SyntaxKind::Comma), ",");
    builder.token(GlyimLang::kind_to_raw(SyntaxKind::IntLit), "1");
    builder.token(GlyimLang::kind_to_raw(SyntaxKind::Comma), ",");
    builder.token(GlyimLang::kind_to_raw(SyntaxKind::StringLit), "\"+\"");
    builder.token(GlyimLang::kind_to_raw(SyntaxKind::RParen), ")");
    builder.finish_node();
    SyntaxNode::new_root(builder.finish())
}

/// Build a `SyntaxNode` argument for stringify!(foo(bar))
/// Produces flat tokens: (, foo, (, bar, ), )
fn build_stringify_foo_bar() -> SyntaxNode {
    let mut builder = rowan::GreenNodeBuilder::new();
    builder.start_node(GlyimLang::kind_to_raw(SyntaxKind::TokenTree));
    builder.token(GlyimLang::kind_to_raw(SyntaxKind::LParen), "(");
    builder.token(GlyimLang::kind_to_raw(SyntaxKind::Ident), "foo");
    builder.token(GlyimLang::kind_to_raw(SyntaxKind::LParen), "(");
    builder.token(GlyimLang::kind_to_raw(SyntaxKind::Ident), "bar");
    builder.token(GlyimLang::kind_to_raw(SyntaxKind::RParen), ")");
    builder.token(GlyimLang::kind_to_raw(SyntaxKind::RParen), ")");
    builder.finish_node();
    SyntaxNode::new_root(builder.finish())
}

/// Build a `SyntaxNode` argument for stringify!(concat!("a", "b"))
/// Produces flat tokens: (, concat, !, (, "a", ,, "b", ), )
fn build_stringify_concat_ab() -> SyntaxNode {
    let mut builder = rowan::GreenNodeBuilder::new();
    builder.start_node(GlyimLang::kind_to_raw(SyntaxKind::TokenTree));
    builder.token(GlyimLang::kind_to_raw(SyntaxKind::LParen), "(");
    builder.token(GlyimLang::kind_to_raw(SyntaxKind::Ident), "concat");
    builder.token(GlyimLang::kind_to_raw(SyntaxKind::Bang), "!");
    builder.token(GlyimLang::kind_to_raw(SyntaxKind::LParen), "(");
    builder.token(GlyimLang::kind_to_raw(SyntaxKind::StringLit), "\"a\"");
    builder.token(GlyimLang::kind_to_raw(SyntaxKind::Comma), ",");
    builder.token(GlyimLang::kind_to_raw(SyntaxKind::StringLit), "\"b\"");
    builder.token(GlyimLang::kind_to_raw(SyntaxKind::RParen), ")");
    builder.token(GlyimLang::kind_to_raw(SyntaxKind::RParen), ")");
    builder.finish_node();
    SyntaxNode::new_root(builder.finish())
}

/// W1-C04-T01: concat!("a", 1, "+") → "a1+"
#[test]
fn concat_mixed_args() {
    let mut hygiene = HygieneCtx::default();
    let mut expander = Expander::new(&mut hygiene);
    register_builtin(&mut expander, "concat", BuiltinMacro::Concat);

    let span = dummy_span(FileId::BOGUS, 0);
    let args = build_concat_args_mixed();
    let name = expander.interner().intern("concat");

    let result = expander.expand(name, &args, span);
    assert!(
        result.diagnostics.is_empty(),
        "Unexpected diagnostics: {:?}",
        result.diagnostics
    );
    let expanded = result.expanded.expect("Expected expansion");
    let text = expanded.text().to_string();
    assert!(
        text.contains("\"a1+\""),
        "Expected concat!(\"a\", 1, \"+\") to produce \"a1+\", got: {text}"
    );
}

/// W1-C04-T02: stringify!(foo(bar)) → "foo ( bar )"
#[test]
fn stringify_foo_bar() {
    let mut hygiene = HygieneCtx::default();
    let mut expander = Expander::new(&mut hygiene);
    register_builtin(&mut expander, "stringify", BuiltinMacro::Stringify);

    let span = dummy_span(FileId::BOGUS, 0);
    let args = build_stringify_foo_bar();
    let name = expander.interner().intern("stringify");

    let result = expander.expand(name, &args, span);
    assert!(
        result.diagnostics.is_empty(),
        "Unexpected diagnostics: {:?}",
        result.diagnostics
    );
    let expanded = result.expanded.expect("Expected expansion");
    let text = expanded.text().to_string();
    assert!(
        text.contains("foo (bar)"),
        "Expected stringify!(foo(bar)) to contain 'foo (bar)', got: {text}"
    );
}

/// W1-C04-T03: stringify!(concat!("a", "b")) → "concat ! ( \"a\" , \"b\" )"
#[test]
fn stringify_concat_ab() {
    let mut hygiene = HygieneCtx::default();
    let mut expander = Expander::new(&mut hygiene);
    register_builtin(&mut expander, "stringify", BuiltinMacro::Stringify);

    let span = dummy_span(FileId::BOGUS, 0);
    let args = build_stringify_concat_ab();
    let name = expander.interner().intern("stringify");

    let result = expander.expand(name, &args, span);
    assert!(
        result.diagnostics.is_empty(),
        "Unexpected diagnostics: {:?}",
        result.diagnostics
    );
    let expanded = result.expanded.expect("Expected expansion");
    let text = expanded.text().to_string();
    // The expanded StringLit token contains escaped inner quotes: \"a\" and \"b\"
    assert!(
        text.contains("concat"),
        "Expected stringify!(concat!(\"a\", \"b\")) to contain 'concat'; got: {text}"
    );
    // Tier 4.4 correction: the old buggy spacing inserted spaces around the
    // quoted args (`concat ! ( "a" , "b" )`). The fixed output must not.
    assert!(
        !text.contains("\"a\" ,") && !text.contains(", \"b\""),
        "stringify! must not insert space around quoted args' commas; got: {text}"
    );
}
