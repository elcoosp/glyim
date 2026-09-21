use glyim_span::{ByteIdx, FileId, HygieneCtx, Span, SyntaxContext};
use glyim_syntax::{GlyimLang, SyntaxNode};
use rowan::Language;
use std::fs;
use tempfile::tempdir;

use crate::{BuiltinMacro, Expander, MacroDef, MacroKind};

// Helper to create a dummy span with given file id and byte offset
fn dummy_span(file_id: FileId, lo: u32) -> Span {
    Span::new(
        file_id,
        ByteIdx::from_raw(lo),
        ByteIdx::from_raw(lo + 1),
        SyntaxContext::ROOT,
    )
}

// Helper to create a simple token tree from a string literal
fn string_lit_token(s: &str) -> SyntaxNode {
    let mut builder = rowan::GreenNodeBuilder::new();
    builder.start_node(GlyimLang::kind_to_raw(glyim_syntax::SyntaxKind::TokenTree));
    builder.token(
        GlyimLang::kind_to_raw(glyim_syntax::SyntaxKind::StringLit),
        &format!("\"{}\"", s),
    );
    builder.finish_node();
    SyntaxNode::new_root(builder.finish())
}

// Helper to create an argument node for concat! that contains two string literals
fn concat_args(a: &str, b: &str) -> SyntaxNode {
    let mut builder = rowan::GreenNodeBuilder::new();
    builder.start_node(GlyimLang::kind_to_raw(glyim_syntax::SyntaxKind::TokenTree));
    // First string literal
    builder.start_node(GlyimLang::kind_to_raw(glyim_syntax::SyntaxKind::TokenTree));
    builder.token(
        GlyimLang::kind_to_raw(glyim_syntax::SyntaxKind::StringLit),
        &format!("\"{}\"", a),
    );
    builder.finish_node();
    // Comma punctuation
    builder.token(GlyimLang::kind_to_raw(glyim_syntax::SyntaxKind::Comma), ",");
    // Second string literal
    builder.start_node(GlyimLang::kind_to_raw(glyim_syntax::SyntaxKind::TokenTree));
    builder.token(
        GlyimLang::kind_to_raw(glyim_syntax::SyntaxKind::StringLit),
        &format!("\"{}\"", b),
    );
    builder.finish_node();
    builder.finish_node();
    SyntaxNode::new_root(builder.finish())
}

// Helper to create an argument node for stringify! containing an expression
fn stringify_args(expr_text: &str) -> SyntaxNode {
    let mut builder = rowan::GreenNodeBuilder::new();
    builder.start_node(GlyimLang::kind_to_raw(glyim_syntax::SyntaxKind::TokenTree));
    builder.token(
        GlyimLang::kind_to_raw(glyim_syntax::SyntaxKind::Ident),
        expr_text,
    );
    builder.finish_node();
    SyntaxNode::new_root(builder.finish())
}

// Helper to register a builtin macro in an expander
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

#[test]
fn test_file_macro() {
    let mut hygiene = HygieneCtx::default();
    let mut expander = Expander::new(&mut hygiene);
    register_builtin(&mut expander, "file", BuiltinMacro::File);

    let file_id = FileId::from_raw(42);
    let span = dummy_span(file_id, 10);
    let args = string_lit_token(""); // file! takes no arguments
    let name = expander.interner().intern("file");

    let result = expander.expand(name, &args, span);
    assert!(result.diagnostics.is_empty());
    let expanded = result.expanded.unwrap();
    let text = expanded.text().to_string();
    assert!(
        text.contains("file_42") || text.contains("42"),
        "Expected file ID 42 in expansion, got {}",
        text
    );
}

#[test]
fn test_line_macro() {
    let mut hygiene = HygieneCtx::default();
    let mut expander = Expander::new(&mut hygiene);
    register_builtin(&mut expander, "line", BuiltinMacro::Line);

    let span = dummy_span(FileId::BOGUS, 160); // line approx = 160/80 + 1 = 3
    let args = string_lit_token("");
    let name = expander.interner().intern("line");

    let result = expander.expand(name, &args, span);
    assert!(result.diagnostics.is_empty());
    let expanded = result.expanded.unwrap();
    let text = expanded.text().to_string();
    assert!(text.contains("3"), "Expected line 3, got {}", text);
}

#[test]
fn test_column_macro() {
    let mut hygiene = HygieneCtx::default();
    let mut expander = Expander::new(&mut hygiene);
    register_builtin(&mut expander, "column", BuiltinMacro::Column);

    let span = dummy_span(FileId::BOGUS, 42); // column approx = 42 % 80 + 1 = 43
    let args = string_lit_token("");
    let name = expander.interner().intern("column");

    let result = expander.expand(name, &args, span);
    assert!(result.diagnostics.is_empty());
    let expanded = result.expanded.unwrap();
    let text = expanded.text().to_string();
    assert!(text.contains("43"), "Expected column 43, got {}", text);
}

#[test]
fn test_env_macro() {
    unsafe {
        std::env::set_var("GLYIM_TEST_VAR", "hello_from_env");
    }
    let mut hygiene = HygieneCtx::default();
    let mut expander = Expander::new(&mut hygiene);
    register_builtin(&mut expander, "env", BuiltinMacro::Env);

    let span = dummy_span(FileId::BOGUS, 0);
    // Build argument: string literal "GLYIM_TEST_VAR"
    let mut builder = rowan::GreenNodeBuilder::new();
    builder.start_node(GlyimLang::kind_to_raw(glyim_syntax::SyntaxKind::TokenTree));
    builder.token(
        GlyimLang::kind_to_raw(glyim_syntax::SyntaxKind::StringLit),
        "\"GLYIM_TEST_VAR\"",
    );
    builder.finish_node();
    let args = SyntaxNode::new_root(builder.finish());
    let name = expander.interner().intern("env");

    let result = expander.expand(name, &args, span);
    assert!(result.diagnostics.is_empty());
    let expanded = result.expanded.unwrap();
    let text = expanded.text().to_string();
    assert!(
        text.contains("hello_from_env"),
        "Expected 'hello_from_env', got {}",
        text
    );
}

#[test]
fn test_option_env_macro() {
    // Present case: option_env! expands to Some("value").
    unsafe {
        std::env::set_var("GLYIM_TEST_OPTION_VAR", "present_value");
    }
    let mut hygiene = HygieneCtx::default();
    let mut expander = Expander::new(&mut hygiene);
    register_builtin(&mut expander, "option_env", BuiltinMacro::OptionEnv);

    let span = dummy_span(FileId::BOGUS, 0);
    let mut builder = rowan::GreenNodeBuilder::new();
    builder.start_node(GlyimLang::kind_to_raw(glyim_syntax::SyntaxKind::TokenTree));
    builder.token(
        GlyimLang::kind_to_raw(glyim_syntax::SyntaxKind::StringLit),
        "\"GLYIM_TEST_OPTION_VAR\"",
    );
    builder.finish_node();
    let args = SyntaxNode::new_root(builder.finish());
    let name = expander.interner().intern("option_env");

    let result = expander.expand(name, &args, span);
    assert!(result.diagnostics.is_empty());
    let text = result.expanded.unwrap().text().to_string();
    assert!(
        text.contains("Some(\"present_value\")"),
        "Expected Some(\"present_value\"), got {}",
        text
    );

    // Absent case: option_env! expands to None rather than erroring.
    let mut hygiene2 = HygieneCtx::default();
    let mut expander2 = Expander::new(&mut hygiene2);
    register_builtin(&mut expander2, "option_env", BuiltinMacro::OptionEnv);
    // Use a var name that is not set (and ensure it is unset for the test).
    unsafe {
        std::env::remove_var("GLYIM_TEST_OPTION_VAR_ABSENT");
    }
    let mut builder2 = rowan::GreenNodeBuilder::new();
    builder2.start_node(GlyimLang::kind_to_raw(glyim_syntax::SyntaxKind::TokenTree));
    builder2.token(
        GlyimLang::kind_to_raw(glyim_syntax::SyntaxKind::StringLit),
        "\"GLYIM_TEST_OPTION_VAR_ABSENT\"",
    );
    builder2.finish_node();
    let args2 = SyntaxNode::new_root(builder2.finish());
    let name2 = expander2.interner().intern("option_env");

    let result2 = expander2.expand(name2, &args2, span);
    assert!(
        result2.diagnostics.is_empty(),
        "option_env! must not error on missing var"
    );
    let text2 = result2.expanded.unwrap().text().to_string();
    assert!(text2.contains("None"), "Expected None, got {}", text2);
}

#[test]
fn test_include_macro() {
    let dir = tempdir().unwrap();
    let file_path = dir.path().join("test.txt");
    let content = "Hello, include!";
    fs::write(&file_path, content).unwrap();

    let mut hygiene = HygieneCtx::default();
    let mut expander = Expander::new(&mut hygiene);
    register_builtin(&mut expander, "include", BuiltinMacro::Include);

    let span = dummy_span(FileId::BOGUS, 0);
    // Build argument: string literal with path
    let mut builder = rowan::GreenNodeBuilder::new();
    builder.start_node(GlyimLang::kind_to_raw(glyim_syntax::SyntaxKind::TokenTree));
    builder.token(
        GlyimLang::kind_to_raw(glyim_syntax::SyntaxKind::StringLit),
        &format!("\"{}\"", file_path.to_str().unwrap()),
    );
    builder.finish_node();
    let args = SyntaxNode::new_root(builder.finish());
    let name = expander.interner().intern("include");

    let result = expander.expand(name, &args, span);
    assert!(result.diagnostics.is_empty());
    let expanded = result.expanded.unwrap();
    let text = expanded.text().to_string();
    assert!(
        text.contains(content),
        "Expected '{}', got {}",
        content,
        text
    );
}

#[test]
fn test_include_str_macro() {
    let dir = tempfile::tempdir().unwrap();
    let file_path = dir.path().join("test_str.txt");
    let content = "Hello, include_str!";
    fs::write(&file_path, content).unwrap();

    let mut hygiene = HygieneCtx::default();
    let mut expander = Expander::new(&mut hygiene);
    register_builtin(&mut expander, "include_str", BuiltinMacro::IncludeStr);

    let span = dummy_span(FileId::BOGUS, 0);
    let mut builder = rowan::GreenNodeBuilder::new();
    builder.start_node(GlyimLang::kind_to_raw(glyim_syntax::SyntaxKind::TokenTree));
    builder.token(
        GlyimLang::kind_to_raw(glyim_syntax::SyntaxKind::StringLit),
        &format!("\"{}\"", file_path.to_str().unwrap()),
    );
    builder.finish_node();
    let args = SyntaxNode::new_root(builder.finish());
    let name = expander.interner().intern("include_str");

    let result = expander.expand(name, &args, span);
    assert!(result.diagnostics.is_empty());
    let expanded = result.expanded.unwrap();
    let text = expanded.text().to_string();
    assert!(
        text.contains(content),
        "Expected '{}', got {}",
        content,
        text
    );
}

#[test]
fn test_include_bytes_macro() {
    let dir = tempfile::tempdir().unwrap();
    let file_path = dir.path().join("test_bytes.bin");
    let raw: &[u8] = &[104, 105, 10]; // "hi\n"
    fs::write(&file_path, raw).unwrap();

    let mut hygiene = HygieneCtx::default();
    let mut expander = Expander::new(&mut hygiene);
    register_builtin(&mut expander, "include_bytes", BuiltinMacro::IncludeBytes);

    let span = dummy_span(FileId::BOGUS, 0);
    let mut builder = rowan::GreenNodeBuilder::new();
    builder.start_node(GlyimLang::kind_to_raw(glyim_syntax::SyntaxKind::TokenTree));
    builder.token(
        GlyimLang::kind_to_raw(glyim_syntax::SyntaxKind::StringLit),
        &format!("\"{}\"", file_path.to_str().unwrap()),
    );
    builder.finish_node();
    let args = SyntaxNode::new_root(builder.finish());
    let name = expander.interner().intern("include_bytes");

    let result = expander.expand(name, &args, span);
    assert!(result.diagnostics.is_empty());
    let expanded = result.expanded.unwrap();
    let text = expanded.text().to_string();
    // Produces a bracketed integer-array literal `[104, 105, 10]`.
    assert!(
        text.contains("104") && text.contains("105") && text.contains("10"),
        "Expected byte-array literal, got {}",
        text
    );
}

#[test]
fn test_concat_macro() {
    let mut hygiene = HygieneCtx::default();
    let mut expander = Expander::new(&mut hygiene);
    register_builtin(&mut expander, "concat", BuiltinMacro::Concat);

    let span = dummy_span(FileId::BOGUS, 0);
    let args = concat_args("foo", "bar");
    let name = expander.interner().intern("concat");

    let result = expander.expand(name, &args, span);
    assert!(result.diagnostics.is_empty());
    let expanded = result.expanded.unwrap();
    let text = expanded.text().to_string();
    assert!(
        text.contains("\"foobar\""),
        "Expected 'foobar', got {}",
        text
    );
}

#[test]
fn test_concat_idents_macro() {
    // Helper to build args: a comma-separated token tree of identifier tokens.
    fn ident_args(parts: &[&str]) -> SyntaxNode {
        let mut builder = rowan::GreenNodeBuilder::new();
        builder.start_node(GlyimLang::kind_to_raw(glyim_syntax::SyntaxKind::TokenTree));
        for (i, p) in parts.iter().enumerate() {
            if i > 0 {
                builder.token(GlyimLang::kind_to_raw(glyim_syntax::SyntaxKind::Comma), ",");
            }
            builder.token(GlyimLang::kind_to_raw(glyim_syntax::SyntaxKind::Ident), p);
        }
        builder.finish_node();
        SyntaxNode::new_root(builder.finish())
    }

    let mut hygiene = HygieneCtx::default();
    let mut expander = Expander::new(&mut hygiene);
    register_builtin(&mut expander, "concat_idents", BuiltinMacro::ConcatIdents);

    let span = dummy_span(FileId::BOGUS, 0);
    let args = ident_args(&["foo", "_", "bar"]);
    let name = expander.interner().intern("concat_idents");

    let result = expander.expand(name, &args, span);
    assert!(result.diagnostics.is_empty());
    let expanded = result.expanded.unwrap();
    let text = expanded.text().to_string();
    assert!(
        text.contains("foo_bar"),
        "Expected identifier 'foo_bar', got {}",
        text
    );
}

#[test]
fn test_stringify_macro() {
    let mut hygiene = HygieneCtx::default();
    let mut expander = Expander::new(&mut hygiene);
    register_builtin(&mut expander, "stringify", BuiltinMacro::Stringify);

    let span = dummy_span(FileId::BOGUS, 0);
    let args = stringify_args("1 + 2");
    let name = expander.interner().intern("stringify");

    let result = expander.expand(name, &args, span);
    assert!(result.diagnostics.is_empty());
    let expanded = result.expanded.unwrap();
    let text = expanded.text().to_string();
    assert!(text.contains("1 + 2"), "Expected '1 + 2', got {}", text);
}
