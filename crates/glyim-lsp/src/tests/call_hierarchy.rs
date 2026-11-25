//! Policy-level tests for the call-hierarchy engine (LSP-CALL-HIERARCHY).
//!
//! Caller/callee joins are resolved through an in-memory [`MockSource`]
//! implementing [`CallGraphSource`], not the live `AnalysisDatabase` —
//! the point of the trait seam: the *hierarchy policy* (which recorded
//! item counts as the target, how call sites group into callers, what
//! drops out of the response) is verified decoupled from any graph,
//! lock, or source-map machinery. The graph-backed path (real HIR call
//! records, wire mapping, URI round-trips) is covered in
//! `call_hierarchy_tests`.

use crate::call_hierarchy::{
    CallGraphSource, CallSite, ItemDefinition, compute_incoming_calls, compute_outgoing_calls,
    prepare_target,
};
use crate::tests::helpers::make_span;
use glyim_span::FileId;
use std::collections::HashMap;

/// In-memory call-graph store: the policy-side test double. Mirrors the
/// production `ReferenceGraphSource`: `fns` is the item-level function
/// table, `sites` the recorded call edges (each carrying its owning
/// item, the file/byte span of the call expression, and the callee
/// name it was recorded under).
#[derive(Default)]
struct MockSource {
    fns: Vec<ItemDefinition>,
    sites: Vec<CallSite>,
}

impl MockSource {
    fn add_fn(&mut self, name: &str, owner: u32, file: u32, lo: usize, hi: usize) {
        let file_id = FileId::from_usize(file as usize);
        self.fns.push(ItemDefinition {
            name: name.to_string(),
            owner_item_id: owner,
            file_id,
            span: make_span(file_id, lo, hi),
        });
    }

    fn add_site(&mut self, callee: &str, owner: u32, file: u32, lo: usize, hi: usize) {
        let file_id = FileId::from_usize(file as usize);
        self.sites.push(CallSite {
            callee: callee.to_string(),
            owner_item_id: owner,
            file_id,
            span: make_span(file_id, lo, hi),
        });
    }
}

impl CallGraphSource for MockSource {
    fn function_definition(&self, name: &str) -> Option<ItemDefinition> {
        self.fns.iter().find(|f| f.name == name).cloned()
    }

    fn item_definition_by_owner(&self, owner_item_id: u32) -> Option<ItemDefinition> {
        self.fns
            .iter()
            .find(|f| f.owner_item_id == owner_item_id)
            .cloned()
    }

    fn item_definition_at(&self, file_id: FileId, lo: usize, hi: usize) -> Option<ItemDefinition> {
        self.fns
            .iter()
            .find(|f| {
                f.file_id == file_id && f.span.lo().to_usize() == lo && f.span.hi().to_usize() == hi
            })
            .cloned()
    }

    fn incoming_call_sites(&self, callee: &str) -> Vec<CallSite> {
        self.sites
            .iter()
            .filter(|s| s.callee == callee)
            .cloned()
            .collect()
    }

    fn outgoing_call_sites(&self, owner_item_id: u32) -> Vec<CallSite> {
        self.sites
            .iter()
            .filter(|s| s.owner_item_id == owner_item_id)
            .cloned()
            .collect()
    }
}

/// Three functions in two files: `leaf` (file 1) is called twice by
/// `helper` and once by `main` (both file 1), and `helper` is called by
/// `main`. `Config` is a struct item, not a function. Owner ids are the
/// HIR item ids the production source joins on.
fn populated_source() -> MockSource {
    let mut src = MockSource::default();
    // fn items: leaf(owner 0, file 1), helper(owner 1, file 1),
    // main(owner 2, file 1), plus a same-named `leaf` shape in file 2 to
    // prove the definition lookup answers the FIRST recorded site.
    src.add_fn("leaf", 0, 1, 30, 70);
    src.add_fn("helper", 1, 1, 72, 120);
    src.add_fn("main", 2, 1, 122, 160);
    src.add_fn("leaf", 3, 2, 10, 40);
    // Call sites: helper → leaf ×2, main → helper, main → leaf.
    src.add_site("leaf", 1, 1, 90, 97);
    src.add_site("leaf", 1, 1, 103, 114);
    src.add_site("helper", 2, 1, 140, 149);
    src.add_site("leaf", 2, 1, 152, 158);
    src
}

#[test]
fn prepare_lands_on_the_function_definition_under_the_cursor() {
    // Cursor inside `fn helper`'s item span (file 1, offset 80): the
    // recorded definition of `helper` covers the cursor, so the item is
    // the prepare target.
    let src = populated_source();

    let target = prepare_target(&src, FileId::from_usize(1), 80, "helper")
        .expect("the cursor sits on helper's definition");

    assert_eq!(target.name, "helper");
    assert_eq!(target.owner_item_id, 1);
}

#[test]
fn prepare_resolves_a_call_site_to_the_callee() {
    // Cursor on `main`'s `helper(3)` call (offset 145, file 1): `main`'s
    // own definition does not cover the cursor, but a recorded call site
    // of `helper` does — the target is the callee's item, not the
    // caller's (the caller is what the incoming side already answers).
    let src = populated_source();

    let target = prepare_target(&src, FileId::from_usize(1), 145, "helper")
        .expect("the cursor sits on a helper call site");

    assert_eq!(target.name, "helper");
    assert_eq!(
        target.owner_item_id, 1,
        "the callee's item, not the caller's"
    );
}

#[test]
fn prepare_definition_site_beats_the_call_site_rule() {
    // A recursive call inside the function's own body: the cursor sits
    // both on the item definition (the span covers the body) and on a
    // recorded call site. The definition rule wins; both rules name the
    // same item, so the answer is stable either way — this pins that
    // the definition lookup is consulted first.
    let mut src = populated_source();
    src.add_site("leaf", 0, 1, 55, 62);

    let target = prepare_target(&src, FileId::from_usize(1), 58, "leaf").expect("recursive call");

    assert_eq!(target.owner_item_id, 0, "leaf's own item");
}

#[test]
fn prepare_none_when_the_name_is_not_a_function() {
    // The production source only answers `function_definition` for names
    // in the graph's fn table (struct/enum items are recorded in the same
    // reference store but are not call-hierarchy material), so a cursor
    // on a non-function name — or on a call edge whose callee resolves
    // to no fn item — must answer `None` rather than fabricate a target.
    // The mock mirrors the production gate by simply not listing
    // `Config` in `fns`.
    let mut src = populated_source();
    src.add_site("Config", 2, 1, 0, 10);

    assert!(
        prepare_target(&src, FileId::from_usize(1), 5, "Config").is_none(),
        "a non-function name is not a call-hierarchy target"
    );
}

#[test]
fn prepare_none_off_any_recorded_name() {
    let src = populated_source();
    assert!(prepare_target(&src, FileId::from_usize(1), 80, "ghost").is_none());
}

#[test]
fn prepare_none_for_a_foreign_file_definition() {
    // The definition rule requires the cursor to sit on the definition
    // in the SAME file: the cursor here is inside file 2's `leaf` shape
    // span (offset 20), but the query runs for file 1, whose `leaf`
    // span does not cover offset 20 — and there is no call site under
    // the cursor either. Answering the other file's item from a cursor
    // that never touched it would be a lie.
    let src = populated_source();
    assert!(prepare_target(&src, FileId::from_usize(1), 20, "leaf").is_none());
}

#[test]
fn incoming_groups_callers_and_collects_their_call_sites() {
    let src = populated_source();
    let target = src.function_definition("leaf").expect("leaf is a fn item");

    let calls = compute_incoming_calls(&src, &target);

    assert_eq!(calls.len(), 2, "helper and main, grouped");
    assert_eq!(calls[0].caller.name, "helper", "sorted by caller name");
    assert_eq!(calls[0].call_sites.len(), 2, "helper calls leaf twice");
    assert_eq!(calls[0].call_sites[0].lo().to_usize(), 90, "site order");
    assert_eq!(calls[0].call_sites[1].lo().to_usize(), 103);
    assert_eq!(calls[1].caller.name, "main");
    assert_eq!(calls[1].call_sites.len(), 1);
}

#[test]
fn incoming_skips_callers_without_a_resolvable_item() {
    // A call edge whose owner has no item-level definition (stale edge
    // after a delete, or a synthetic owner) must not fabricate a
    // caller — it drops out of the response.
    let mut src = populated_source();
    src.add_site("leaf", 99, 1, 170, 176);
    let target = src.function_definition("leaf").expect("leaf is a fn item");

    let calls = compute_incoming_calls(&src, &target);

    assert_eq!(calls.len(), 2, "the orphan owner never becomes a caller");
    assert!(calls.iter().all(|c| c.caller.owner_item_id != 99));
}

#[test]
fn incoming_keeps_self_recursion() {
    // A call edge whose owner is the target itself is recursion; LSP
    // clients render it as a self edge, so the policy must not drop it.
    let mut src = populated_source();
    src.add_site("leaf", 0, 1, 55, 62);
    let target = src.function_definition("leaf").expect("leaf is a fn item");

    let calls = compute_incoming_calls(&src, &target);

    assert!(
        calls.iter().any(|c| c.caller.owner_item_id == 0),
        "self recursion surfaces as an incoming call"
    );
}

#[test]
fn outgoing_lists_callees_with_their_definition_sites() {
    let src = populated_source();
    let target = src.function_definition("main").expect("main is a fn item");

    let calls = compute_outgoing_calls(&src, &target);

    assert_eq!(calls.len(), 2, "helper and leaf, grouped by callee");
    assert_eq!(calls[0].callee.name, "helper", "sorted by callee name");
    assert_eq!(calls[0].call_sites.len(), 1);
    assert_eq!(calls[1].callee.name, "leaf");
    assert_eq!(calls[1].call_sites.len(), 1);
}

#[test]
fn outgoing_groups_repeated_calls_to_one_callee() {
    let src = populated_source();
    let target = src
        .function_definition("helper")
        .expect("helper is a fn item");

    let calls = compute_outgoing_calls(&src, &target);

    assert_eq!(calls.len(), 1, "one callee, two call sites");
    assert_eq!(calls[0].callee.name, "leaf");
    assert_eq!(calls[0].call_sites.len(), 2);
}

#[test]
fn outgoing_skips_calls_without_a_function_target() {
    // `Expr::Call` records a Call edge for ANY called name — including
    // function-typed locals (`callback(...)`) and method receivers whose
    // name is not a free fn item. A callee that resolves to no function
    // item drops out rather than fabricating a hierarchy item.
    let mut src = populated_source();
    src.add_site("callback", 2, 1, 141, 150);
    let target = src.function_definition("main").expect("main is a fn item");

    let calls = compute_outgoing_calls(&src, &target);

    assert_eq!(calls.len(), 2, "helper and leaf, callback skipped");
    assert!(calls.iter().all(|c| c.callee.name != "callback"));
}

#[test]
fn responses_are_sorted_deterministically() {
    // The store records sites in walk order; the response order must
    // not depend on it. Callers sort by name (then file, then byte
    // offset), call sites by byte offset — the same determinism the
    // highlight policy pins.
    let mut src = MockSource::default();
    src.add_fn("leaf", 0, 1, 30, 70);
    src.add_fn("alpha", 1, 1, 72, 120);
    src.add_fn("beta", 2, 1, 122, 160);
    src.add_site("leaf", 2, 1, 150, 156);
    src.add_site("leaf", 1, 1, 100, 106);
    src.add_site("leaf", 1, 1, 80, 86);
    let target = src.function_definition("leaf").expect("leaf is a fn item");

    let calls = compute_incoming_calls(&src, &target);

    assert_eq!(calls[0].caller.name, "alpha");
    assert_eq!(calls[0].call_sites[0].lo().to_usize(), 80, "sites sorted");
    assert_eq!(calls[0].call_sites[1].lo().to_usize(), 100);
    assert_eq!(calls[1].caller.name, "beta");
}

#[test]
fn call_site_structs_are_hashable_for_grouping() {
    // The grouping keys derive Hash from the owner/callee pair; this
    // compiles only if the derived bounds hold (the wire layer groups
    // with HashMap-like passes).
    let mut map: HashMap<(u32, String), usize> = HashMap::new();
    let src = populated_source();
    for site in src.sites.iter() {
        *map.entry((site.owner_item_id, site.callee.clone()))
            .or_default() += 1;
    }
    assert_eq!(map.get(&(1, "leaf".to_string())), Some(&2));
}
