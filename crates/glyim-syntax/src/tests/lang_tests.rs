//! Tests for `GlyimLang` `rowan::Language` impl

use crate::{GlyimLang, SyntaxKind};
use rowan::Language;

#[test]
fn kind_from_raw_roundtrip() {
    let all_kinds = [
        SyntaxKind::KwFn,
        SyntaxKind::Ident,
        SyntaxKind::IntLit,
        SyntaxKind::Plus,
        SyntaxKind::LParen,
        SyntaxKind::Whitespace,
        SyntaxKind::SourceFile,
        SyntaxKind::Error,
    ];
    for kind in all_kinds {
        let raw = GlyimLang::kind_to_raw(kind);
        let roundtrip = GlyimLang::kind_from_raw(raw);
        assert_eq!(roundtrip, kind, "Roundtrip failed for {kind:?}");
    }
}

#[test]
fn kind_from_raw_converts_error_for_unknown_raw() {
    let unknown_raw = rowan::SyntaxKind((SyntaxKind::Error as u16) + 100);
    let kind = GlyimLang::kind_from_raw(unknown_raw);
    assert_eq!(kind, SyntaxKind::Error);
}

#[test]
fn kind_to_raw_matches_inner_u16() {
    let kind = SyntaxKind::SourceFile;
    let raw = GlyimLang::kind_to_raw(kind);
    assert_eq!(raw.0, kind as u16);
}
