//! Choice-policy tests for the goto-implementation request
//! (LSP-IMPLEMENTATION).
//!
//! Mirrors `tests/goto_declaration.rs` and `tests/type_definition.rs`: the
//! policy in [`crate::implementation::implementation_targets`] reads
//! recorded impl sites through the [`crate::implementation::ImplSource`]
//! seam, so the *choice* is pinned here without a parse tree, HIR arena,
//! or database lock. The production [`crate::implementation::HirImplSource`]
//! and the full wire path are covered end-to-end in
//! `implementation_tests.rs`.

use crate::implementation::{ImplSite, ImplSource, implementation_targets};
use crate::symbol_index::DefinitionLocation;
use glyim_span::{ByteIdx, FileId, Span, SyntaxContext};

fn span(file: u32, lo: usize, hi: usize) -> Span {
    Span::range(
        FileId::from_usize(file as usize),
        ByteIdx::from_usize(lo),
        ByteIdx::from_usize(hi),
        SyntaxContext::ROOT,
    )
}

fn location(file: u32, lo: usize, hi: usize) -> DefinitionLocation {
    DefinitionLocation {
        file_id: FileId::from_usize(file as usize),
        span: span(file, lo, hi),
    }
}

/// An impl site with both names unset matches nothing (the engine's
/// "unknown shape" honesty); this helper builds the common shapes.
fn site(trait_name: Option<&str>, self_ty_name: Option<&str>, lo: usize, hi: usize) -> ImplSite {
    ImplSite {
        trait_name: trait_name.map(str::to_string),
        self_ty_name: self_ty_name.map(str::to_string),
        location: location(0, lo, hi),
    }
}

struct MockSource {
    sites: Vec<ImplSite>,
}

impl ImplSource for MockSource {
    fn impl_sites(&self) -> Vec<ImplSite> {
        self.sites.clone()
    }
}

#[test]
fn trait_name_matches_answer_the_impl_site() {
    // `impl Greet for P` recorded with its trait name: a cursor on the
    // trait symbol answers the impl block, byte-exact.
    let mock = MockSource {
        sites: vec![site(Some("Greet"), Some("P"), 40, 90)],
    };
    let targets = implementation_targets(&mock, "Greet");
    assert_eq!(targets.len(), 1);
    assert_eq!(targets[0].span.lo().to_usize(), 40);
    assert_eq!(targets[0].span.hi().to_usize(), 90);
}

#[test]
fn self_ty_name_matches_answer_the_impl_site_too() {
    // A cursor on the *type* symbol answers the impls of that type —
    // both trait impls and inherent impls.
    let mock = MockSource {
        sites: vec![site(Some("Greet"), Some("P"), 40, 90)],
    };
    let targets = implementation_targets(&mock, "P");
    assert_eq!(targets.len(), 1);
    assert_eq!(targets[0].span.lo().to_usize(), 40);

    let inherent = MockSource {
        sites: vec![site(None, Some("P"), 100, 150)],
    };
    let targets = implementation_targets(&inherent, "P");
    assert_eq!(targets.len(), 1);
    assert_eq!(targets[0].span.lo().to_usize(), 100);
}

#[test]
fn unrelated_names_answer_nothing() {
    // A cursor on a function or local name must not masquerade as an
    // implementation query: only trait and type names qualify.
    let mock = MockSource {
        sites: vec![site(Some("Greet"), Some("P"), 40, 90)],
    };
    assert!(implementation_targets(&mock, "name").is_empty());
    assert!(implementation_targets(&mock, "n").is_empty());
    assert!(implementation_targets(&mock, "impl").is_empty());
}

#[test]
fn an_impl_with_no_names_matches_nothing() {
    // A malformed or non-Path self type leaves both names unset; the
    // site stays recorded (the source reports facts) but the policy
    // never answers it — there is no symbol it can honestly attach to.
    let mock = MockSource {
        sites: vec![site(None, None, 40, 90)],
    };
    assert!(implementation_targets(&mock, "Greet").is_empty());
    assert!(implementation_targets(&mock, "P").is_empty());
}

#[test]
fn a_name_matching_both_sides_answers_once() {
    // `impl Greet for Greet`-shaped sites (the trait name equals the
    // self type name) match both filters; the policy dedupes to one
    // target — a duplicated location would render as two identical
    // editor highlights.
    let mock = MockSource {
        sites: vec![site(Some("Greet"), Some("Greet"), 40, 90)],
    };
    let targets = implementation_targets(&mock, "Greet");
    assert_eq!(targets.len(), 1);
    assert_eq!(targets[0].span.lo().to_usize(), 40);
}

#[test]
fn zero_width_spans_are_skipped() {
    // A zero-width extent cannot name a jump target (the same rule
    // `declaration_target` applies); answering it would aim the client
    // at a collapsed caret instead of the impl block.
    let mock = MockSource {
        sites: vec![
            site(Some("Greet"), Some("P"), 40, 40),
            site(Some("Greet"), Some("P"), 50, 80),
        ],
    };
    let targets = implementation_targets(&mock, "Greet");
    assert_eq!(targets.len(), 1);
    assert_eq!(targets[0].span.lo().to_usize(), 50);
}

#[test]
fn targets_are_ordered_by_file_then_offset() {
    // The production source walks a per-file map whose iteration order
    // is not deterministic, so the policy pins the wire order: sorted by
    // (file id, span start), which makes the response stable across
    // runs regardless of the recording order.
    let mock = MockSource {
        sites: vec![
            ImplSite {
                trait_name: Some("Greet".to_string()),
                self_ty_name: Some("Q".to_string()),
                location: location(1, 10, 20),
            },
            site(Some("Greet"), Some("P"), 90, 120),
            site(Some("Greet"), Some("P"), 30, 60),
        ],
    };
    let targets = implementation_targets(&mock, "Greet");
    assert_eq!(targets.len(), 3);
    assert_eq!(targets[0].file_id.to_raw(), 0);
    assert_eq!(targets[0].span.lo().to_usize(), 30);
    assert_eq!(targets[1].file_id.to_raw(), 0);
    assert_eq!(targets[1].span.lo().to_usize(), 90);
    assert_eq!(targets[2].file_id.to_raw(), 1);
    assert_eq!(targets[2].span.lo().to_usize(), 10);
}
