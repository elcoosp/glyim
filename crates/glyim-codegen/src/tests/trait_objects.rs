use crate::*;
use glyim_core::{CrateId, DefId, FnDefId, Interner, LocalDefId, TargetInfo, TraitDefId};
use glyim_test::with_fresh_ty_ctx;
use glyim_type::*;
use std::sync::Arc;

fn make_dyn_ty(ctx: &mut TyCtxMut) -> Ty {
    let empty_subst = ctx.intern_substitution(vec![]);
    let trait_ref = TraitRef {
        def_id: TraitDefId::from_raw(0),
        substs: empty_subst,
    };
    let predicate = Predicate::Trait(TraitPredicate {
        trait_ref,
        polarity: ImplPolarity::Positive,
    });
    let box_predicates: Box<[Predicate]> = Box::new([predicate]);
    let bound_vars: Box<[BoundVariableKind]> = vec![].into();
    let binder = Binder::bind(box_predicates, bound_vars);
    let kind = TyKind::Dynamic(binder, Region::Erased);
    ctx.mk_ty(kind)
}

#[test]
fn create_trait_object_from_concrete_type() {
    let (_, is_dynamic) = with_fresh_ty_ctx(|ctx| {
        let dyn_ty = make_dyn_ty(ctx);
        matches!(ctx.ty_kind(dyn_ty), TyKind::Dynamic(_, _))
    });
    assert!(is_dynamic, "trait object type should be Dynamic");
}

#[test]
fn call_method_through_trait_object() {
    let (ctx, dyn_ty) = with_fresh_ty_ctx(make_dyn_ty);

    let local0 = LocalIdx::from_raw(0);
    let local1 = LocalIdx::from_raw(1);
    let mut local_decls = glyim_core::IndexVec::new();
    local_decls.push(LocalDecl {
        ty: dyn_ty,
        mutability: glyim_core::Mutability::Mut,
        source_info: SourceInfo::new(glyim_span::Span::DUMMY),
    });
    local_decls.push(LocalDecl {
        ty: dyn_ty,
        mutability: glyim_core::Mutability::Not,
        source_info: SourceInfo::new(glyim_span::Span::DUMMY),
    });

    let mut basic_blocks = glyim_core::IndexVec::new();
    let term = Terminator {
        kind: TerminatorKind::Call {
            func: Operand::Copy(Place::new(local0)),
            args: vec![],
            destination: Place::new(local1),
            target: Some(BasicBlockIdx::from_raw(1)),
            cleanup: None,
        },
        source_info: SourceInfo::new(glyim_span::Span::DUMMY),
    };
    let mut block0 = BasicBlockData::new(term);
    block0.is_cleanup = false;
    basic_blocks.push(block0);

    let ret_term = Terminator {
        kind: TerminatorKind::Return,
        source_info: SourceInfo::new(glyim_span::Span::DUMMY),
    };
    basic_blocks.push(BasicBlockData::new(ret_term));

    let body = Arc::new(Body {
        owner: DefId::new(CrateId::from_raw(0), LocalDefId::from_raw(0)),
        basic_blocks,
        locals: local_decls,
        arg_count: 0,
        return_ty: glyim_type::Ty::UNIT,
        span: glyim_span::Span::DUMMY,
        var_debug_info: vec![],
    });

    let backend = BytecodeBackend::with_ty_ctx(
        std::sync::Arc::new(glyim_type::TyCtxMut::new(glyim_core::Interner::default()).freeze()),
        glyim_core::TargetInfo::default(),
    );
    let result = backend.generate_function(&body);
    assert!(
        result.is_ok(),
        "generate_function should succeed: {:?}",
        result.err()
    );
    let bc = result.unwrap();
    assert!(
        bc.contains(&0x22),
        "bytecode should contain OP_CALL_INDIRECT"
    );
    assert!(
        !bc.contains(&0x1B),
        "bytecode should NOT contain OP_CALL for indirect call"
    );
    drop(ctx);
}

#[test]
fn upcast_to_supertrait_object() {
    let (ctx, dyn_ty) = with_fresh_ty_ctx(make_dyn_ty);

    let _local0 = LocalIdx::from_raw(0);
    let local1 = LocalIdx::from_raw(1);
    let mut local_decls = glyim_core::IndexVec::new();
    local_decls.push(LocalDecl {
        ty: dyn_ty,
        mutability: glyim_core::Mutability::Mut,
        source_info: SourceInfo::new(glyim_span::Span::DUMMY),
    });
    local_decls.push(LocalDecl {
        ty: dyn_ty,
        mutability: glyim_core::Mutability::Not,
        source_info: SourceInfo::new(glyim_span::Span::DUMMY),
    });

    let mut basic_blocks = glyim_core::IndexVec::new();
    let term = Terminator {
        kind: TerminatorKind::Call {
            func: Operand::Constant(MirConst {
                kind: MirConstKind::Fn(FnDefId::from_raw(42), Substitution::empty()),
                ty: dyn_ty,
                span: glyim_span::Span::DUMMY,
            }),
            args: vec![],
            destination: Place::new(local1),
            target: Some(BasicBlockIdx::from_raw(1)),
            cleanup: None,
        },
        source_info: SourceInfo::new(glyim_span::Span::DUMMY),
    };
    let mut block0 = BasicBlockData::new(term);
    block0.is_cleanup = false;
    basic_blocks.push(block0);

    let ret_term = Terminator {
        kind: TerminatorKind::Return,
        source_info: SourceInfo::new(glyim_span::Span::DUMMY),
    };
    basic_blocks.push(BasicBlockData::new(ret_term));

    let body = Arc::new(Body {
        owner: DefId::new(CrateId::from_raw(0), LocalDefId::from_raw(0)),
        basic_blocks,
        locals: local_decls,
        arg_count: 0,
        return_ty: glyim_type::Ty::UNIT,
        span: glyim_span::Span::DUMMY,
        var_debug_info: vec![],
    });

    let backend = BytecodeBackend::with_ty_ctx(
        std::sync::Arc::new(glyim_type::TyCtxMut::new(glyim_core::Interner::default()).freeze()),
        glyim_core::TargetInfo::default(),
    );
    let result = backend.generate_function(&body);
    assert!(
        result.is_ok(),
        "generate_function should succeed: {:?}",
        result.err()
    );
    let bc = result.unwrap();
    assert!(
        bc.contains(&0x1B),
        "bytecode should contain OP_CALL for direct call"
    );
    assert!(
        !bc.contains(&0x22),
        "bytecode should NOT contain OP_CALL_INDIRECT for direct call"
    );
    drop(ctx);
}

#[test]
fn object_safety_check() {
    use glyim_span::Span;
    use glyim_type::object_safety::*;

    let interner = Interner::new();
    let method_name = interner.intern("method");
    let static_name = interner.intern("static_fn");

    // Object-safe trait: only &self methods, no generics, no Self: Sized
    let methods = vec![MethodSignature {
        name: method_name,
        span: Span::DUMMY,
        self_kind: MethodSelfKind::ByReference,
        has_generic_params: false,
        returns_self: false,
    }];
    let violations = check_object_safety(&TraitObjectSafetyInput {
        requires_self_sized: false,
        methods: &methods,
        associated_types: &[],
        supertrait_safety: &[],
    });
    assert!(violations.is_empty(), "trait should be object-safe");

    // Non-object-safe: generic method
    let methods = vec![MethodSignature {
        name: method_name,
        span: Span::DUMMY,
        self_kind: MethodSelfKind::ByReference,
        has_generic_params: true,
        returns_self: false,
    }];
    let violations = check_object_safety(&TraitObjectSafetyInput {
        requires_self_sized: false,
        methods: &methods,
        associated_types: &[],
        supertrait_safety: &[],
    });
    assert!(
        !violations.is_empty(),
        "trait with generic method should not be object-safe"
    );
    assert!(
        violations
            .iter()
            .any(|v| matches!(v, ObjectSafetyViolation::GenericMethod { .. }))
    );

    // Non-object-safe: requires Self: Sized
    let violations = check_object_safety(&TraitObjectSafetyInput {
        requires_self_sized: true,
        methods: &[],
        associated_types: &[],
        supertrait_safety: &[],
    });
    assert!(
        violations
            .iter()
            .any(|v| matches!(v, ObjectSafetyViolation::SelfSized))
    );

    // Non-object-safe: static method (no self)
    let methods = vec![MethodSignature {
        name: static_name,
        span: Span::DUMMY,
        self_kind: MethodSelfKind::None,
        has_generic_params: false,
        returns_self: false,
    }];
    let violations = check_object_safety(&TraitObjectSafetyInput {
        requires_self_sized: false,
        methods: &methods,
        associated_types: &[],
        supertrait_safety: &[],
    });
    assert!(
        violations
            .iter()
            .any(|v| matches!(v, ObjectSafetyViolation::StaticMethod { .. }))
    );
}

#[test]
fn object_safety_by_value_self_on_non_sized_trait() {
    use glyim_span::Span;
    use glyim_type::object_safety::*;

    let interner = Interner::new();
    let method_name = interner.intern("consume");

    let methods = vec![MethodSignature {
        name: method_name,
        span: Span::DUMMY,
        self_kind: MethodSelfKind::ByValue,
        has_generic_params: false,
        returns_self: false,
    }];
    // Trait does not require Self: Sized, so by-value self should be a violation
    let violations = check_object_safety(&TraitObjectSafetyInput {
        requires_self_sized: false,
        methods: &methods,
        associated_types: &[],
        supertrait_safety: &[],
    });
    assert!(
        violations
            .iter()
            .any(|v| matches!(v, ObjectSafetyViolation::ByValueSelf { .. })),
        "by-value self on non-Sized trait should be a violation"
    );
    // If trait requires Self: Sized, by-value self is allowed (but SelfSized violation exists)
    let violations = check_object_safety(&TraitObjectSafetyInput {
        requires_self_sized: true,
        methods: &methods,
        associated_types: &[],
        supertrait_safety: &[],
    });
    assert!(
        violations
            .iter()
            .any(|v| matches!(v, ObjectSafetyViolation::SelfSized)),
        "Sized trait still has SelfSized violation"
    );
    // ByValueSelf should NOT appear if Self: Sized is already required
    assert!(
        !violations
            .iter()
            .any(|v| matches!(v, ObjectSafetyViolation::ByValueSelf { .. })),
        "ByValueSelf should not appear if Self: Sized already required"
    );
}

#[test]
fn object_safety_associated_function_violation() {
    use glyim_span::Span;
    use glyim_type::object_safety::*;

    let interner = Interner::new();
    let assoc_name = interner.intern("new");

    let methods = vec![MethodSignature {
        name: assoc_name,
        span: Span::DUMMY,
        self_kind: MethodSelfKind::None,
        has_generic_params: false,
        returns_self: true,
    }];
    let violations = check_object_safety(&TraitObjectSafetyInput {
        requires_self_sized: false,
        methods: &methods,
        associated_types: &[],
        supertrait_safety: &[],
    });
    assert!(
        violations
            .iter()
            .any(|v| matches!(v, ObjectSafetyViolation::StaticMethod { .. })),
        "associated function without self should be StaticMethod violation"
    );
}

#[test]
fn object_safety_multiple_violations_accumulate() {
    use glyim_span::Span;
    use glyim_type::object_safety::*;

    let interner = Interner::new();
    let name1 = interner.intern("generic_method");
    let name2 = interner.intern("static_method");

    let methods = vec![
        MethodSignature {
            name: name1,
            span: Span::DUMMY,
            self_kind: MethodSelfKind::ByReference,
            has_generic_params: true,
            returns_self: false,
        },
        MethodSignature {
            name: name2,
            span: Span::DUMMY,
            self_kind: MethodSelfKind::None,
            has_generic_params: false,
            returns_self: false,
        },
    ];
    let violations = check_object_safety(&TraitObjectSafetyInput {
        requires_self_sized: false,
        methods: &methods,
        associated_types: &[],
        supertrait_safety: &[],
    });
    assert_eq!(violations.len(), 2, "should have two violations");
    assert!(
        violations
            .iter()
            .any(|v| matches!(v, ObjectSafetyViolation::GenericMethod { .. })),
        "should have GenericMethod"
    );
    assert!(
        violations
            .iter()
            .any(|v| matches!(v, ObjectSafetyViolation::StaticMethod { .. })),
        "should have StaticMethod"
    );
}

#[test]
fn vtable_layout_matches_expectations() {
    use glyim_layout::{LayoutComputer, SimpleLayoutComputer};
    let (ctx, dyn_ty) = with_fresh_ty_ctx(|ctx| {
        let empty_subst = ctx.intern_substitution(vec![]);
        let trait_ref = TraitRef {
            def_id: TraitDefId::from_raw(0),
            substs: empty_subst,
        };
        let predicate = Predicate::Trait(TraitPredicate {
            trait_ref,
            polarity: ImplPolarity::Positive,
        });
        let box_predicates: Box<[Predicate]> = Box::new([predicate]);
        let bound_vars: Box<[BoundVariableKind]> = vec![].into();
        let binder = Binder::bind(box_predicates, bound_vars);
        let kind = TyKind::Dynamic(binder, Region::Erased);
        ctx.mk_ty(kind)
    });
    let computer = SimpleLayoutComputer::new(&ctx, TargetInfo::x86_64());
    let layout = computer
        .layout_of(dyn_ty)
        .expect("layout_of(dyn Trait) should succeed");
    assert_eq!(layout.size.0, 16, "fat pointer size should be 16 on x86_64");
    assert_eq!(layout.align.0, 8, "fat pointer align should be 8 on x86_64");
    assert!(!layout.is_unsized);
}

#[test]
fn vtable_layout_offsets_with_methods() {
    use glyim_core::{Abi, Safety};
    use glyim_layout::vtable::{VTableComputer, VTableEntry, VTableLayout};
    use glyim_type::FnSig;
    let (ctx, dyn_ty) = with_fresh_ty_ctx(|ctx| {
        // Register trait id 1 (no methods) so vtable_of resolves it to a
        // legitimately-empty method list (plan §10.1: an *unregistered* trait
        // is now a hard `UnknownTrait` error rather than a silent empty vtable).
        let trait_name = glyim_core::Interner::new().intern("TestTrait");
        ctx.register_trait_def(
            TraitDefId::from_raw(1),
            glyim_type::TraitDef {
                name: trait_name,
                methods: vec![],
                associated_types: vec![],
            },
        );
        let empty_subst = ctx.intern_substitution(vec![]);
        let trait_ref = TraitRef {
            def_id: TraitDefId::from_raw(1),
            substs: empty_subst,
        };
        let predicate = Predicate::Trait(TraitPredicate {
            trait_ref,
            polarity: ImplPolarity::Positive,
        });
        let box_predicates: Box<[Predicate]> = Box::new([predicate]);
        let bound_vars: Box<[BoundVariableKind]> = vec![].into();
        let binder = Binder::bind(box_predicates, bound_vars);
        let kind = TyKind::Dynamic(binder, Region::Erased);
        ctx.mk_ty(kind)
    });
    let computer = glyim_layout::SimpleLayoutComputer::new(&ctx, TargetInfo::x86_64());
    let vtable_layout = computer
        .vtable_of(TraitDefId::from_raw(1), dyn_ty)
        .expect("vtable_of should return Some");
    // Empty methods, so vtable has only metadata
    assert_eq!(vtable_layout.methods.len(), 0);
    // Check memory size: 3 metadata entries * 8 bytes = 24 bytes
    let mem = vtable_layout.memory_size(8);
    assert_eq!(mem.size, 24);
    assert_eq!(mem.align, 8);

    // Add some method entries manually to test offsets
    let interner = Interner::new();
    let sig = FnSig {
        inputs: Substitution::empty(),
        output: Ty::UNIT,
        c_variadic: false,
        unsafety: Safety::Safe,
        abi: Abi::Glyim,
    };
    let method_entries = vec![
        VTableEntry {
            name: interner.intern("method1"),
            sig: sig.clone(),
            fn_def_id: FnDefId::from_raw(100),
        },
        VTableEntry {
            name: interner.intern("method2"),
            sig,
            fn_def_id: FnDefId::from_raw(101),
        },
    ];
    let vtable_with_methods = VTableLayout {
        methods: method_entries,
        ..vtable_layout
    };
    let mem2 = vtable_with_methods.memory_size(8);
    assert_eq!(
        mem2.size,
        (3 + 2) * 8,
        "size should include two method pointers"
    );
    assert_eq!(
        vtable_with_methods.method_offset(0, 8),
        3 * 8,
        "first method at offset 24"
    );
    assert_eq!(
        vtable_with_methods.method_offset(1, 8),
        4 * 8,
        "second method at offset 32"
    );
}
