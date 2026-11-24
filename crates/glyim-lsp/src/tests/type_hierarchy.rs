//! Policy-level tests for the type-hierarchy engine (LSP-TYPE-HIERARCHY).
//!
//! Mirrors `implementation.rs` and `call_hierarchy.rs`: the *policy*
//! (which recorded items can start a hierarchy, and which recorded impl
//! facts answer supertypes/subtypes) is pinned through an in-memory
//! [`MockSource`] implementing [`crate::type_hierarchy::TypeHierarchySource`],
//! decoupled from any HIR arena, interner, or lock. The HIR-backed
//! production path and the full wire requests (including the echo
//! re-resolution) are covered end-to-end in `type_hierarchy_tests.rs`.

use crate::implementation::ImplSite;
use crate::symbol_index::DefinitionLocation;
use crate::type_hierarchy::{
    HierarchyItemKind, HierarchyMember, NamedItem, TypeHierarchySource, compute_subtypes,
    compute_supertypes, hierarchy_target,
};
use glyim_span::{FileId, Span, SyntaxContext};

/// In-memory type-hierarchy store: the policy-side test double. Items and
/// impl sites are listed in insertion order; the policy applies its own
/// ordering, dedupe, and skip rules on top.
#[derive(Default)]
struct MockSource {
    items: Vec<NamedItem>,
    impls: Vec<ImplSite>,
}

impl MockSource {
    fn item(&mut self, name: &str, kind: HierarchyItemKind, file: u32, lo: usize, hi: usize) {
        self.items.push(NamedItem {
            name: name.to_string(),
            kind,
            location: DefinitionLocation {
                file_id: FileId::from_usize(file as usize),
                span: span_of(file, lo, hi),
            },
        });
    }
    fn site(
        &mut self,
        trait_name: Option<&str>,
        self_ty_name: Option<&str>,
        file: u32,
        lo: usize,
        hi: usize,
    ) {
        self.impls.push(ImplSite {
            trait_name: trait_name.map(str::to_string),
            self_ty_name: self_ty_name.map(str::to_string),
            location: DefinitionLocation {
                file_id: FileId::from_usize(file as usize),
                span: span_of(file, lo, hi),
            },
        });
    }
}

fn span_of(file: u32, lo: usize, hi: usize) -> Span {
    Span::new(
        FileId::from_usize(file as usize),
        glyim_span::ByteIdx::from_usize(lo),
        glyim_span::ByteIdx::from_usize(hi),
        SyntaxContext::ROOT,
    )
}

impl TypeHierarchySource for MockSource {
    fn named_item(&self, name: &str) -> Option<NamedItem> {
        self.items.iter().find(|item| item.name == name).cloned()
    }
    fn impl_sites_for_type(&self, type_name: &str) -> Vec<ImplSite> {
        self.impls
            .iter()
            .filter(|site| {
                site.self_ty_name.as_deref() == Some(type_name) && site.trait_name.is_some()
            })
            .cloned()
            .collect()
    }
    fn impl_sites_of_trait(&self, trait_name: &str) -> Vec<ImplSite> {
        self.impls
            .iter()
            .filter(|site| site.trait_name.as_deref() == Some(trait_name))
            .cloned()
            .collect()
    }
}

/// The cursor's item answers with the recorded classification and the
/// recorded extent — trait, struct, and enum alike.
#[test]
fn hierarchy_target_classifies_recorded_items() {
    let mut mock = MockSource::default();
    mock.item("Greet", HierarchyItemKind::Trait, 0, 0, 20);
    mock.item("P", HierarchyItemKind::Struct, 0, 21, 39);
    mock.item("E", HierarchyItemKind::Enum, 0, 40, 60);

    let greet = hierarchy_target(&mock, "Greet").expect("trait prepares");
    assert_eq!(greet.kind, HierarchyItemKind::Trait);
    assert_eq!(greet.location.file_id, FileId::from_usize(0));
    assert_eq!(greet.location.span.lo.to_usize(), 0);

    let p = hierarchy_target(&mock, "P").expect("struct prepares");
    assert_eq!(p.kind, HierarchyItemKind::Struct);

    let e = hierarchy_target(&mock, "E").expect("enum prepares");
    assert_eq!(e.kind, HierarchyItemKind::Enum);
}

/// A name the source records no trait/struct/enum item for — a function,
/// a local, or a total miss — prepares nothing.
#[test]
fn hierarchy_target_answers_none_for_unrecorded_names() {
    let mut mock = MockSource::default();
    mock.item("P", HierarchyItemKind::Struct, 0, 0, 10);
    assert!(hierarchy_target(&mock, "main").is_none());
    assert!(hierarchy_target(&mock, "").is_none());
}

/// A zero-width recorded extent cannot name a jump target, so it must
/// not be answered as one (the same rule `declaration_target` and
/// `implementation_targets` apply).
#[test]
fn hierarchy_target_skips_zero_width_extents() {
    let mut mock = MockSource::default();
    mock.item("Ghost", HierarchyItemKind::Struct, 0, 7, 7);
    assert!(hierarchy_target(&mock, "Ghost").is_none());
}

/// Supertypes of a type: one member per trait impl site, ordered by
/// (file id, span start) regardless of the source's walk order, with
/// duplicates (same trait name at the same extent) collapsed.
#[test]
fn supertypes_are_ordered_and_deduped() {
    let mut mock = MockSource::default();
    // Out-of-order sites across two files, plus one exact duplicate.
    mock.site(Some("Late"), Some("P"), 1, 0, 30);
    mock.site(Some("Early"), Some("P"), 0, 44, 70);
    mock.site(Some("Early"), Some("P"), 0, 44, 70);
    mock.item("P", HierarchyItemKind::Struct, 0, 0, 20);

    let members = compute_supertypes(&mock, "P");
    assert_eq!(members.len(), 2, "duplicate site collapses");
    assert_eq!(members[0].name, "Early");
    assert_eq!(members[0].location.file_id, FileId::from_usize(0));
    assert_eq!(members[1].name, "Late");
    assert_eq!(members[1].location.file_id, FileId::from_usize(1));
}

/// Inherent impls (`impl P`) record no trait: they are not supertypes of
/// the self type, so the walk must skip them rather than answer an
/// unnamed member.
#[test]
fn supertypes_exclude_inherent_impls() {
    let mut mock = MockSource::default();
    mock.site(None, Some("P"), 0, 0, 30);
    mock.item("P", HierarchyItemKind::Struct, 0, 31, 45);
    assert!(compute_supertypes(&mock, "P").is_empty());
}

/// A trait impl site whose self type is the queried type but whose trait
/// path or extent names nothing usable (blank name, zero-width extent)
/// cannot be shown as a jump target and is skipped honestly.
#[test]
fn supertypes_skip_unnamed_and_zero_width_sites() {
    let mut mock = MockSource::default();
    mock.site(Some(""), Some("P"), 0, 0, 30);
    mock.site(Some("Ghost"), Some("P"), 0, 40, 40);
    mock.site(Some("Real"), Some("P"), 0, 50, 80);
    let members = compute_supertypes(&mock, "P");
    assert_eq!(members.len(), 1);
    assert_eq!(members[0].name, "Real");
}

/// Subtypes of a trait: one member per impl self type, ordered by
/// (file id, span start), duplicates collapsed, inherent impls of other
/// traits excluded.
#[test]
fn subtypes_are_ordered_and_deduped() {
    let mut mock = MockSource::default();
    mock.site(Some("Greet"), Some("Q"), 1, 10, 40);
    mock.site(Some("Greet"), Some("P"), 0, 44, 74);
    mock.site(Some("Greet"), Some("P"), 0, 44, 74);
    mock.site(Some("Other"), Some("R"), 0, 80, 110);
    let members = compute_subtypes(&mock, "Greet");
    assert_eq!(members.len(), 2, "duplicate site collapses, Other excluded");
    assert_eq!(members[0].name, "P");
    assert_eq!(members[0].location.file_id, FileId::from_usize(0));
    assert_eq!(members[1].name, "Q");
    assert_eq!(members[1].location.file_id, FileId::from_usize(1));
}

/// An impl site of the queried trait whose self type names nothing
/// usable (non-path self types, zero-width extents) cannot be shown and
/// is skipped; a trait with no impls answers an empty walk.
#[test]
fn subtypes_skip_unnamed_and_zero_width_sites() {
    let mut mock = MockSource::default();
    mock.site(Some("Greet"), Some(""), 0, 0, 30);
    mock.site(Some("Greet"), Some("Ghost"), 0, 40, 40);
    assert!(compute_subtypes(&mock, "Greet").is_empty());

    mock.site(Some("Greet"), Some("P"), 0, 50, 80);
    let members = compute_subtypes(&mock, "Greet");
    assert_eq!(members.len(), 1);
    assert_eq!(
        members[0],
        HierarchyMember {
            name: "P".to_string(),
            location: DefinitionLocation {
                file_id: FileId::from_usize(0),
                span: span_of(0, 50, 80),
            },
        }
    );
}
