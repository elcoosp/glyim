//! Invariants the builtin **trait** registry must satisfy at all times — the
//! trait-side mirror of `builtin_adts.rs`' table and of the builtin-method
//! registry guards in `builtin_registry.rs`.
//!
//! The bug class guarded against here is **parallel-table drift**: the
//! builtin trait table used to be hand-copied in two places
//! (`TyCtxMut::register_builtin_ranges` for the resolver/typeck side and the
//! pipeline's solver bootstrap), and the copies had already diverged once
//! (`Default`/`Any` missing from the solver). With a single registry the
//! compiler cannot compile a consumer that forgot an entry, and the tests
//! below pin the registry to what both contexts actually register.
//!
//! 1. **Reserved-id discipline** — trait ids live in `2001..3000`, are
//!    unique, and are disjoint from the builtin `AdtId` reservation
//!    (`1000..2000`, `BuiltinAdt::adt_id`).
//! 2. **Round-trip identity** — `from_name` / `from_trait_id` invert the
//!    id/name tables exactly.
//! 3. **Context parity** — a fully built `TyCtx` registers *exactly* the
//!    registry's set: every name resolves to its reserved id, and no extra
//!    builtin-range trait sneaks in via a hand edit.
//! 4. **Lang-trait flag discipline** — exactly the five structural traits
//!    (`Copy`/`Sized`/`Send`/`Sync`/`Unpin`) are flagged as lang traits;
//!    the solver maps precisely those five.

use glyim_core::def_id::TraitDefId;
use glyim_test::test_ty_ctx;

use crate::builtin_adts::BuiltinAdt;
use crate::builtin_traits::{BuiltinTraitDef, BuiltinTraitSink};

/// A collecting sink used to observe exactly what `register_all` would
/// drive into a real context, without building one.
#[derive(Default)]
struct RecordingSink {
    registered: Vec<(u32, &'static str)>,
}

impl BuiltinTraitSink for RecordingSink {
    fn register_builtin_trait(&mut self, trait_id: u32, name: &'static str) {
        self.registered.push((trait_id, name));
    }
}

#[test]
fn all_trait_ids_are_unique_and_in_the_reserved_range() {
    let mut seen = std::collections::HashSet::new();
    for t in BuiltinTraitDef::ALL {
        let id = t.trait_id();
        assert!(
            (2001..3000).contains(&id),
            "{} has TraitDefId {id} outside the reserved 2001..3000 range",
            t.name(),
        );
        assert!(
            seen.insert(id),
            "duplicate TraitDefId {id} (from {})",
            t.name()
        );
    }
}

#[test]
fn trait_ids_are_disjoint_from_builtin_adt_ids() {
    let adt_ids: std::collections::HashSet<u32> =
        BuiltinAdt::ALL.iter().map(|b| b.adt_id()).collect();
    for t in BuiltinTraitDef::ALL {
        assert!(
            !adt_ids.contains(&t.trait_id()),
            "{} (trait id {}) collides with a reserved builtin AdtId — the two \
             id spaces must stay disjoint or resolver lookups silently alias",
            t.name(),
            t.trait_id(),
        );
    }
}

#[test]
fn all_names_are_unique_and_round_trip() {
    let mut seen = std::collections::HashSet::new();
    for t in BuiltinTraitDef::ALL {
        let n = t.name();
        assert!(seen.insert(n), "duplicate builtin trait name {n}");
        assert_eq!(
            BuiltinTraitDef::from_name(n),
            Some(*t),
            "from_name round-trip failed for {n}"
        );
        assert_eq!(
            BuiltinTraitDef::from_trait_id(t.trait_id()),
            Some(*t),
            "from_trait_id round-trip failed for {} ({})",
            n,
            t.trait_id(),
        );
    }
    assert_eq!(BuiltinTraitDef::from_name("NotATrait"), None);
    assert_eq!(BuiltinTraitDef::from_trait_id(1999), None);
    assert_eq!(BuiltinTraitDef::from_trait_id(3001), None);
}

#[test]
fn ids_are_contiguous_from_2001() {
    // The id space is published API (the pipeline's solver bootstrap and
    // the resolver both hand these ids to diagnostics); gaps in the middle
    // would suggest a deleted entry that downstream fixtures may still
    // reference. Adding a *new* id at the end is fine; this pins the
    // existing block.
    let ids: Vec<u32> = BuiltinTraitDef::ALL.iter().map(|t| t.trait_id()).collect();
    let expected: Vec<u32> = (2001..2001 + BuiltinTraitDef::ALL.len() as u32).collect();
    assert_eq!(ids, expected, "builtin trait ids must stay contiguous");
}

#[test]
fn exactly_the_five_structural_traits_are_lang_traits() {
    let lang: Vec<&str> = BuiltinTraitDef::ALL
        .iter()
        .filter(|t| t.is_lang_trait())
        .map(|t| t.name())
        .collect();
    assert_eq!(lang, vec!["Copy", "Sized", "Send", "Sync", "Unpin"]);
}

#[test]
fn register_all_drives_the_whole_table_in_id_order() {
    let mut sink = RecordingSink::default();
    BuiltinTraitDef::register_all(&mut sink);
    assert_eq!(
        sink.registered.len(),
        BuiltinTraitDef::ALL.len(),
        "register_all dropped entries"
    );
    let mut ids: Vec<u32> = sink.registered.iter().map(|(id, _)| *id).collect();
    let sorted = ids.clone();
    ids.sort_unstable();
    assert_eq!(ids, sorted, "register_all must visit entries in id order");
    for (id, name) in sink.registered {
        let t = BuiltinTraitDef::from_trait_id(id)
            .unwrap_or_else(|| panic!("register_all produced unknown id {id}"));
        assert_eq!(t.name(), name, "register_all mismatched name for id {id}");
    }
}

/// The drift-killer: the *frozen* type context must register exactly the
/// registry's table — every builtin name resolves to its reserved id, and
/// nothing else occupies the builtin trait range. This is the test that
/// would have caught `Default`/`Any` silently missing from a consumer.
#[test]
fn ty_ctx_registration_matches_the_registry_exactly() {
    let ctx = test_ty_ctx().freeze();
    let resolver = ctx.resolver();
    for t in BuiltinTraitDef::ALL {
        let name = resolver.intern(t.name());
        let got = ctx.trait_by_name.get(&name).copied();
        assert_eq!(
            got,
            Some(TraitDefId::from_raw(t.trait_id())),
            "TyCtx registered `{}` under {:?}, expected the reserved id {} — \
             parallel-table drift between the registry and register_builtin_ranges",
            t.name(),
            got.map(|id| id.to_raw()),
            t.trait_id(),
        );
    }
    // Inverse direction: no builtin-range id in the context that the
    // registry does not know about.
    for (name, tid) in &ctx.trait_by_name {
        let raw = tid.to_raw();
        if (2001..3000).contains(&raw) {
            let t = BuiltinTraitDef::from_trait_id(raw).unwrap_or_else(|| {
                panic!("TyCtx registers builtin trait id {raw} that the registry does not know")
            });
            assert_eq!(
                resolver.resolve(*name),
                t.name(),
                "TyCtx maps name `{}` onto {raw} but the registry calls id {raw} `{}`",
                resolver.resolve(*name),
                t.name(),
            );
        }
    }
}
