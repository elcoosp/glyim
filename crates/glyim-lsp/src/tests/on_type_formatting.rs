//! Policy-level tests for the on-type-formatting engine
//! (LSP-ON-TYPE-FORMAT).
//!
//! Mirrors `document_link.rs` and `code_lens.rs`: the *policy* (what a
//! just-typed closing bracket should be indented to) is pinned through
//! an in-memory [`MockSource`] implementing
//! [`crate::on_type_formatting::OnTypeTokenSource`], decoupled from any
//! rowan tree, lock, or source map. The parse-tree-backed production
//! path and the full wire request are covered end-to-end in
//! `on_type_formatting_tests.rs`.

use crate::on_type_formatting::{BracketToken, OnTypeTokenSource, on_type_indent};
use glyim_syntax::SyntaxKind;

/// In-memory bracket store: the policy-side test double. Tokens are
/// appended in source order; only the six bracket kinds are kept (the
/// production source filters the same way). Offsets are byte positions
/// into `text`, spelled out beside each fixture line.
#[derive(Default)]
struct MockSource {
    brackets: Vec<BracketToken>,
}

impl MockSource {
    fn open(&mut self, lo: usize, kind: SyntaxKind) {
        self.brackets.push(BracketToken {
            lo,
            hi: lo + 1,
            kind,
        });
    }
    fn close(&mut self, lo: usize, kind: SyntaxKind) {
        self.brackets.push(BracketToken {
            lo,
            hi: lo + 1,
            kind,
        });
    }
}

impl OnTypeTokenSource for MockSource {
    fn bracket_tokens(&self) -> Vec<BracketToken> {
        self.brackets.clone()
    }
}

/// `fn f() {` on its own line, a nested block, and the just-typed `}`
/// carrying an auto-indent the editor guessed (12 spaces). The fix:
/// dedent to the opener line's indentation (4 spaces).
///
/// ```text
/// fn f() {          <- opener at byte 7, line indent ""
///     if x {        <- mid-line opener at byte 18, line indent "    "
///         0
///             }     <- typed `}` at byte 42, run 30..42 (12 spaces)
/// ```
#[test]
fn closer_dedents_to_the_opener_line_indent() {
    let text = "fn f() {\n    if x {\n        0\n            }\n";
    let mut mock = MockSource::default();
    mock.open(7, SyntaxKind::LBrace);
    mock.open(18, SyntaxKind::LBrace);
    mock.close(42, SyntaxKind::RBrace);

    let fix = on_type_indent(&mock, text, 42, '}').expect("a dedent is due");
    assert_eq!(&text[fix.indent_lo..fix.indent_hi], "            ");
    assert_eq!(fix.indent, "    ");
}

/// An opener that sits mid-line still fixes the closer to the opener
/// line's *leading* whitespace — the standard style answer.
#[test]
fn mid_line_opener_aligns_with_its_line_indent() {
    let text = "fn f() {\n    if x {\n        0\n            }\n";
    let mut mock = MockSource::default();
    mock.open(18, SyntaxKind::LBrace); // "    if x {"
    mock.close(42, SyntaxKind::RBrace);
    let fix = on_type_indent(&mock, text, 42, '}').expect("a dedent is due");
    assert_eq!(fix.indent, "    ");
}

/// Tabs are preserved verbatim: the replacement is the opener line's
/// leading whitespace exactly as spelled. The fixture spells real tab
/// characters; the comment tree below shows them as four spaces.
///
/// ```text
/// fn f() {      <- opener at byte 7
///     if x {    <- opener at byte 15 (one tab in the fixture)
///         0
///             }  <- typed `}` at byte 24, run of three tabs
/// ```
#[test]
fn tab_indents_are_preserved() {
    let text = "fn f() {\n\tif x {\n\t\t0\n\t\t\t}\n";
    let mut mock = MockSource::default();
    mock.open(7, SyntaxKind::LBrace);
    mock.open(15, SyntaxKind::LBrace);
    mock.close(24, SyntaxKind::RBrace);
    let fix = on_type_indent(&mock, text, 24, '}').expect("a dedent is due");
    assert_eq!(fix.indent, "\t");
}

/// The closer already sits at the right indent: no edit (the caller
/// answers `null` — a zero-diff rewrite would churn the buffer).
///
/// ```text
/// fn f() {
///     if x {
///         0
///     }         <- typed `}` at byte 34, run "    " — already right
/// ```
#[test]
fn already_correct_indent_answers_none() {
    let text = "fn f() {\n    if x {\n        0\n    }\n";
    let mut mock = MockSource::default();
    mock.open(7, SyntaxKind::LBrace);
    mock.open(18, SyntaxKind::LBrace);
    mock.close(34, SyntaxKind::RBrace);
    assert!(on_type_indent(&mock, text, 34, '}').is_none());
}

/// Code already precedes the typed closer on its line (`… 0 }`): there
/// is no indent run to fix.
#[test]
fn non_whitespace_before_closer_answers_none() {
    let text = "fn f() {\n    0 }\n";
    let mut mock = MockSource::default();
    mock.open(7, SyntaxKind::LBrace);
    mock.close(15, SyntaxKind::RBrace);
    assert!(on_type_indent(&mock, text, 15, '}').is_none());
}

/// A closer with no opener on the stack (unbalanced code): honest `None`,
/// never a guess.
#[test]
fn unbalanced_closer_answers_none() {
    let text = "    }\n";
    let mut mock = MockSource::default();
    mock.close(4, SyntaxKind::RBrace);
    assert!(on_type_indent(&mock, text, 4, '}').is_none());
}

/// The matching opener is of the wrong family (`(` opened, `}` typed):
/// honest `None` — an editor that fires on both `}` and `)` must not
/// cross-fix families.
#[test]
fn family_mismatch_answers_none() {
    let text = "fn f() (\n    }\n";
    let mut mock = MockSource::default();
    mock.open(7, SyntaxKind::LParen);
    mock.close(13, SyntaxKind::RBrace);
    assert!(on_type_indent(&mock, text, 13, '}').is_none());
}

/// The typed character is not a closing bracket at all (`{`/`(`/`[` are
/// openers; the capability answers nothing for them).
#[test]
fn opener_character_answers_none() {
    let text = "fn f() {\n";
    let mut mock = MockSource::default();
    mock.open(7, SyntaxKind::LBrace);
    assert!(on_type_indent(&mock, text, 8, '{').is_none());
}

/// The typed char does not sit right before the reported position (the
/// client's buffer state and the server's text disagree): honest `None`.
#[test]
fn typed_char_mismatch_with_text_answers_none() {
    let text = "fn f() {\n    }\n";
    let mut mock = MockSource::default();
    mock.open(7, SyntaxKind::LBrace);
    mock.close(13, SyntaxKind::RBrace);
    // `x` is not what the text carries at byte 11.
    assert!(on_type_indent(&mock, text, 11, 'x').is_none());
}

/// The just-typed closer token is not in the token stream (the editor
/// fired before the parse tree caught up): honest `None`.
#[test]
fn closer_missing_from_token_stream_answers_none() {
    let text = "fn f() {\n    }\n";
    let mut mock = MockSource::default();
    mock.open(7, SyntaxKind::LBrace);
    // No close token recorded; the typed char is the `}` at byte 13.
    assert!(on_type_indent(&mock, text, 13, '}').is_none());
}

/// Parens and brackets follow the same rule (`[` opener, `]` closer).
///
/// ```text
/// fn f() {
///     let t = [     <- opener at byte 21, line indent "    "
///         0,
///             ]     <- typed `]` at byte 46, run 34..46
/// ```
#[test]
fn parens_and_brackets_align_like_braces() {
    let text = "fn f() {\n    let t = [\n        0,\n            ]\n";
    let mut mock = MockSource::default();
    mock.open(21, SyntaxKind::LBracket);
    mock.close(46, SyntaxKind::RBracket);
    let fix = on_type_indent(&mock, text, 46, ']').expect("a dedent is due");
    assert_eq!(&text[fix.indent_lo..fix.indent_hi], "            ");
    assert_eq!(fix.indent, "    ");
}
