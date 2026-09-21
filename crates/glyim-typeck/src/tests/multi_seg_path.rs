//! Multi‑segment path resolution for struct *patterns* (plan §9.4).
//!
//! A struct pattern such as `zoo::Point { x }` names the ADT through the module
//! tree rather than the crate root. `resolve_path_to_adt_id` walks the def map's
//! module children and resolves the final segment in the reached module's scope;
//! `check_pattern` now routes every struct path through it (single‑ and
//! multi‑segment), so the old "multi‑segment struct paths not yet implemented"
//! diagnostic is gone.
use glyim_core::arena::IndexVec;
use glyim_core::def_id::{AdtId, CrateId, LocalDefId};
use glyim_def_map::{CrateDefMap, ItemScope, ModuleData, ModuleId, ModuleOrigin};
use glyim_hir::Path;
use glyim_span::Span;
use glyim_type::TyCtxMut;

use crate::tyconv::resolve_path_to_adt_id;

/// Build a def map with a nested module `zoo` containing an ADT `Point`.
fn nested_def_map(interner: &mut glyim_core::interner::Interner) -> CrateDefMap {
    let zoo = interner.intern("zoo");
    let point = interner.intern("Point");

    // Root module: just contains the `zoo` submodule.
    let mut root_scope = ItemScope::default();
    // The submodule id is 1.
    root_scope
        .types
        .insert(zoo, (LocalDefId::from_raw(1), Visibility::Public, Span::DUMMY));

    let root = ModuleData {
        parent: None,
        children: vec![(zoo, ModuleId::from_raw(1))],
        scope: root_scope,
        origin: ModuleOrigin::CrateRoot,
        span: Span::DUMMY,
        def_id: LocalDefId::from_raw(0),
        visibility: Visibility::Public,
    };

    // Nested module `zoo`: contains the `Point` ADT (LocalDefId 2).
    let mut zoo_scope = ItemScope::default();
    zoo_scope
        .types
        .insert(point, (LocalDefId::from_raw(2), Visibility::Public, Span::DUMMY));
    let zoo_mod = ModuleData {
        parent: Some(ModuleId::from_raw(0)),
        children: vec![],
        scope: zoo_scope,
        origin: ModuleOrigin::CrateRoot,
        span: Span::DUMMY,
        def_id: LocalDefId::from_raw(1),
        visibility: Visibility::Public,
    };

    let mut modules = IndexVec::new();
    modules.push(root);
    modules.push(zoo_mod);

    CrateDefMap {
        root: ModuleId::from_raw(0),
        modules,
        krate: CrateId::from_raw(0),
        interner: interner.clone(),
    variant_map: Default::default(),
    max_local_def_id: 0,
    }
}

#[test]
fn multi_segment_struct_pattern_resolves_to_adt() {
    let mut interner = glyim_core::interner::Interner::new();
    let def_map = nested_def_map(&mut interner);
    let ctx = TyCtxMut::new(interner.clone());

    let zoo = interner.intern("zoo");
    let point = interner.intern("Point");
    let mut path = Path::from_single(point);
    path.kind = glyim_core::path::PathKind::Plain;
    path.segments.insert(
        0,
        glyim_hir::PathSegment {
            name: zoo,
            generic_args: None,
        },
    );

    let resolved = resolve_path_to_adt_id(&ctx, &def_map, &path);
    assert!(
        resolved.is_some(),
        "multi-segment struct path `zoo::Point` must resolve"
    );
    assert_eq!(
        resolved.unwrap(),
        AdtId::from_raw(2),
        "must resolve to the nested ADT's id"
    );
}

#[test]
fn single_segment_struct_pattern_still_resolves() {
    let interner = glyim_core::interner::Interner::new();
    let point = interner.intern("Point");
    // Single-segment: register directly in root scope.
    let mut root_scope = ItemScope::default();
    root_scope
        .types
        .insert(point, (LocalDefId::from_raw(7), Visibility::Public, Span::DUMMY));
    let root = ModuleData {
        parent: None,
        children: vec![],
        scope: root_scope,
        origin: ModuleOrigin::CrateRoot,
        span: Span::DUMMY,
        def_id: LocalDefId::from_raw(0),
        visibility: Visibility::Public,
    };
    let mut modules = IndexVec::new();
    modules.push(root);
    let def_map = CrateDefMap {
        root: ModuleId::from_raw(0),
        modules,
        krate: CrateId::from_raw(0),
        interner: interner.clone(),
        variant_map: Default::default(),
        max_local_def_id: 0,
    };
    let ctx = TyCtxMut::new(interner.clone());

    let path = Path::from_single(point);
    let resolved = resolve_path_to_adt_id(&ctx, &def_map, &path);
    assert_eq!(resolved, Some(AdtId::from_raw(7)));
}

use glyim_core::primitives::Visibility;
