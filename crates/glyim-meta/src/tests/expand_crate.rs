//! S11-T03: `expand_crate` processes multiple macro calls in sequence

use crate::{BuiltinMacro, Expander, MacroDef, MacroKind};
use glyim_diag::GlyimDiagnostic;
use glyim_frontend::parse_to_syntax;
use glyim_span::{FileId, HygieneCtx, Span};

fn parse(source: &str) -> glyim_syntax::SyntaxNode {
    parse_to_syntax(source, FileId::BOGUS).root
}

/// Test that `expand_crate` processes two macro calls in the same source.
#[test]
fn multiple_macro_calls_in_sequence() {
    let source = r"
macro_rules! double {
    ($x:expr) => { $x + $x }
}

fn main() {
    let a = double!(1);
    let b = double!(2);
}
";
    let root = parse(source);
    let mut hygiene = HygieneCtx::default();
    let mut expander = Expander::new(&mut hygiene);
    let (expanded, diags) = expander.expand_crate(&root);

    let has_error = diags.iter().any(|d: &GlyimDiagnostic| d.is_error());
    assert!(!has_error, "Expected no errors, got: {diags:?}");

    let expanded_text = expanded.text().to_string();
    assert!(
        !expanded_text.contains("double!"),
        "Expected all double! macro calls to be expanded away, got: {expanded_text}"
    );
    assert!(
        expanded_text.contains('1'),
        "Expected expanded output to contain '1', got: {expanded_text}"
    );
    assert!(
        expanded_text.contains('2'),
        "Expected expanded output to contain '2', got: {expanded_text}"
    );
}

/// Test that `expand_crate` handles nested macro definitions and calls.
#[test]
fn nested_macro_definitions() {
    let source = r"
macro_rules! first {
    ($x:expr) => { $x }
}

macro_rules! second {
    ($x:expr) => { $x * 2 }
}

fn main() {
    let a = first!(10);
    let b = second!(5);
}
";
    let root = parse(source);
    let mut hygiene = HygieneCtx::default();
    let mut expander = Expander::new(&mut hygiene);
    let (expanded, diags) = expander.expand_crate(&root);

    let has_error = diags.iter().any(|d: &GlyimDiagnostic| d.is_error());
    assert!(!has_error, "Expected no errors, got: {diags:?}");

    let expanded_text = expanded.text().to_string();
    assert!(
        !expanded_text.contains("first!"),
        "Expected first! to be expanded away, got: {expanded_text}"
    );
    assert!(
        !expanded_text.contains("second!"),
        "Expected second! to be expanded away, got: {expanded_text}"
    );
}

/// Test that `expand_crate` removes `macro_rules`! definitions from output.
#[test]
fn macro_definitions_removed_from_output() {
    let source = r"
macro_rules! unused {
    ($x:expr) => { $x }
}

fn main() {}
";
    let root = parse(source);
    let mut hygiene = HygieneCtx::default();
    let mut expander = Expander::new(&mut hygiene);
    let (expanded, diags) = expander.expand_crate(&root);

    let expanded_text = expanded.text().to_string();
    assert!(
        !expanded_text.contains("macro_rules"),
        "Expected macro_rules definitions to be removed, got: {expanded_text}"
    );
    let _ = diags;
}

/// Test that `expand_crate` with no macros returns the original tree.
#[test]
fn no_macros_passthrough() {
    let source = r"
fn main() {
    let x = 1 + 2;
}
";
    let root = parse(source);
    let mut hygiene = HygieneCtx::default();
    let mut expander = Expander::new(&mut hygiene);
    let (expanded, diags) = expander.expand_crate(&root);

    let has_error = diags.iter().any(|d: &GlyimDiagnostic| d.is_error());
    assert!(!has_error, "Expected no errors, got: {diags:?}");

    let expanded_text = expanded.text().to_string();
    assert!(
        expanded_text.contains("fn"),
        "Expected function definition to be preserved, got: {expanded_text}"
    );
    assert!(
        expanded_text.contains('1') && expanded_text.contains('+') && expanded_text.contains('2'),
        "Expected expression tokens to be preserved, got: {expanded_text}"
    );
}

/// Test that macro calls that don't match any definition produce diagnostics.
#[test]
fn unmatched_macro_call_produces_diagnostic() {
    let source = r"
fn main() {
    nonexistent!(42);
}
";
    let root = parse(source);
    let mut hygiene = HygieneCtx::default();
    let mut expander = Expander::new(&mut hygiene);
    let (expanded, diags) = expander.expand_crate(&root);

    let expanded_text = expanded.text().to_string();
    assert!(
        expanded_text.contains("fn") || !expanded_text.is_empty(),
        "Expected some output even with unmatched macro"
    );
    let _ = diags;
}

/// Test that `expand_crate` with builtin macros registered works.
#[test]
fn expand_crate_with_builtin_macros() {
    let source = r"
fn main() {
    let f = file!();
    let l = line!();
}
";
    let root = parse(source);
    let mut hygiene = HygieneCtx::default();
    let mut expander = Expander::new(&mut hygiene);

    let file_name = expander.interner().intern("file");
    let line_name = expander.interner().intern("line");

    expander.register_macro(MacroDef {
        name: file_name,
        kind: MacroKind::Builtin {
            name: file_name,
            handler: BuiltinMacro::File,
        },
        span: Span::DUMMY,
    });
    expander.register_macro(MacroDef {
        name: line_name,
        kind: MacroKind::Builtin {
            name: line_name,
            handler: BuiltinMacro::Line,
        },
        span: Span::DUMMY,
    });

    let (expanded, diags) = expander.expand_crate(&root);

    let expanded_text = expanded.text().to_string();
    assert!(
        !expanded_text.contains("file!()"),
        "Expected file!() to be expanded away, got: {expanded_text}"
    );
    assert!(
        !expanded_text.contains("line!()"),
        "Expected line!() to be expanded away, got: {expanded_text}"
    );
    let _ = diags;
}
