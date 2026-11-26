use glyim_core::def_id::TraitDefId;
use glyim_core::interner::Interner;
use glyim_diag::GlyimDiagnostic;
use glyim_span::Span;
use glyim_type::{GenericArg, ProjectionTy, TraitRef, TyCtxMut};

/// Build a projection and call the diagnostic helper directly.
#[test]
fn unresolved_projection_diagnostic() {
    let mut ctx = TyCtxMut::new(Interner::default());
    let name = ctx.resolver().intern("Item");
    let trait_id = TraitDefId::from_usize(42);
    let self_ty = ctx.bool_ty();
    let substs = ctx.intern_substitution(vec![GenericArg::Ty(self_ty)]);
    let trait_ref = TraitRef {
        def_id: trait_id,
        substs,
    };
    let proj = ProjectionTy {
        trait_ref,
        item_name: name,
    };

    // Emit diagnostic (simulating what the type checker will do)
    let msg = format!(
        "cannot resolve projection <_ as Trait{}>::{}",
        proj.trait_ref.def_id.to_raw(),
        ctx.name_str(proj.item_name)
    );
    let diag = GlyimDiagnostic::type_error(Span::DUMMY, msg);
    assert!(diag.message.contains("cannot resolve projection"));
    assert!(diag.message.contains("Trait42"));
    assert!(diag.message.contains("Item"));
}
