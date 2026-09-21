//! VTable layout computation tests

use crate::vtable::*;
use crate::*;
use glyim_core::primitives::*;
use glyim_test::with_fresh_ty_ctx;

#[test]
fn s15_vtable_memory_size_no_methods() {
    let (_ctx, concrete_ty) = with_fresh_ty_ctx(|c| c.bool_ty());
    let layout = VTableLayout {
        trait_def_id: glyim_core::TraitDefId::from_raw(1),
        concrete_ty,
        size: Size::bytes(1),
        align: Align::ONE,
        drop_fn: None,
        methods: vec![],
    };
    let mem = layout.memory_size(8);
    assert_eq!(mem.size, 24, "3 pointers * 8 bytes = 24");
    assert_eq!(mem.align, 8);
}

#[test]
fn s15_vtable_memory_size_with_methods() {
    let (_ctx, (concrete_ty, sig, foo_name, bar_name)) = with_fresh_ty_ctx(|c| {
        let i32_ty = c.mk_ty(glyim_type::TyKind::Int(IntTy::I32));
        let bool_ty = c.bool_ty();
        let inputs = c.intern_substitution(vec![glyim_type::GenericArg::Ty(i32_ty)]);
        let sig = glyim_type::FnSig {
            inputs,
            output: bool_ty,
            c_variadic: false,
            unsafety: Safety::Safe,
            abi: Abi::Glyim,
        };
        let foo_name = c.resolver().intern("foo");
        let bar_name = c.resolver().intern("bar");
        (bool_ty, sig, foo_name, bar_name)
    });
    let layout = VTableLayout {
        trait_def_id: glyim_core::TraitDefId::from_raw(1),
        concrete_ty,
        size: Size::bytes(1),
        align: Align::ONE,
        drop_fn: None,
        methods: vec![
            VTableEntry {
                name: foo_name,
                sig: sig.clone(),
                fn_def_id: glyim_core::FnDefId::from_raw(10),
            },
            VTableEntry {
                name: bar_name,
                sig,
                fn_def_id: glyim_core::FnDefId::from_raw(11),
            },
        ],
    };
    let mem = layout.memory_size(8);
    assert_eq!(mem.size, 40, "5 pointers * 8 = 40");
    assert_eq!(mem.align, 8);
}

#[test]
fn s15_vtable_method_offset() {
    let (_ctx, concrete_ty) = with_fresh_ty_ctx(|c| c.bool_ty());
    let layout = VTableLayout {
        trait_def_id: glyim_core::TraitDefId::from_raw(1),
        concrete_ty,
        size: Size::bytes(1),
        align: Align::ONE,
        drop_fn: None,
        methods: vec![],
    };
    assert_eq!(layout.method_offset(0, 8), 24);
    assert_eq!(layout.method_offset(1, 8), 32);
    assert_eq!(layout.method_offset(0, 4), 12);
}

#[test]
fn s15_vtable_computer_populates_methods_from_trait_def() {
    use glyim_test::test_ty_ctx;

    let mut ctx_mut = test_ty_ctx();
    let concrete_ty = ctx_mut.bool_ty();
    // Register a trait (id 7) with two methods so vtable_of can derive the
    // method slots from the real trait definition.
    let i32_ty = ctx_mut.mk_ty(glyim_type::TyKind::Int(IntTy::I32));
    let inputs = ctx_mut.intern_substitution(vec![glyim_type::GenericArg::Ty(i32_ty)]);
    let sig = glyim_type::FnSig {
        inputs,
        output: ctx_mut.bool_ty(),
        c_variadic: false,
        unsafety: Safety::Safe,
        abi: Abi::Glyim,
    };
    let foo_name = ctx_mut.resolver().intern("foo");
    let bar_name = ctx_mut.resolver().intern("bar");
    let trait_def = glyim_type::TraitDef {
        name: ctx_mut.resolver().intern("Foo"),
        methods: vec![
            glyim_type::MethodDef {
                name: foo_name,
                sig: sig.clone(),
                fn_def_id: Some(glyim_core::FnDefId::from_raw(10)),
            },
            glyim_type::MethodDef {
                name: bar_name,
                sig,
                fn_def_id: Some(glyim_core::FnDefId::from_raw(11)),
            },
        ],
        associated_types: vec![],
    };
    ctx_mut.register_trait_def(glyim_core::TraitDefId::from_raw(7), trait_def);
    let ctx = ctx_mut.freeze();

    let computer = SimpleLayoutComputer::new(&ctx, TargetInfo::x86_64());
    let vtable = computer
        .vtable_of(glyim_core::TraitDefId::from_raw(7), concrete_ty)
        .expect("vtable_of with a registered trait should return Some");

    assert_eq!(vtable.methods.len(), 2, "methods derived from trait def");
    assert_eq!(vtable.methods[0].name, foo_name);
    assert_eq!(
        vtable.methods[0].fn_def_id,
        glyim_core::FnDefId::from_raw(10)
    );
    assert_eq!(vtable.methods[1].name, bar_name);
    assert_eq!(
        vtable.methods[1].fn_def_id,
        glyim_core::FnDefId::from_raw(11)
    );
    // One method slot per trait method: 3 metadata + 2 methods.
    let mem = vtable.memory_size(8);
    assert_eq!(mem.size, 40);
}

// Plan §10.1: an unresolvable trait (no registered `TraitDef`) must surface as
// a hard `LayoutError::UnknownTrait` rather than a silently-empty vtable that
// would miscall through null slots at runtime.
#[test]
fn s15_vtable_unknown_trait_is_hard_error() {
    let (_ctx, concrete_ty) = with_fresh_ty_ctx(|c| c.bool_ty());
    let computer = SimpleLayoutComputer::new(&_ctx, TargetInfo::x86_64());
    let result = computer.vtable_of(glyim_core::TraitDefId::from_raw(999), concrete_ty);
    assert!(
        matches!(result, Err(crate::LayoutError::UnknownTrait(_))),
        "missing trait must be a hard error, got {:?}",
        result
    );
}
