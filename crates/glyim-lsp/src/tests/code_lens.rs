//! Policy-level tests for the code-lens engine (LSP-CODE-LENS).
//!
//! Mirrors `document_link.rs` and `document_highlight.rs`: the *policy*
//! (which items carry a lens, and what the resolve answer says) is pinned
//! through an in-memory [`MockSource`] implementing
//! [`crate::code_lens::LensSource`], decoupled from any reference-graph
//! lock, source map, or wire type. The graph-backed production path and
//! the full wire requests (including the resolve round-trip) are covered
//! end-to-end in `code_lens_tests.rs`.

use crate::code_lens::{ItemLens, LensItem, LensSource, compute_code_lenses};
use std::collections::HashMap;

/// In-memory lens store: the policy-side test double. Items are listed in
/// source order; use counts are keyed by item name exactly like the
/// production graph lookup.
#[derive(Default)]
struct MockSource {
    items: Vec<LensItem>,
    uses: HashMap<String, usize>,
}

impl MockSource {
    fn add(&mut self, name: &str, lo: usize, hi: usize) {
        self.items.push(LensItem {
            name: name.to_string(),
            lo,
            hi,
        });
    }
    fn call(&mut self, name: &str, times: usize) {
        *self.uses.entry(name.to_string()).or_default() += times;
    }
}

impl LensSource for MockSource {
    fn lens_items(&self) -> Vec<LensItem> {
        self.items.clone()
    }
    fn use_count(&self, name: &str) -> usize {
        self.uses.get(name).copied().unwrap_or(0)
    }
}

/// One lens per item-level definition, in the source order the seam
/// reports them in; the use count is joined per item name.
#[test]
fn one_lens_per_item_in_source_order() {
    let mut mock = MockSource::default();
    mock.add("main", 3, 7);
    mock.add("Point", 18, 23);
    mock.call("main", 2);
    mock.call("Point", 5);

    let lenses = compute_code_lenses(&mock);
    assert_eq!(
        lenses,
        vec![
            ItemLens {
                name: "main".to_string(),
                lo: 3,
                hi: 7,
                uses: 2
            },
            ItemLens {
                name: "Point".to_string(),
                lo: 18,
                hi: 23,
                uses: 5
            },
        ]
    );
}

/// An item nobody references still answers a lens — with an honest
/// `uses == 0` — instead of disappearing from the response.
#[test]
fn zero_use_item_answers_zero_not_absent() {
    let mut mock = MockSource::default();
    mock.add("unused_helper", 0, 13);
    let lenses = compute_code_lenses(&mock);
    assert_eq!(lenses.len(), 1);
    assert_eq!(lenses[0].uses, 0);
    assert_eq!(lenses[0].name, "unused_helper");
}

/// Items the seam reports with an empty name carry no lens: there is
/// nothing to resolve against, and a lens on an unnamed extent would be
/// a lie.
#[test]
fn blank_item_names_are_skipped() {
    let mut mock = MockSource::default();
    mock.add("", 0, 5);
    mock.add("real", 8, 12);
    let lenses = compute_code_lenses(&mock);
    assert_eq!(lenses.len(), 1);
    assert_eq!(lenses[0].name, "real");
}

/// Two same-named items the seam reports (the graph only records one
/// top-level definition per name, but the policy does not have to trust
/// that) each get their own lens with the same joined count.
#[test]
fn same_named_items_each_get_their_own_lens() {
    let mut mock = MockSource::default();
    mock.add("helper", 0, 6);
    mock.add("helper", 40, 46);
    mock.call("helper", 3);
    let lenses = compute_code_lenses(&mock);
    assert_eq!(lenses.len(), 2);
    assert!(lenses.iter().all(|lens| lens.uses == 3));
    assert_eq!((lenses[0].lo, lenses[0].hi), (0, 6));
    assert_eq!((lenses[1].lo, lenses[1].hi), (40, 46));
}
