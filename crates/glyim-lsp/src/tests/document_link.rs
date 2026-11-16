//! Policy-level tests for the document-link engine (LSP-DOC-LINK).
//!
//! Mirrors `semantic_tokens.rs` and `implementation.rs`: the *policy*
//! (which token shapes are include-macro link sites, and what each one
//! targets) is pinned through an in-memory [`MockSource`] implementing
//! [`crate::document_link::LinkLexSource`], decoupled from any rowan
//! tree, lock, or source map. The parse-tree-backed production path and
//! the full wire request are covered end-to-end in
//! `document_link_tests.rs`.

use crate::document_link::{IncludeLink, IncludeMacroKind, LinkLexSource, LinkToken, link_targets};
use glyim_syntax::SyntaxKind;
use std::path::{Path, PathBuf};

/// In-memory token store: the policy-side test double. Tokens are
/// appended in source order; text is carried verbatim (a string literal
/// keeps its quotes, exactly like the lexer's token text).
#[derive(Default)]
struct MockSource {
    tokens: Vec<LinkToken>,
}

impl MockSource {
    fn add(&mut self, lo: usize, hi: usize, kind: SyntaxKind, text: &str) {
        self.tokens.push(LinkToken {
            lo,
            hi,
            kind,
            text: text.to_string(),
        });
    }

    /// `name!(` — the macro-call opener the policy matches on.
    fn call_open(&mut self, lo: usize, name: &str) {
        let name_hi = lo + name.len();
        self.add(lo, name_hi, SyntaxKind::Ident, name);
        self.add(name_hi, name_hi + 1, SyntaxKind::Bang, "!");
        self.add(name_hi + 1, name_hi + 2, SyntaxKind::LParen, "(");
    }

    /// `)` — the macro-call closer.
    fn call_close(&mut self, lo: usize) {
        self.add(lo, lo + 1, SyntaxKind::RParen, ")");
    }
}

impl LinkLexSource for MockSource {
    fn lexical_tokens(&self) -> Vec<LinkToken> {
        self.tokens.clone()
    }
}

/// A relative target lands inside the document's directory.
#[test]
fn include_str_literal_links_relative_to_the_document_dir() {
    let mut mock = MockSource::default();
    // include_str!("lib/header.g")
    mock.call_open(0, "include_str");
    mock.add(14, 29, SyntaxKind::StringLit, "\"lib/header.g\"");
    mock.call_close(29);
    let doc_dir = Path::new("/proj/src");
    let links = link_targets(&mock, doc_dir);
    assert_eq!(links.len(), 1);
    let link = &links[0];
    assert_eq!(link.macro_kind, IncludeMacroKind::IncludeStr);
    // The literal's exact token extent (quotes included) is the clickable
    // range (LSP-EXACT-EXTENTS convention).
    assert_eq!((link.literal_lo, link.literal_hi), (14, 29));
    assert_eq!(link.target, PathBuf::from("/proj/src/lib/header.g"));
}

/// All three file-taking builtin macros link; the ident decides the kind.
#[test]
fn include_and_include_bytes_link_with_their_own_kinds() {
    let mut mock = MockSource::default();
    // include!("mod.g") include_bytes!("img.dat")
    mock.call_open(0, "include");
    mock.add(9, 16, SyntaxKind::StringLit, "\"mod.g\"");
    mock.call_close(16);
    mock.call_open(17, "include_bytes");
    mock.add(32, 41, SyntaxKind::StringLit, "\"img.dat\"");
    mock.call_close(41);
    let links = link_targets(&mock, Path::new("/w"));
    assert_eq!(links.len(), 2);
    assert_eq!(links[0].macro_kind, IncludeMacroKind::Include);
    assert_eq!(links[0].target, PathBuf::from("/w/mod.g"));
    assert_eq!(links[1].macro_kind, IncludeMacroKind::IncludeBytes);
    assert_eq!(links[1].target, PathBuf::from("/w/img.dat"));
}

/// `env!`/`option_env!`/`format!` take no file path; they are not link
/// sites even when their argument is a string literal.
#[test]
fn env_and_format_are_not_file_macros() {
    let mut mock = MockSource::default();
    // env!("PATH") format!("{} {}", 1, 2) option_env!("X")
    mock.call_open(0, "env");
    mock.add(5, 11, SyntaxKind::StringLit, "\"PATH\"");
    mock.call_close(11);
    mock.call_open(12, "format");
    mock.add(20, 24, SyntaxKind::StringLit, "\"{}\"");
    mock.call_close(24);
    mock.call_open(25, "option_env");
    mock.add(37, 40, SyntaxKind::StringLit, "\"X\"");
    mock.call_close(40);
    assert!(link_targets(&mock, Path::new("/w")).is_empty());
}

/// A include-macro call whose argument is not a bare string literal
/// (e.g. `include!(concat!("a", "b"))`) produces no link: the expansion
/// target is not knowable from source, so guessing would be dishonest.
#[test]
fn non_literal_argument_produces_no_link() {
    let mut mock = MockSource::default();
    // include!(concat!("a", "b"))  — an Ident, not a StringLit, follows `(`.
    mock.call_open(0, "include");
    mock.add(9, 15, SyntaxKind::Ident, "concat");
    mock.call_close(15);
    assert!(link_targets(&mock, Path::new("/w")).is_empty());
}

/// An unterminated call shape (missing `!`, missing `(`, missing `)`)
/// produces no link — the policy matches whole call shapes only.
#[test]
fn partial_call_shapes_produce_no_link() {
    // `include "x.g"` — no `!`.
    let mut mock = MockSource::default();
    mock.add(0, 7, SyntaxKind::Ident, "include");
    mock.add(8, 15, SyntaxKind::StringLit, "\"x.g\"");
    assert!(link_targets(&mock, Path::new("/w")).is_empty());
    // `include!("x.g"` — no closing `)`.
    let mut mock = MockSource::default();
    mock.call_open(0, "include");
    mock.add(9, 15, SyntaxKind::StringLit, "\"x.g\"");
    assert!(link_targets(&mock, Path::new("/w")).is_empty());
}

/// An absolute literal is used as-is; it is not joined onto the
/// document's directory.
#[test]
fn absolute_target_is_used_as_is() {
    let mut mock = MockSource::default();
    mock.call_open(0, "include_str");
    mock.add(14, 25, SyntaxKind::StringLit, "\"/etc/hosts\"");
    mock.call_close(25);
    let links = link_targets(&mock, Path::new("/proj/src"));
    assert_eq!(links.len(), 1);
    assert_eq!(links[0].target, PathBuf::from("/etc/hosts"));
}

/// Escapes inside the literal are decoded (`\\` and `\"`), and an empty
/// literal links nowhere.
#[test]
fn escaped_sequences_decode_and_empty_literal_links_nowhere() {
    let mut mock = MockSource::default();
    // include!("a\\b\"c.g")
    mock.call_open(0, "include");
    mock.add(9, 20, SyntaxKind::StringLit, "\"a\\\\b\\\"c.g\"");
    mock.call_close(20);
    let links = link_targets(&mock, Path::new("/w"));
    assert_eq!(links.len(), 1);
    assert_eq!(links[0].target, PathBuf::from("/w/a\\b\"c.g"));
    // include!("") — empty target, no link.
    let mut mock = MockSource::default();
    mock.call_open(0, "include");
    mock.add(9, 11, SyntaxKind::StringLit, "\"\"");
    mock.call_close(11);
    assert!(link_targets(&mock, Path::new("/w")).is_empty());
}

/// Links come in source order — the token walk order — so the wire
/// response is deterministic regardless of any map iteration upstream.
#[test]
fn links_come_in_source_order() {
    let mut mock = MockSource::default();
    // include!("b.g") ... include!("a.g") — declaration order wins.
    mock.call_open(0, "include");
    mock.add(9, 14, SyntaxKind::StringLit, "\"b.g\"");
    mock.call_close(14);
    mock.call_open(15, "include");
    mock.add(24, 29, SyntaxKind::StringLit, "\"a.g\"");
    mock.call_close(29);
    let links: Vec<IncludeLink> = link_targets(&mock, Path::new("/w"));
    let targets: Vec<PathBuf> = links.iter().map(|l| l.target.clone()).collect();
    assert_eq!(
        targets,
        vec![PathBuf::from("/w/b.g"), PathBuf::from("/w/a.g")]
    );
}
