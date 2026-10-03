//! S11-T01: Declarative macro matches and substitutes correctly

use crate::{BuiltinMacro, Expander, MacroDef, MacroKind};
use glyim_diag::GlyimDiagnostic;
use glyim_frontend::parse_to_syntax;
use glyim_span::{ByteIdx, FileId, HygieneCtx, Span, SyntaxContext};

/// Helper: parse source and return the syntax root.
fn parse(source: &str) -> glyim_syntax::SyntaxNode {
    parse_to_syntax(source, FileId::BOGUS).root
}

/// Test that a simple macro_rules! identity macro expands correctly.
#[test]
fn identity_macro_expands_to_input() {
    let source = r#"
macro_rules! ident {
    ($x:expr) => { $x }
}

fn main() {
    let _ = ident!(42);
}
"#;
    let root = parse(source);
    let mut hygiene = HygieneCtx::default();
    let mut expander = Expander::new(&mut hygiene);
    let (expanded, diags) = expander.expand_crate(&root);

    let has_error = diags.iter().any(|d: &GlyimDiagnostic| d.is_error());
    assert!(!has_error, "Expected no errors, got: {:?}", diags);

    let expanded_text = expanded.text().to_string();
    assert!(
        expanded_text.contains("42"),
        "Expected expanded output to contain '42', got: {}",
        expanded_text
    );
    assert!(
        !expanded_text.contains("ident!"),
        "Expected macro call 'ident!' to be expanded away, got: {}",
        expanded_text
    );
}

/// Test that a macro with multiple arms selects the correct one.
#[test]
fn multi_arm_macro_selects_correct_arm() {
    let source = r#"
macro_rules! choose {
    ($x:ident) => { 1 }
    ($x:literal) => { 2 }
}

fn main() {
    let _ = choose!(foo);
    let _ = choose!(99);
}
"#;
    let root = parse(source);
    let mut hygiene = HygieneCtx::default();
    let mut expander = Expander::new(&mut hygiene);
    let (expanded, diags) = expander.expand_crate(&root);

    let has_error = diags.iter().any(|d: &GlyimDiagnostic| d.is_error());
    assert!(!has_error, "Expected no errors, got: {:?}", diags);

    let expanded_text = expanded.text().to_string();
    assert!(
        expanded_text.contains("1"),
        "Expected expanded output to contain '1' from ident arm, got: {}",
        expanded_text
    );
    assert!(
        expanded_text.contains("2"),
        "Expected expanded output to contain '2' from literal arm, got: {}",
        expanded_text
    );
}

/// Test that a macro with repetition ($($x)* ) expands correctly.
#[test]
fn repetition_macro_expands() {
    let source = r#"
macro_rules! make_tuple {
    ($($x:tt),*) => { ($($x),*) }
}

fn main() {
    let _ = make_tuple!(1, 2, 3);
}
"#;
    let root = parse(source);
    let mut hygiene = HygieneCtx::default();
    let mut expander = Expander::new(&mut hygiene);
    let (expanded, diags) = expander.expand_crate(&root);

    let expanded_text = expanded.text().to_string();
    assert!(
        !expanded_text.contains("make_tuple!"),
        "Expected macro call to be expanded away, got: {}",
        expanded_text
    );
    let _ = diags;
}

/// Test that a macro producing a struct definition works.
#[test]
fn macro_produces_struct() {
    let source = r#"
macro_rules! unit_struct {
    ($name:ident) => { struct $name; }
}

unit_struct!(Foo);

fn main() {}
"#;
    let root = parse(source);
    let mut hygiene = HygieneCtx::default();
    let mut expander = Expander::new(&mut hygiene);
    let (expanded, diags) = expander.expand_crate(&root);

    let expanded_text = expanded.text().to_string();
    assert!(
        expanded_text.contains("struct"),
        "Expected expanded output to contain 'struct', got: {}",
        expanded_text
    );
    assert!(
        expanded_text.contains("Foo"),
        "Expected expanded output to contain 'Foo', got: {}",
        expanded_text
    );
    let _ = diags;
}

/// Test that substitution replaces $x with the captured value.
#[test]
fn substitution_replaces_metavar() {
    let source = r#"
macro_rules! echo {
    ($x:ident) => { $x }
}

fn main() {
    let _ = echo!(value);
}
"#;
    let root = parse(source);
    let mut hygiene = HygieneCtx::default();
    let mut expander = Expander::new(&mut hygiene);
    let (expanded, diags) = expander.expand_crate(&root);

    let has_error = diags.iter().any(|d: &GlyimDiagnostic| d.is_error());
    assert!(!has_error, "Expected no errors, got: {:?}", diags);

    let expanded_text = expanded.text().to_string();
    assert!(
        expanded_text.contains("value"),
        "Expected expanded output to contain 'value', got: {}",
        expanded_text
    );
    assert!(
        !expanded_text.contains("echo!"),
        "Expected macro call to be expanded away, got: {}",
        expanded_text
    );
}

/// §19.3: an unbound metavariable in the transcriber must be a hard error,
/// not silently dropped. Here `$y` is referenced in the expansion but never
/// captured by the matcher (`$x:ident`).
#[test]
fn unbound_metavariable_is_hard_error() {
    let source = r#"
macro_rules! bad {
    ($x:ident) => { $y }
}

fn main() {
    let _ = bad!(value);
}
"#;
    let root = parse(source);
    let mut hygiene = HygieneCtx::default();
    let mut expander = Expander::new(&mut hygiene);
    let (expanded, diags) = expander.expand_crate(&root);

    let has_error = diags.iter().any(|d: &GlyimDiagnostic| d.is_error());
    assert!(has_error, "Expected an error for unbound metavariable $y");

    // The macro call must NOT have been expanded away (it is ill-formed).
    let expanded_text = expanded.text().to_string();
    assert!(
        expanded_text.contains("bad!"),
        "Expected ill-formed macro call to be left intact, got: {}",
        expanded_text
    );

    let msg = diags
        .iter()
        .find(|d| d.is_error())
        .map(|d| d.message.clone())
        .unwrap_or_default();
    assert!(
        msg.contains("$y") && msg.contains("bad"),
        "Error should name the unbound metavariable and macro, got: {:?}",
        msg
    );
}

/// Test expand() public API with a registered builtin macro.
#[test]
fn expand_api_with_builtin_file_macro() {
    let source = r#"file!()"#;
    let root = parse(source);
    let mut hygiene = HygieneCtx::default();
    let mut expander = Expander::new(&mut hygiene);

    let name = expander.interner().intern("file");

    expander.register_macro(MacroDef {
        name,
        kind: MacroKind::Builtin {
            name,
            handler: BuiltinMacro::File,
        },
        span: Span::DUMMY,
    });

    let call_site = Span::new(
        FileId::from_raw(42),
        ByteIdx::from_raw(0),
        ByteIdx::from_raw(7),
        SyntaxContext::ROOT,
    );

    let result = expander.expand(name, &root, call_site);
    assert!(
        result.expanded.is_some(),
        "Expected file!() to produce an expansion, got diagnostics: {:?}",
        result.diagnostics
    );
    let expanded_text = result.expanded.unwrap().text().to_string();
    assert!(
        expanded_text.contains("42"),
        "Expected file!() expansion to contain file ID 42, got: {}",
        expanded_text
    );
}


/// HIR-2 regression: a repetition over a *multi-token* fragment must expand to
/// exactly one item per matched iteration, not one per captured token.
///
/// Before the depth-aware binding fix, `$($e:expr),*` matching `aa + bb, cc * dd`
/// recorded `e = [aa, +, bb, cc, *, dd]` (six "iterations"), so
/// `$( let _ = $e; )*` produced six malformed statements. It must produce two.
#[test]
fn repetition_multi_token_fragment_expands_once_per_iteration() {
    let source = r#"
macro_rules! stmts {
    ($($e:expr),*) => {
        $( let _ = $e; )*
    }
}

fn main() {
    stmts!(aa + bb, cc * dd);
}
"#;
    let root = parse(source);
    let mut hygiene = HygieneCtx::default();
    let mut expander = Expander::new(&mut hygiene);
    let (expanded, diags) = expander.expand_crate(&root);

    let has_error = diags.iter().any(|d: &GlyimDiagnostic| d.is_error());
    assert!(!has_error, "Expected no errors, got: {:?}", diags);

    let expanded_text = expanded.text().to_string();
    assert!(
        !expanded_text.contains("stmts!"),
        "Expected macro call to be expanded away, got: {}",
        expanded_text
    );
    // Exactly two `let` statements — one per `$e` iteration. The pre-fix code
    // emitted six (one per captured token: aa, +, bb, cc, *, dd).
    let lets = expanded_text.matches("let").count();
    assert_eq!(
        lets, 2,
        "multi-token fragments must expand once per iteration, got {lets} `let`s in: {}",
        expanded_text
    );
}


/// HIR-2 follow-on: the green→text round-trip in `expand_node_recursive`
/// reconstructs source from token texts. Without a separator, `let` and `_`
/// emitted as adjacent tokens fused into the single identifier `let_`, so any
/// macro whose expansion contained a `let _ = ...` failed downstream with
/// `[T0001] unresolved name 'let_'`. The builder must insert a `Whitespace`
/// token at every boundary that would change how the text re-lexes.
#[test]
fn expansion_does_not_fuse_adjacent_identifier_tokens() {
    let source = r#"
macro_rules! stmts {
    ($($e:expr),*) => {
        $( let _ = $e; )*
    }
}

fn main() {
    stmts!(1 + 2, 3 * 4);
}
"#;
    let root = parse(source);
    let mut hygiene = HygieneCtx::default();
    let mut expander = Expander::new(&mut hygiene);
    let (expanded, diags) = expander.expand_crate(&root);

    let has_error = diags.iter().any(|d: &GlyimDiagnostic| d.is_error());
    assert!(!has_error, "Expected no errors, got: {:?}", diags);

    let text = expanded.text().to_string();
    assert!(
        !text.contains("let_"),
        "`let` and `_` fused into `let_`; expansion text was: {}",
        text
    );
    // Both `let`s must still be present (HIR-2: one per iteration).
    assert_eq!(text.matches("let").count(), 2, "expansion was: {}", text);
}
