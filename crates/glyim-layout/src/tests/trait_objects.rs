use crate::{FieldsShape, LayoutComputer, SimpleLayoutComputer, Size};
use glyim_core::{TargetInfo, TraitDefId};
use glyim_test::with_fresh_ty_ctx;
use glyim_type::*;

fn make_dyn_tykind(ctx: &mut TyCtxMut) -> TyKind {
    let empty_subst = ctx.intern_substitution(vec![]);
    let trait_ref = TraitRef {
        def_id: TraitDefId::from_usize(0),
        substs: empty_subst,
    };
    let predicate = Predicate::Trait(TraitPredicate {
        trait_ref,
        polarity: ImplPolarity::Positive,
    });
    let box_predicates: Box<[Predicate]> = Box::new([predicate]);
    let bound_vars: Box<[BoundVariableKind]> = vec![].into();
    let binder = Binder::bind(box_predicates, bound_vars);
    TyKind::Dynamic(binder, Region::Erased)
}

#[test]
fn vtable_size_and_alignment() {
    let (ctx, dyn_ty) = with_fresh_ty_ctx(|ctx| {
        let kind = make_dyn_tykind(ctx);
        ctx.mk_ty(kind)
    });
    let computer = SimpleLayoutComputer::new(&ctx, TargetInfo::x86_64());
    let layout = computer
        .layout_of(dyn_ty)
        .expect("layout_of(dyn Trait) should succeed");
    let ptr_size = computer.ptr_size();
    // dyn Trait fat pointer: data pointer + vtable pointer
    assert_eq!(
        layout.size,
        Size::bytes(ptr_size.0 * 2),
        "size = two pointers"
    );
    assert_eq!(
        layout.align,
        computer.ptr_align(),
        "alignment = pointer alignment"
    );
    assert!(!layout.is_unsized, "dyn Trait fat pointer is sized");
    match &layout.fields {
        FieldsShape::Arbitrary { offsets } => {
            assert_eq!(offsets.len(), 2, "two fields");
            assert_eq!(
                offsets[FieldIdx::from_usize(0)],
                Size::ZERO,
                "data ptr at offset 0"
            );
            assert_eq!(
                offsets[FieldIdx::from_usize(1)],
                Size::bytes(ptr_size.0),
                "vtable ptr at offset ptr_size"
            );
        }
        _ => panic!("expected Arbitrary fields"),
    }
}
