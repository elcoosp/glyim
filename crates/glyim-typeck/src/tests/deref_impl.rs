//! Phase 5 (GLYIM_DESTUB_PLAN): integration test that a real
//! `impl Deref for X { type Target = Y; }` HIR item is picked up by
//! `populate_deref_registry` during `typeck_crate` and lands in the frozen
//! `TyCtx::deref_registry`, so `deref_ty` (consulted by method autoderef)
//! resolves through it.

use glyim_core::arena::IndexVec;
use glyim_core::def_id::{AdtId, CrateId, LocalDefId};
use glyim_core::interner::{Interner, Name};
use glyim_core::path::PathKind;
use glyim_core::primitives::Visibility;
use glyim_def_map::{CrateDefMap, ItemScope, ModuleData, ModuleId, ModuleOrigin};
use glyim_hir::{AssociatedTy, ImplItem, Item, ItemId, ItemKind, Path, PathSegment, TypeRef};
use glyim_solve::SimpleTraitSolver;
use glyim_span::Span;
use glyim_type::ty::TyKind;
use glyim_type::{TyCtx, TyCtxMut};

use crate::{TypeckResult, typeck_crate};

fn intern_name(interner: &Interner, s: &str) -> Name {
    interner.intern(s)
}

fn build_def_map(interner: &mut Interner, krate: CrateId, type_names: &[&str]) -> CrateDefMap {
    let mut scope = ItemScope::default();
    for (i, &name_str) in type_names.iter().enumerate() {
        let name = interner.intern(name_str);
        scope.types.insert(
            name,
            (
                LocalDefId::from_raw(i as u32),
                Visibility::Public,
                Span::DUMMY,
            ),
        );
    }
    let root_id = ModuleId::from_raw(0);
    let root_data = ModuleData {
        parent: None,
        children: vec![],
        scope,
        origin: ModuleOrigin::CrateRoot,
        span: Span::DUMMY,
        def_id: LocalDefId::from_raw(0),
        visibility: Visibility::Public,
    };
    let mut modules = IndexVec::new();
    modules.push(root_data);
    CrateDefMap {
        root: root_id,
        modules,
        krate,
        interner: interner.clone(),
        variant_map: Default::default(),
        max_local_def_id: 0,
    }
}

#[test]
fn impl_deref_for_adt_populates_registry() {
    let mut interner = Interner::new();
    let wrapper_name = intern_name(&interner, "Wrapper");
    let deref_name = intern_name(&interner, "Deref");
    let target_name = intern_name(&interner, "Target");
    let i32_name = intern_name(&interner, "i32");

    // `impl Deref for Wrapper { type Target = i32; }`
    let impl_item = ImplItem {
        trait_ref: Some(Path {
            segments: vec![PathSegment {
                name: deref_name,
                generic_args: None,
            }],
            kind: PathKind::Plain,
        }),
        self_ty: TypeRef::Path(Path::from_single(wrapper_name)),
        methods: vec![],
        generic_params: vec![],
        where_clauses: vec![],
        associated_types: vec![AssociatedTy {
            name: target_name,
            bounds: vec![],
            default: Some(TypeRef::Path(Path::from_single(i32_name))),
        }],
    };

    let mut items = IndexVec::new();
    items.push(Item {
        id: ItemId::from_raw(0),
        name: wrapper_name,
        kind: ItemKind::Impl(impl_item),
        visibility: Visibility::Inherited,
        span: Span::DUMMY,
    });

    let hir = glyim_hir::CrateHir {
        items,
        bodies: IndexVec::new(),
        body_owners: IndexVec::new(),
        interner: Default::default(),
    };

    // `Wrapper` must resolve to a registered ADT self type (def-map local id 0).
    let def_map = build_def_map(&mut interner, CrateId::from_raw(0), &["Wrapper"]);

    let ctx: TyCtxMut = TyCtxMut::new(interner);
    let trait_ctx = glyim_solve::TraitContext::new();
    let mut solver = SimpleTraitSolver::new(&trait_ctx);
    let (frozen, result): (TyCtx, TypeckResult) = typeck_crate(ctx, &def_map, &hir, &mut solver);

    // The registry must contain a template keyed by the `Wrapper` AdtId (0).
    let wrapper_id = AdtId::from_raw(0);
    let target_ty = frozen
        .deref_registry_target_for(wrapper_id)
        .expect("impl Deref for Wrapper must populate the deref registry");
    assert!(
        matches!(frozen.ty_kind(target_ty), TyKind::Int(_)),
        "Deref::Target of Wrapper must resolve to i32, got {:?}",
        frozen.ty_kind(target_ty)
    );

    // No Deref-related diagnostics should be emitted by the registry
    // population itself. (A minimal harness that does not define the `Deref`
    // trait may still surface an unrelated "unresolved trait" diagnostic from
    // the coherence pass; that is outside the scope of this feature test.)
    for d in &result.diagnostics {
        let text = format!("{:?}", d);
        assert!(
            !text.contains("deref registry") && !text.contains("populate_deref"),
            "unexpected Deref-registry diagnostic: {:?}",
            d
        );
    }
}
