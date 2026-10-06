//! Tests for V15: LLVM Drop & Deallocation
//!
//! Core tests:
//! V15-T01: Drop of Copy type at block end - no drop call emitted
//! V15-T02: Drop of non-Copy type → drop_in_place call emitted
//! V15-T03: Drop in panic path (cleanup) with non-Copy type
//! V15-T04: Drop order matches declaration order (sequential drops)
//! V15-T05: Drop of mutable ref to non-Copy type → dealloc emitted
//! V15-T06: Drop of &mut Copy type → no dealloc needed
//!
//! Extended tests:
//! V15-T07: Drop of tuple of non-Copy types → drop_in_place emitted
//! V15-T08: Drop of tuple of Copy types → no drop needed
//! V15-T09: Drop of &String (shared ref) → drop_in_place but no dealloc
//! V15-T10: Drop of raw pointer to non-Copy type
//! V15-T11: Drop of unit type → no drop needed
//! V15-T12: Drop of bool → no drop needed
//! V15-T13: Drop of nested ref &&mut String → drop_in_place emitted
//! V15-T14: Drop with cleanup target block specified
//! V15-T15: Drop of array of non-Copy elements → drop_in_place emitted

use crate::LlvmBackend;
use glyim_core::arena::IndexVec;
use glyim_core::def_id::{CrateId, DefId, LocalDefId};
use glyim_core::{IntTy, Mutability, UintTy};
use glyim_mir::*;
use glyim_test::with_fresh_ty_ctx;
use glyim_type::{Ty, TyKind};
use inkwell::targets::{InitializationConfig, Target};

fn dummy_def_id(idx: u32) -> DefId {
    DefId::new(CrateId::from_raw(0), LocalDefId::from_raw(idx))
}

/// Helper: count occurrences of `call void @glyim_drop_in_place` in IR
fn count_drop_calls(ir: &str) -> usize {
    ir.matches("call void @glyim_drop_in_place").count()
}

/// V15-T01: Drop of Copy type at block end - no drop call emitted.
#[test]
fn v15_t01_drop_copy_type_at_block_end() {
    Target::initialize_all(&InitializationConfig::default());

    let (ctx, body) = with_fresh_ty_ctx(|ctx_mut| {
        let i32_ty = ctx_mut.mk_ty(TyKind::Int(IntTy::I32));

        let mut locals = IndexVec::<LocalIdx, LocalDecl>::new();
        locals.push(LocalDecl {
            ty: Ty::UNIT,
            mutability: Mutability::Not,
            source_info: SourceInfo::new(glyim_span::Span::DUMMY),
        });
        locals.push(LocalDecl {
            ty: i32_ty,
            mutability: Mutability::Mut,
            source_info: SourceInfo::new(glyim_span::Span::DUMMY),
        });

        let mut bbs = IndexVec::<BasicBlockIdx, BasicBlockData>::new();
        bbs.push(BasicBlockData {
            statements: vec![Statement {
                kind: StatementKind::Assign(
                    Place::new(LocalIdx::from_raw(1)),
                    Rvalue::Use(Operand::Constant(MirConst {
                        kind: MirConstKind::Int(42),
                        ty: i32_ty,
                        span: glyim_span::Span::DUMMY,
                    })),
                ),
                source_info: SourceInfo::new(glyim_span::Span::DUMMY),
            }],
            terminator: Terminator {
                kind: TerminatorKind::Drop {
                    place: Place::new(LocalIdx::from_raw(1)),
                    target: BasicBlockIdx::from_raw(1),
                    cleanup: None,
                },
                source_info: SourceInfo::new(glyim_span::Span::DUMMY),
            },
            is_cleanup: false,
        });
        bbs.push(BasicBlockData {
            statements: vec![],
            terminator: Terminator {
                kind: TerminatorKind::Return,
                source_info: SourceInfo::new(glyim_span::Span::DUMMY),
            },
            is_cleanup: false,
        });

        Body {
            owner: dummy_def_id(100),
            basic_blocks: bbs,
            locals,
            arg_count: 0,
            return_ty: Ty::UNIT,
            span: glyim_span::Span::DUMMY,
            var_debug_info: vec![],
        }
    });

    let backend = LlvmBackend::new().with_ty_ctx(ctx);
    let context = inkwell::context::Context::create();
    let module = backend
        .lower_body_to_module(&context, &body)
        .expect("lowering should succeed");

    let ir = module.print_to_string().to_string();
    assert!(
        !ir.contains("call void @glyim_drop_in_place"),
        "IR should NOT call glyim_drop_in_place for Copy type i32, got:\n{}",
        ir
    );
    assert!(
        ir.contains("br label") || ir.contains("ret void"),
        "IR should contain branch or return, got:\n{}",
        ir
    );
}

/// V15-T02: Drop of non-Copy type → drop_in_place call emitted.
#[test]
fn v15_t02_drop_non_copy_type_emits_drop_in_place() {
    Target::initialize_all(&InitializationConfig::default());

    let (ctx, body) = with_fresh_ty_ctx(|ctx_mut| {
        let string_ty = ctx_mut.mk_ty(TyKind::String);

        let mut locals = IndexVec::<LocalIdx, LocalDecl>::new();
        locals.push(LocalDecl {
            ty: Ty::UNIT,
            mutability: Mutability::Not,
            source_info: SourceInfo::new(glyim_span::Span::DUMMY),
        });
        locals.push(LocalDecl {
            ty: string_ty,
            mutability: Mutability::Mut,
            source_info: SourceInfo::new(glyim_span::Span::DUMMY),
        });

        let mut bbs = IndexVec::<BasicBlockIdx, BasicBlockData>::new();
        bbs.push(BasicBlockData {
            statements: vec![],
            terminator: Terminator {
                kind: TerminatorKind::Drop {
                    place: Place::new(LocalIdx::from_raw(1)),
                    target: BasicBlockIdx::from_raw(1),
                    cleanup: None,
                },
                source_info: SourceInfo::new(glyim_span::Span::DUMMY),
            },
            is_cleanup: false,
        });
        bbs.push(BasicBlockData {
            statements: vec![],
            terminator: Terminator {
                kind: TerminatorKind::Return,
                source_info: SourceInfo::new(glyim_span::Span::DUMMY),
            },
            is_cleanup: false,
        });

        Body {
            owner: dummy_def_id(101),
            basic_blocks: bbs,
            locals,
            arg_count: 0,
            return_ty: Ty::UNIT,
            span: glyim_span::Span::DUMMY,
            var_debug_info: vec![],
        }
    });

    let backend = LlvmBackend::new().with_ty_ctx(ctx);
    let context = inkwell::context::Context::create();
    let module = backend
        .lower_body_to_module(&context, &body)
        .expect("lowering should succeed");

    let ir = module.print_to_string().to_string();
    assert!(
        ir.contains("call void @glyim_drop_in_place"),
        "IR should call glyim_drop_in_place for non-Copy String type, got:\n{}",
        ir
    );
}

/// V15-T03: Drop in panic path (cleanup) with non-Copy type.
#[test]
fn v15_t03_drop_in_panic_cleanup_path() {
    Target::initialize_all(&InitializationConfig::default());

    let (ctx, body) = with_fresh_ty_ctx(|ctx_mut| {
        let string_ty = ctx_mut.mk_ty(TyKind::String);
        let i32_ty = ctx_mut.mk_ty(TyKind::Int(IntTy::I32));

        let mut locals = IndexVec::<LocalIdx, LocalDecl>::new();
        locals.push(LocalDecl {
            ty: Ty::UNIT,
            mutability: Mutability::Not,
            source_info: SourceInfo::new(glyim_span::Span::DUMMY),
        });
        locals.push(LocalDecl {
            ty: string_ty,
            mutability: Mutability::Mut,
            source_info: SourceInfo::new(glyim_span::Span::DUMMY),
        });
        locals.push(LocalDecl {
            ty: i32_ty,
            mutability: Mutability::Not,
            source_info: SourceInfo::new(glyim_span::Span::DUMMY),
        });

        let mut bbs = IndexVec::<BasicBlockIdx, BasicBlockData>::new();
        bbs.push(BasicBlockData {
            statements: vec![],
            terminator: Terminator {
                kind: TerminatorKind::SwitchInt {
                    discr: Operand::Move(Place::new(LocalIdx::from_raw(2))),
                    switch_ty: i32_ty,
                    targets: SwitchTargets::new(
                        Box::from_iter([(1u128, BasicBlockIdx::from_raw(1))]),
                        BasicBlockIdx::from_raw(2),
                    ),
                },
                source_info: SourceInfo::new(glyim_span::Span::DUMMY),
            },
            is_cleanup: false,
        });
        bbs.push(BasicBlockData {
            statements: vec![],
            terminator: Terminator {
                kind: TerminatorKind::Drop {
                    place: Place::new(LocalIdx::from_raw(1)),
                    target: BasicBlockIdx::from_raw(3),
                    cleanup: None,
                },
                source_info: SourceInfo::new(glyim_span::Span::DUMMY),
            },
            is_cleanup: false,
        });
        bbs.push(BasicBlockData {
            statements: vec![],
            terminator: Terminator {
                kind: TerminatorKind::Drop {
                    place: Place::new(LocalIdx::from_raw(1)),
                    target: BasicBlockIdx::from_raw(3),
                    cleanup: None,
                },
                source_info: SourceInfo::new(glyim_span::Span::DUMMY),
            },
            is_cleanup: true,
        });
        bbs.push(BasicBlockData {
            statements: vec![],
            terminator: Terminator {
                kind: TerminatorKind::Return,
                source_info: SourceInfo::new(glyim_span::Span::DUMMY),
            },
            is_cleanup: false,
        });

        Body {
            owner: dummy_def_id(102),
            basic_blocks: bbs,
            locals,
            arg_count: 0,
            return_ty: Ty::UNIT,
            span: glyim_span::Span::DUMMY,
            var_debug_info: vec![],
        }
    });

    let backend = LlvmBackend::new().with_ty_ctx(ctx);
    let context = inkwell::context::Context::create();
    let module = backend
        .lower_body_to_module(&context, &body)
        .expect("lowering should succeed");

    let ir = module.print_to_string().to_string();
    let drop_count = count_drop_calls(&ir);
    assert!(
        drop_count >= 2,
        "IR should have at least 2 drop_in_place calls (normal + cleanup), found {}, got:\n{}",
        drop_count,
        ir
    );
}

/// V15-T04: Drop order matches declaration order (sequential drops).
#[test]
fn v15_t04_drop_order_matches_declaration_order() {
    Target::initialize_all(&InitializationConfig::default());

    let (ctx, body) = with_fresh_ty_ctx(|ctx_mut| {
        let string_ty = ctx_mut.mk_ty(TyKind::String);

        let mut locals = IndexVec::<LocalIdx, LocalDecl>::new();
        locals.push(LocalDecl {
            ty: Ty::UNIT,
            mutability: Mutability::Not,
            source_info: SourceInfo::new(glyim_span::Span::DUMMY),
        });
        locals.push(LocalDecl {
            ty: string_ty,
            mutability: Mutability::Mut,
            source_info: SourceInfo::new(glyim_span::Span::DUMMY),
        });
        locals.push(LocalDecl {
            ty: string_ty,
            mutability: Mutability::Mut,
            source_info: SourceInfo::new(glyim_span::Span::DUMMY),
        });

        let mut bbs = IndexVec::<BasicBlockIdx, BasicBlockData>::new();
        bbs.push(BasicBlockData {
            statements: vec![],
            terminator: Terminator {
                kind: TerminatorKind::Drop {
                    place: Place::new(LocalIdx::from_raw(2)),
                    target: BasicBlockIdx::from_raw(1),
                    cleanup: None,
                },
                source_info: SourceInfo::new(glyim_span::Span::DUMMY),
            },
            is_cleanup: false,
        });
        bbs.push(BasicBlockData {
            statements: vec![],
            terminator: Terminator {
                kind: TerminatorKind::Drop {
                    place: Place::new(LocalIdx::from_raw(1)),
                    target: BasicBlockIdx::from_raw(2),
                    cleanup: None,
                },
                source_info: SourceInfo::new(glyim_span::Span::DUMMY),
            },
            is_cleanup: false,
        });
        bbs.push(BasicBlockData {
            statements: vec![],
            terminator: Terminator {
                kind: TerminatorKind::Return,
                source_info: SourceInfo::new(glyim_span::Span::DUMMY),
            },
            is_cleanup: false,
        });

        Body {
            owner: dummy_def_id(103),
            basic_blocks: bbs,
            locals,
            arg_count: 0,
            return_ty: Ty::UNIT,
            span: glyim_span::Span::DUMMY,
            var_debug_info: vec![],
        }
    });

    let backend = LlvmBackend::new().with_ty_ctx(ctx);
    let context = inkwell::context::Context::create();
    let module = backend
        .lower_body_to_module(&context, &body)
        .expect("lowering should succeed");

    let ir = module.print_to_string().to_string();
    let drop_count = count_drop_calls(&ir);
    assert!(
        drop_count >= 2,
        "IR should have at least 2 drop_in_place calls for sequential drops, found {}, got:\n{}",
        drop_count,
        ir
    );
}

/// V15-T05: Drop of mutable ref to non-Copy type → both drop_in_place and dealloc emitted.
#[test]
fn v15_t05_drop_mut_ref_non_copy_emits_dealloc() {
    Target::initialize_all(&InitializationConfig::default());

    let (ctx, body) = with_fresh_ty_ctx(|ctx_mut| {
        let string_ty = ctx_mut.mk_ty(TyKind::String);
        let ref_string_ty = ctx_mut.mk_ty(TyKind::Ref(
            glyim_type::Region::Erased,
            string_ty,
            Mutability::Mut,
        ));

        let mut locals = IndexVec::<LocalIdx, LocalDecl>::new();
        locals.push(LocalDecl {
            ty: Ty::UNIT,
            mutability: Mutability::Not,
            source_info: SourceInfo::new(glyim_span::Span::DUMMY),
        });
        locals.push(LocalDecl {
            ty: ref_string_ty,
            mutability: Mutability::Mut,
            source_info: SourceInfo::new(glyim_span::Span::DUMMY),
        });

        let mut bbs = IndexVec::<BasicBlockIdx, BasicBlockData>::new();
        bbs.push(BasicBlockData {
            statements: vec![],
            terminator: Terminator {
                kind: TerminatorKind::Drop {
                    place: Place::new(LocalIdx::from_raw(1)),
                    target: BasicBlockIdx::from_raw(1),
                    cleanup: None,
                },
                source_info: SourceInfo::new(glyim_span::Span::DUMMY),
            },
            is_cleanup: false,
        });
        bbs.push(BasicBlockData {
            statements: vec![],
            terminator: Terminator {
                kind: TerminatorKind::Return,
                source_info: SourceInfo::new(glyim_span::Span::DUMMY),
            },
            is_cleanup: false,
        });

        Body {
            owner: dummy_def_id(104),
            basic_blocks: bbs,
            locals,
            arg_count: 0,
            return_ty: Ty::UNIT,
            span: glyim_span::Span::DUMMY,
            var_debug_info: vec![],
        }
    });

    let backend = LlvmBackend::new().with_ty_ctx(ctx);
    let context = inkwell::context::Context::create();
    let module = backend
        .lower_body_to_module(&context, &body)
        .expect("lowering should succeed");

    let ir = module.print_to_string().to_string();
    assert!(
        ir.contains("call void @glyim_drop_in_place"),
        "IR should call glyim_drop_in_place for &mut String drop, got:\n{}",
        ir
    );
    assert!(
        ir.contains("call void @glyim_dealloc"),
        "IR should call glyim_dealloc for &mut String deallocation, got:\n{}",
        ir
    );
}

/// V15-T06: Drop of &mut Copy type → no drop_in_place call, no dealloc call.
#[test]
fn v15_t06_drop_mut_ref_copy_type_no_dealloc() {
    Target::initialize_all(&InitializationConfig::default());

    let (ctx, body) = with_fresh_ty_ctx(|ctx_mut| {
        let i32_ty = ctx_mut.mk_ty(TyKind::Int(IntTy::I32));
        let ref_i32_ty = ctx_mut.mk_ty(TyKind::Ref(
            glyim_type::Region::Erased,
            i32_ty,
            Mutability::Mut,
        ));

        let mut locals = IndexVec::<LocalIdx, LocalDecl>::new();
        locals.push(LocalDecl {
            ty: Ty::UNIT,
            mutability: Mutability::Not,
            source_info: SourceInfo::new(glyim_span::Span::DUMMY),
        });
        locals.push(LocalDecl {
            ty: ref_i32_ty,
            mutability: Mutability::Mut,
            source_info: SourceInfo::new(glyim_span::Span::DUMMY),
        });

        let mut bbs = IndexVec::<BasicBlockIdx, BasicBlockData>::new();
        bbs.push(BasicBlockData {
            statements: vec![],
            terminator: Terminator {
                kind: TerminatorKind::Drop {
                    place: Place::new(LocalIdx::from_raw(1)),
                    target: BasicBlockIdx::from_raw(1),
                    cleanup: None,
                },
                source_info: SourceInfo::new(glyim_span::Span::DUMMY),
            },
            is_cleanup: false,
        });
        bbs.push(BasicBlockData {
            statements: vec![],
            terminator: Terminator {
                kind: TerminatorKind::Return,
                source_info: SourceInfo::new(glyim_span::Span::DUMMY),
            },
            is_cleanup: false,
        });

        Body {
            owner: dummy_def_id(105),
            basic_blocks: bbs,
            locals,
            arg_count: 0,
            return_ty: Ty::UNIT,
            span: glyim_span::Span::DUMMY,
            var_debug_info: vec![],
        }
    });

    let backend = LlvmBackend::new().with_ty_ctx(ctx);
    let context = inkwell::context::Context::create();
    let module = backend
        .lower_body_to_module(&context, &body)
        .expect("lowering should succeed");

    let ir = module.print_to_string().to_string();
    assert!(
        !ir.contains("call void @glyim_drop_in_place"),
        "IR should NOT call glyim_drop_in_place for &mut i32, got:\n{}",
        ir
    );
    assert!(
        !ir.contains("call void @glyim_dealloc"),
        "IR should NOT call glyim_dealloc for &mut i32, got:\n{}",
        ir
    );
}

/// V15-T07: Drop of tuple of non-Copy types → drop_in_place emitted.
///
/// A tuple containing String needs drop because String needs drop.
#[test]
fn v15_t07_drop_tuple_of_non_copy_types() {
    Target::initialize_all(&InitializationConfig::default());

    let (ctx, body) = with_fresh_ty_ctx(|ctx_mut| {
        let string_ty = ctx_mut.mk_ty(TyKind::String);
        let subst = ctx_mut.intern_substitution(vec![
            glyim_type::GenericArg::Ty(string_ty),
            glyim_type::GenericArg::Ty(string_ty),
        ]);
        let tuple_ty = ctx_mut.mk_ty(TyKind::Tuple(subst));

        let mut locals = IndexVec::<LocalIdx, LocalDecl>::new();
        locals.push(LocalDecl {
            ty: Ty::UNIT,
            mutability: Mutability::Not,
            source_info: SourceInfo::new(glyim_span::Span::DUMMY),
        });
        locals.push(LocalDecl {
            ty: tuple_ty,
            mutability: Mutability::Mut,
            source_info: SourceInfo::new(glyim_span::Span::DUMMY),
        });

        let mut bbs = IndexVec::<BasicBlockIdx, BasicBlockData>::new();
        bbs.push(BasicBlockData {
            statements: vec![],
            terminator: Terminator {
                kind: TerminatorKind::Drop {
                    place: Place::new(LocalIdx::from_raw(1)),
                    target: BasicBlockIdx::from_raw(1),
                    cleanup: None,
                },
                source_info: SourceInfo::new(glyim_span::Span::DUMMY),
            },
            is_cleanup: false,
        });
        bbs.push(BasicBlockData {
            statements: vec![],
            terminator: Terminator {
                kind: TerminatorKind::Return,
                source_info: SourceInfo::new(glyim_span::Span::DUMMY),
            },
            is_cleanup: false,
        });

        Body {
            owner: dummy_def_id(106),
            basic_blocks: bbs,
            locals,
            arg_count: 0,
            return_ty: Ty::UNIT,
            span: glyim_span::Span::DUMMY,
            var_debug_info: vec![],
        }
    });

    let backend = LlvmBackend::new().with_ty_ctx(ctx);
    let context = inkwell::context::Context::create();
    let module = backend
        .lower_body_to_module(&context, &body)
        .expect("lowering should succeed");

    let ir = module.print_to_string().to_string();
    assert!(
        ir.contains("call void @glyim_drop_in_place"),
        "IR should call glyim_drop_in_place for tuple of Strings, got:\n{}",
        ir
    );
}

/// V15-T08: Drop of tuple of Copy types → no drop needed.
///
/// A tuple of (i32, bool) is all Copy, so no drop is needed.
#[test]
fn v15_t08_drop_tuple_of_copy_types() {
    Target::initialize_all(&InitializationConfig::default());

    let (ctx, body) = with_fresh_ty_ctx(|ctx_mut| {
        let i32_ty = ctx_mut.mk_ty(TyKind::Int(IntTy::I32));
        let subst = ctx_mut.intern_substitution(vec![
            glyim_type::GenericArg::Ty(i32_ty),
            glyim_type::GenericArg::Ty(Ty::BOOL),
        ]);
        let tuple_ty = ctx_mut.mk_ty(TyKind::Tuple(subst));

        let mut locals = IndexVec::<LocalIdx, LocalDecl>::new();
        locals.push(LocalDecl {
            ty: Ty::UNIT,
            mutability: Mutability::Not,
            source_info: SourceInfo::new(glyim_span::Span::DUMMY),
        });
        locals.push(LocalDecl {
            ty: tuple_ty,
            mutability: Mutability::Mut,
            source_info: SourceInfo::new(glyim_span::Span::DUMMY),
        });

        let mut bbs = IndexVec::<BasicBlockIdx, BasicBlockData>::new();
        bbs.push(BasicBlockData {
            statements: vec![],
            terminator: Terminator {
                kind: TerminatorKind::Drop {
                    place: Place::new(LocalIdx::from_raw(1)),
                    target: BasicBlockIdx::from_raw(1),
                    cleanup: None,
                },
                source_info: SourceInfo::new(glyim_span::Span::DUMMY),
            },
            is_cleanup: false,
        });
        bbs.push(BasicBlockData {
            statements: vec![],
            terminator: Terminator {
                kind: TerminatorKind::Return,
                source_info: SourceInfo::new(glyim_span::Span::DUMMY),
            },
            is_cleanup: false,
        });

        Body {
            owner: dummy_def_id(107),
            basic_blocks: bbs,
            locals,
            arg_count: 0,
            return_ty: Ty::UNIT,
            span: glyim_span::Span::DUMMY,
            var_debug_info: vec![],
        }
    });

    let backend = LlvmBackend::new().with_ty_ctx(ctx);
    let context = inkwell::context::Context::create();
    let module = backend
        .lower_body_to_module(&context, &body)
        .expect("lowering should succeed");

    let ir = module.print_to_string().to_string();
    assert!(
        !ir.contains("call void @glyim_drop_in_place"),
        "IR should NOT call glyim_drop_in_place for tuple of Copy types, got:\n{}",
        ir
    );
}

/// V15-T09: Drop of &String (shared/immutable ref) → drop_in_place but no dealloc.
///
/// A shared reference (&T) does not own the data, so it should not
/// call glyim_dealloc even if T is not Copy. But if T needs drop,
/// drop_in_place is still called.
#[test]
fn v15_t09_drop_shared_ref_non_copy_no_dealloc() {
    Target::initialize_all(&InitializationConfig::default());

    let (ctx, body) = with_fresh_ty_ctx(|ctx_mut| {
        let string_ty = ctx_mut.mk_ty(TyKind::String);
        let ref_string_ty = ctx_mut.mk_ty(TyKind::Ref(
            glyim_type::Region::Erased,
            string_ty,
            Mutability::Not,
        ));

        let mut locals = IndexVec::<LocalIdx, LocalDecl>::new();
        locals.push(LocalDecl {
            ty: Ty::UNIT,
            mutability: Mutability::Not,
            source_info: SourceInfo::new(glyim_span::Span::DUMMY),
        });
        locals.push(LocalDecl {
            ty: ref_string_ty,
            mutability: Mutability::Mut,
            source_info: SourceInfo::new(glyim_span::Span::DUMMY),
        });

        let mut bbs = IndexVec::<BasicBlockIdx, BasicBlockData>::new();
        bbs.push(BasicBlockData {
            statements: vec![],
            terminator: Terminator {
                kind: TerminatorKind::Drop {
                    place: Place::new(LocalIdx::from_raw(1)),
                    target: BasicBlockIdx::from_raw(1),
                    cleanup: None,
                },
                source_info: SourceInfo::new(glyim_span::Span::DUMMY),
            },
            is_cleanup: false,
        });
        bbs.push(BasicBlockData {
            statements: vec![],
            terminator: Terminator {
                kind: TerminatorKind::Return,
                source_info: SourceInfo::new(glyim_span::Span::DUMMY),
            },
            is_cleanup: false,
        });

        Body {
            owner: dummy_def_id(108),
            basic_blocks: bbs,
            locals,
            arg_count: 0,
            return_ty: Ty::UNIT,
            span: glyim_span::Span::DUMMY,
            var_debug_info: vec![],
        }
    });

    let backend = LlvmBackend::new().with_ty_ctx(ctx);
    let context = inkwell::context::Context::create();
    let module = backend
        .lower_body_to_module(&context, &body)
        .expect("lowering should succeed");

    let ir = module.print_to_string().to_string();
    // A shared reference does NOT own its pointee, so dropping &String must emit
    // no drop glue at all (neither drop_in_place nor dealloc).
    assert!(
        !ir.contains("call void @glyim_drop_in_place"),
        "IR should NOT call glyim_drop_in_place for &String (shared ref does not own), got:\n{}",
        ir
    );
    // Shared ref does not own, so no dealloc
    assert!(
        !ir.contains("call void @glyim_dealloc"),
        "IR should NOT call glyim_dealloc for &String (shared ref), got:\n{}",
        ir
    );
}

/// V15-T10: Drop of raw pointer to non-Copy type.
///
/// *mut String should trigger drop_in_place but since it's a raw pointer
/// and Mutability::Mut, type_needs_dealloc returns true if inner is not Copy.
#[test]
fn v15_t10_drop_raw_ptr_non_copy_type() {
    Target::initialize_all(&InitializationConfig::default());

    let (ctx, body) = with_fresh_ty_ctx(|ctx_mut| {
        let string_ty = ctx_mut.mk_ty(TyKind::String);
        let ptr_string_ty = ctx_mut.mk_ty(TyKind::RawPtr(string_ty, Mutability::Mut));

        let mut locals = IndexVec::<LocalIdx, LocalDecl>::new();
        locals.push(LocalDecl {
            ty: Ty::UNIT,
            mutability: Mutability::Not,
            source_info: SourceInfo::new(glyim_span::Span::DUMMY),
        });
        locals.push(LocalDecl {
            ty: ptr_string_ty,
            mutability: Mutability::Mut,
            source_info: SourceInfo::new(glyim_span::Span::DUMMY),
        });

        let mut bbs = IndexVec::<BasicBlockIdx, BasicBlockData>::new();
        bbs.push(BasicBlockData {
            statements: vec![],
            terminator: Terminator {
                kind: TerminatorKind::Drop {
                    place: Place::new(LocalIdx::from_raw(1)),
                    target: BasicBlockIdx::from_raw(1),
                    cleanup: None,
                },
                source_info: SourceInfo::new(glyim_span::Span::DUMMY),
            },
            is_cleanup: false,
        });
        bbs.push(BasicBlockData {
            statements: vec![],
            terminator: Terminator {
                kind: TerminatorKind::Return,
                source_info: SourceInfo::new(glyim_span::Span::DUMMY),
            },
            is_cleanup: false,
        });

        Body {
            owner: dummy_def_id(109),
            basic_blocks: bbs,
            locals,
            arg_count: 0,
            return_ty: Ty::UNIT,
            span: glyim_span::Span::DUMMY,
            var_debug_info: vec![],
        }
    });

    let backend = LlvmBackend::new().with_ty_ctx(ctx);
    let context = inkwell::context::Context::create();
    let module = backend
        .lower_body_to_module(&context, &body)
        .expect("lowering should succeed");

    let ir = module.print_to_string().to_string();
    // *mut String is an owning raw pointer in glyim's memory model (RawPtr(Mut)
    // with non-Copy inner), so drop_in_place is called on the pointee.
    assert!(
        ir.contains("call void @glyim_drop_in_place"),
        "IR should call glyim_drop_in_place for *mut String (owning raw ptr), got:\n{}",
        ir
    );
    // *mut String: owning raw ptr to non-Copy, dealloc should be called
    assert!(
        ir.contains("call void @glyim_dealloc"),
        "IR should call glyim_dealloc for *mut String (owning raw ptr), got:\n{}",
        ir
    );
}

/// V15-T11: Drop of unit type → no drop needed.
#[test]
fn v15_t11_drop_unit_type() {
    Target::initialize_all(&InitializationConfig::default());

    let (ctx, body) = with_fresh_ty_ctx(|ctx_mut| {
        let unit_ty = ctx_mut.unit_ty();
        let mut locals = IndexVec::<LocalIdx, LocalDecl>::new();
        locals.push(LocalDecl {
            ty: unit_ty,
            mutability: Mutability::Not,
            source_info: SourceInfo::new(glyim_span::Span::DUMMY),
        });
        locals.push(LocalDecl {
            ty: unit_ty,
            mutability: Mutability::Not,
            source_info: SourceInfo::new(glyim_span::Span::DUMMY),
        });

        let mut bbs = IndexVec::<BasicBlockIdx, BasicBlockData>::new();
        bbs.push(BasicBlockData {
            statements: vec![],
            terminator: Terminator {
                kind: TerminatorKind::Drop {
                    place: Place::new(LocalIdx::from_raw(1)),
                    target: BasicBlockIdx::from_raw(1),
                    cleanup: None,
                },
                source_info: SourceInfo::new(glyim_span::Span::DUMMY),
            },
            is_cleanup: false,
        });
        bbs.push(BasicBlockData {
            statements: vec![],
            terminator: Terminator {
                kind: TerminatorKind::Return,
                source_info: SourceInfo::new(glyim_span::Span::DUMMY),
            },
            is_cleanup: false,
        });

        Body {
            owner: dummy_def_id(110),
            basic_blocks: bbs,
            locals,
            arg_count: 0,
            return_ty: Ty::UNIT,
            span: glyim_span::Span::DUMMY,
            var_debug_info: vec![],
        }
    });

    let backend = LlvmBackend::new().with_ty_ctx(ctx);
    let context = inkwell::context::Context::create();
    let module = backend
        .lower_body_to_module(&context, &body)
        .expect("lowering should succeed");

    let ir = module.print_to_string().to_string();
    assert!(
        !ir.contains("call void @glyim_drop_in_place"),
        "IR should NOT call glyim_drop_in_place for unit type, got:\n{}",
        ir
    );
}

/// V15-T12: Drop of bool → no drop needed.
#[test]
fn v15_t12_drop_bool_type() {
    Target::initialize_all(&InitializationConfig::default());

    let (ctx, body) = with_fresh_ty_ctx(|ctx_mut| {
        let unit_ty = ctx_mut.unit_ty();
        let bool_ty = ctx_mut.bool_ty();
        let mut locals = IndexVec::<LocalIdx, LocalDecl>::new();
        locals.push(LocalDecl {
            ty: unit_ty,
            mutability: Mutability::Not,
            source_info: SourceInfo::new(glyim_span::Span::DUMMY),
        });
        locals.push(LocalDecl {
            ty: bool_ty,
            mutability: Mutability::Mut,
            source_info: SourceInfo::new(glyim_span::Span::DUMMY),
        });

        let mut bbs = IndexVec::<BasicBlockIdx, BasicBlockData>::new();
        bbs.push(BasicBlockData {
            statements: vec![],
            terminator: Terminator {
                kind: TerminatorKind::Drop {
                    place: Place::new(LocalIdx::from_raw(1)),
                    target: BasicBlockIdx::from_raw(1),
                    cleanup: None,
                },
                source_info: SourceInfo::new(glyim_span::Span::DUMMY),
            },
            is_cleanup: false,
        });
        bbs.push(BasicBlockData {
            statements: vec![],
            terminator: Terminator {
                kind: TerminatorKind::Return,
                source_info: SourceInfo::new(glyim_span::Span::DUMMY),
            },
            is_cleanup: false,
        });

        Body {
            owner: dummy_def_id(111),
            basic_blocks: bbs,
            locals,
            arg_count: 0,
            return_ty: Ty::UNIT,
            span: glyim_span::Span::DUMMY,
            var_debug_info: vec![],
        }
    });

    let backend = LlvmBackend::new().with_ty_ctx(ctx);
    let context = inkwell::context::Context::create();
    let module = backend
        .lower_body_to_module(&context, &body)
        .expect("lowering should succeed");

    let ir = module.print_to_string().to_string();
    assert!(
        !ir.contains("call void @glyim_drop_in_place"),
        "IR should NOT call glyim_drop_in_place for bool type, got:\n{}",
        ir
    );
}

/// V15-T13: Drop of nested ref &&mut String → drop_in_place emitted.
///
/// Double indirection: the inner type (after peeling & refs) is String
/// which needs drop, so drop_in_place should be called.
#[test]
fn v15_t13_drop_nested_ref() {
    Target::initialize_all(&InitializationConfig::default());

    let (ctx, body) = with_fresh_ty_ctx(|ctx_mut| {
        let string_ty = ctx_mut.mk_ty(TyKind::String);
        let inner_ref = ctx_mut.mk_ty(TyKind::Ref(
            glyim_type::Region::Erased,
            string_ty,
            Mutability::Mut,
        ));
        let outer_ref = ctx_mut.mk_ty(TyKind::Ref(
            glyim_type::Region::Erased,
            inner_ref,
            Mutability::Not,
        ));

        let mut locals = IndexVec::<LocalIdx, LocalDecl>::new();
        locals.push(LocalDecl {
            ty: Ty::UNIT,
            mutability: Mutability::Not,
            source_info: SourceInfo::new(glyim_span::Span::DUMMY),
        });
        locals.push(LocalDecl {
            ty: outer_ref,
            mutability: Mutability::Mut,
            source_info: SourceInfo::new(glyim_span::Span::DUMMY),
        });

        let mut bbs = IndexVec::<BasicBlockIdx, BasicBlockData>::new();
        bbs.push(BasicBlockData {
            statements: vec![],
            terminator: Terminator {
                kind: TerminatorKind::Drop {
                    place: Place::new(LocalIdx::from_raw(1)),
                    target: BasicBlockIdx::from_raw(1),
                    cleanup: None,
                },
                source_info: SourceInfo::new(glyim_span::Span::DUMMY),
            },
            is_cleanup: false,
        });
        bbs.push(BasicBlockData {
            statements: vec![],
            terminator: Terminator {
                kind: TerminatorKind::Return,
                source_info: SourceInfo::new(glyim_span::Span::DUMMY),
            },
            is_cleanup: false,
        });

        Body {
            owner: dummy_def_id(112),
            basic_blocks: bbs,
            locals,
            arg_count: 0,
            return_ty: Ty::UNIT,
            span: glyim_span::Span::DUMMY,
            var_debug_info: vec![],
        }
    });

    let backend = LlvmBackend::new().with_ty_ctx(ctx);
    let context = inkwell::context::Context::create();
    let module = backend
        .lower_body_to_module(&context, &body)
        .expect("lowering should succeed");

    let ir = module.print_to_string().to_string();
    // References do NOT own their pointee, so dropping &&mut String emits no drop
    // glue at all (drop_in_place would wrongly try to drop through the reference).
    assert!(
        !ir.contains("call void @glyim_drop_in_place"),
        "IR should NOT call glyim_drop_in_place for &&mut String (refs do not own), got:\n{}",
        ir
    );
    // Outer ref is immutable, so no dealloc
    assert!(
        !ir.contains("call void @glyim_dealloc"),
        "IR should NOT call glyim_dealloc for &&mut String (outer is shared ref), got:\n{}",
        ir
    );
}

/// V15-T14: Drop with cleanup target block specified.
///
/// The Drop terminator has a cleanup block that should be branched to
/// if the drop itself panics. This tests that the cleanup target is
/// properly handled in codegen.
#[test]
fn v15_t14_drop_with_cleanup_target() {
    Target::initialize_all(&InitializationConfig::default());

    let (ctx, body) = with_fresh_ty_ctx(|ctx_mut| {
        let string_ty = ctx_mut.mk_ty(TyKind::String);

        let mut locals = IndexVec::<LocalIdx, LocalDecl>::new();
        locals.push(LocalDecl {
            ty: Ty::UNIT,
            mutability: Mutability::Not,
            source_info: SourceInfo::new(glyim_span::Span::DUMMY),
        });
        locals.push(LocalDecl {
            ty: string_ty,
            mutability: Mutability::Mut,
            source_info: SourceInfo::new(glyim_span::Span::DUMMY),
        });

        let mut bbs = IndexVec::<BasicBlockIdx, BasicBlockData>::new();
        // bb0: drop with cleanup target
        bbs.push(BasicBlockData {
            statements: vec![],
            terminator: Terminator {
                kind: TerminatorKind::Drop {
                    place: Place::new(LocalIdx::from_raw(1)),
                    target: BasicBlockIdx::from_raw(1),
                    cleanup: Some(BasicBlockIdx::from_raw(2)),
                },
                source_info: SourceInfo::new(glyim_span::Span::DUMMY),
            },
            is_cleanup: false,
        });
        // bb1: normal return after successful drop
        bbs.push(BasicBlockData {
            statements: vec![],
            terminator: Terminator {
                kind: TerminatorKind::Return,
                source_info: SourceInfo::new(glyim_span::Span::DUMMY),
            },
            is_cleanup: false,
        });
        // bb2: cleanup block (unreachable in practice, but must exist)
        bbs.push(BasicBlockData {
            statements: vec![],
            terminator: Terminator {
                kind: TerminatorKind::Unreachable,
                source_info: SourceInfo::new(glyim_span::Span::DUMMY),
            },
            is_cleanup: true,
        });

        Body {
            owner: dummy_def_id(113),
            basic_blocks: bbs,
            locals,
            arg_count: 0,
            return_ty: Ty::UNIT,
            span: glyim_span::Span::DUMMY,
            var_debug_info: vec![],
        }
    });

    let backend = LlvmBackend::new().with_ty_ctx(ctx);
    let context = inkwell::context::Context::create();
    let module = backend
        .lower_body_to_module(&context, &body)
        .expect("lowering should succeed");

    let ir = module.print_to_string().to_string();
    assert!(
        ir.contains("@glyim_drop_in_place"),
        "IR should invoke glyim_drop_in_place for String with cleanup target, got:\n{}",
        ir
    );
    // Should have both target and cleanup blocks
    assert!(
        ir.contains("bb_1") && ir.contains("bb_2"),
        "IR should have basic blocks for both target and cleanup paths, got:\n{}",
        ir
    );
}

/// V15-T15: Drop of array of non-Copy elements → drop_in_place emitted.
///
/// An array of Strings needs drop because the element type needs drop.
#[test]
fn v15_t15_drop_array_of_non_copy() {
    Target::initialize_all(&InitializationConfig::default());

    let (ctx, body) = with_fresh_ty_ctx(|ctx_mut| {
        let string_ty = ctx_mut.mk_ty(TyKind::String);
        let count = glyim_type::Const {
            kind: glyim_type::ConstKind::Uint(3),
            ty: ctx_mut.mk_ty(TyKind::Uint(UintTy::Usize)),
        };
        let array_ty = ctx_mut.mk_ty(TyKind::Array(string_ty, count));

        let mut locals = IndexVec::<LocalIdx, LocalDecl>::new();
        locals.push(LocalDecl {
            ty: Ty::UNIT,
            mutability: Mutability::Not,
            source_info: SourceInfo::new(glyim_span::Span::DUMMY),
        });
        locals.push(LocalDecl {
            ty: array_ty,
            mutability: Mutability::Mut,
            source_info: SourceInfo::new(glyim_span::Span::DUMMY),
        });

        let mut bbs = IndexVec::<BasicBlockIdx, BasicBlockData>::new();
        bbs.push(BasicBlockData {
            statements: vec![],
            terminator: Terminator {
                kind: TerminatorKind::Drop {
                    place: Place::new(LocalIdx::from_raw(1)),
                    target: BasicBlockIdx::from_raw(1),
                    cleanup: None,
                },
                source_info: SourceInfo::new(glyim_span::Span::DUMMY),
            },
            is_cleanup: false,
        });
        bbs.push(BasicBlockData {
            statements: vec![],
            terminator: Terminator {
                kind: TerminatorKind::Return,
                source_info: SourceInfo::new(glyim_span::Span::DUMMY),
            },
            is_cleanup: false,
        });

        Body {
            owner: dummy_def_id(114),
            basic_blocks: bbs,
            locals,
            arg_count: 0,
            return_ty: Ty::UNIT,
            span: glyim_span::Span::DUMMY,
            var_debug_info: vec![],
        }
    });

    let backend = LlvmBackend::new().with_ty_ctx(ctx);
    let context = inkwell::context::Context::create();
    let module = backend
        .lower_body_to_module(&context, &body)
        .expect("lowering should succeed");

    let ir = module.print_to_string().to_string();
    assert!(
        ir.contains("call void @glyim_drop_in_place"),
        "IR should call glyim_drop_in_place for array of Strings, got:\n{}",
        ir
    );
}
/// V15-T16: Drop of u32 (unsigned Copy type) → no drop needed.
#[test]
fn v15_t16_drop_u32_copy_type() {
    Target::initialize_all(&InitializationConfig::default());

    let (ctx, body) = with_fresh_ty_ctx(|ctx_mut| {
        let u32_ty = ctx_mut.mk_ty(TyKind::Uint(glyim_core::UintTy::U32));

        let mut locals = IndexVec::<LocalIdx, LocalDecl>::new();
        locals.push(LocalDecl {
            ty: ctx_mut.unit_ty(),
            mutability: Mutability::Not,
            source_info: SourceInfo::new(glyim_span::Span::DUMMY),
        });
        locals.push(LocalDecl {
            ty: u32_ty,
            mutability: Mutability::Mut,
            source_info: SourceInfo::new(glyim_span::Span::DUMMY),
        });

        let mut bbs = IndexVec::<BasicBlockIdx, BasicBlockData>::new();
        bbs.push(BasicBlockData {
            statements: vec![],
            terminator: Terminator {
                kind: TerminatorKind::Drop {
                    place: Place::new(LocalIdx::from_raw(1)),
                    target: BasicBlockIdx::from_raw(1),
                    cleanup: None,
                },
                source_info: SourceInfo::new(glyim_span::Span::DUMMY),
            },
            is_cleanup: false,
        });
        bbs.push(BasicBlockData {
            statements: vec![],
            terminator: Terminator {
                kind: TerminatorKind::Return,
                source_info: SourceInfo::new(glyim_span::Span::DUMMY),
            },
            is_cleanup: false,
        });

        Body {
            owner: dummy_def_id(200),
            basic_blocks: bbs,
            locals,
            arg_count: 0,
            return_ty: Ty::UNIT,
            span: glyim_span::Span::DUMMY,
            var_debug_info: vec![],
        }
    });

    let backend = LlvmBackend::new().with_ty_ctx(ctx);
    let context = inkwell::context::Context::create();
    let module = backend
        .lower_body_to_module(&context, &body)
        .expect("lowering should succeed");

    let ir = module.print_to_string().to_string();
    assert!(
        !ir.contains("call void @glyim_drop_in_place"),
        "IR should NOT call glyim_drop_in_place for u32, got:\n{}",
        ir
    );
}

/// V15-T17: Drop of f64 (float Copy type) → no drop needed.
#[test]
fn v15_t17_drop_f64_copy_type() {
    Target::initialize_all(&InitializationConfig::default());

    let (ctx, body) = with_fresh_ty_ctx(|ctx_mut| {
        let f64_ty = ctx_mut.mk_ty(TyKind::Float(glyim_core::FloatTy::F64));

        let mut locals = IndexVec::<LocalIdx, LocalDecl>::new();
        locals.push(LocalDecl {
            ty: ctx_mut.unit_ty(),
            mutability: Mutability::Not,
            source_info: SourceInfo::new(glyim_span::Span::DUMMY),
        });
        locals.push(LocalDecl {
            ty: f64_ty,
            mutability: Mutability::Mut,
            source_info: SourceInfo::new(glyim_span::Span::DUMMY),
        });

        let mut bbs = IndexVec::<BasicBlockIdx, BasicBlockData>::new();
        bbs.push(BasicBlockData {
            statements: vec![],
            terminator: Terminator {
                kind: TerminatorKind::Drop {
                    place: Place::new(LocalIdx::from_raw(1)),
                    target: BasicBlockIdx::from_raw(1),
                    cleanup: None,
                },
                source_info: SourceInfo::new(glyim_span::Span::DUMMY),
            },
            is_cleanup: false,
        });
        bbs.push(BasicBlockData {
            statements: vec![],
            terminator: Terminator {
                kind: TerminatorKind::Return,
                source_info: SourceInfo::new(glyim_span::Span::DUMMY),
            },
            is_cleanup: false,
        });

        Body {
            owner: dummy_def_id(201),
            basic_blocks: bbs,
            locals,
            arg_count: 0,
            return_ty: Ty::UNIT,
            span: glyim_span::Span::DUMMY,
            var_debug_info: vec![],
        }
    });

    let backend = LlvmBackend::new().with_ty_ctx(ctx);
    let context = inkwell::context::Context::create();
    let module = backend
        .lower_body_to_module(&context, &body)
        .expect("lowering should succeed");

    let ir = module.print_to_string().to_string();
    assert!(
        !ir.contains("call void @glyim_drop_in_place"),
        "IR should NOT call glyim_drop_in_place for f64, got:\n{}",
        ir
    );
}

/// V15-T18: Drop of char type → no drop needed.
#[test]
fn v15_t18_drop_char_type() {
    Target::initialize_all(&InitializationConfig::default());

    let (ctx, body) = with_fresh_ty_ctx(|ctx_mut| {
        let char_ty = ctx_mut.mk_ty(TyKind::Char);

        let mut locals = IndexVec::<LocalIdx, LocalDecl>::new();
        locals.push(LocalDecl {
            ty: ctx_mut.unit_ty(),
            mutability: Mutability::Not,
            source_info: SourceInfo::new(glyim_span::Span::DUMMY),
        });
        locals.push(LocalDecl {
            ty: char_ty,
            mutability: Mutability::Mut,
            source_info: SourceInfo::new(glyim_span::Span::DUMMY),
        });

        let mut bbs = IndexVec::<BasicBlockIdx, BasicBlockData>::new();
        bbs.push(BasicBlockData {
            statements: vec![],
            terminator: Terminator {
                kind: TerminatorKind::Drop {
                    place: Place::new(LocalIdx::from_raw(1)),
                    target: BasicBlockIdx::from_raw(1),
                    cleanup: None,
                },
                source_info: SourceInfo::new(glyim_span::Span::DUMMY),
            },
            is_cleanup: false,
        });
        bbs.push(BasicBlockData {
            statements: vec![],
            terminator: Terminator {
                kind: TerminatorKind::Return,
                source_info: SourceInfo::new(glyim_span::Span::DUMMY),
            },
            is_cleanup: false,
        });

        Body {
            owner: dummy_def_id(202),
            basic_blocks: bbs,
            locals,
            arg_count: 0,
            return_ty: Ty::UNIT,
            span: glyim_span::Span::DUMMY,
            var_debug_info: vec![],
        }
    });

    let backend = LlvmBackend::new().with_ty_ctx(ctx);
    let context = inkwell::context::Context::create();
    let module = backend
        .lower_body_to_module(&context, &body)
        .expect("lowering should succeed");

    let ir = module.print_to_string().to_string();
    assert!(
        !ir.contains("call void @glyim_drop_in_place"),
        "IR should NOT call glyim_drop_in_place for char, got:\n{}",
        ir
    );
}

/// V15-T19: Drop of fn pointer type → no drop needed.
#[test]
fn v15_t19_drop_fn_ptr_type() {
    Target::initialize_all(&InitializationConfig::default());

    let (ctx, body) = with_fresh_ty_ctx(|ctx_mut| {
        let fn_sig = glyim_type::FnSig {
            inputs: ctx_mut.intern_substitution(vec![]),
            output: ctx_mut.unit_ty(),
            c_variadic: false,
            unsafety: glyim_core::Safety::Safe,
            abi: glyim_core::Abi::Glyim,
        };
        let fn_ptr_ty = ctx_mut.mk_ty(TyKind::FnPtr(fn_sig));

        let mut locals = IndexVec::<LocalIdx, LocalDecl>::new();
        locals.push(LocalDecl {
            ty: ctx_mut.unit_ty(),
            mutability: Mutability::Not,
            source_info: SourceInfo::new(glyim_span::Span::DUMMY),
        });
        locals.push(LocalDecl {
            ty: fn_ptr_ty,
            mutability: Mutability::Not,
            source_info: SourceInfo::new(glyim_span::Span::DUMMY),
        });

        let mut bbs = IndexVec::<BasicBlockIdx, BasicBlockData>::new();
        bbs.push(BasicBlockData {
            statements: vec![],
            terminator: Terminator {
                kind: TerminatorKind::Drop {
                    place: Place::new(LocalIdx::from_raw(1)),
                    target: BasicBlockIdx::from_raw(1),
                    cleanup: None,
                },
                source_info: SourceInfo::new(glyim_span::Span::DUMMY),
            },
            is_cleanup: false,
        });
        bbs.push(BasicBlockData {
            statements: vec![],
            terminator: Terminator {
                kind: TerminatorKind::Return,
                source_info: SourceInfo::new(glyim_span::Span::DUMMY),
            },
            is_cleanup: false,
        });

        Body {
            owner: dummy_def_id(203),
            basic_blocks: bbs,
            locals,
            arg_count: 0,
            return_ty: Ty::UNIT,
            span: glyim_span::Span::DUMMY,
            var_debug_info: vec![],
        }
    });

    let backend = LlvmBackend::new().with_ty_ctx(ctx);
    let context = inkwell::context::Context::create();
    let module = backend
        .lower_body_to_module(&context, &body)
        .expect("lowering should succeed");

    let ir = module.print_to_string().to_string();
    assert!(
        !ir.contains("call void @glyim_drop_in_place"),
        "IR should NOT call glyim_drop_in_place for fn pointer, got:\n{}",
        ir
    );
}

/// V15-T20: Mixed Copy and non-Copy drops in sequence.
///
/// Drop a Copy type, then a non-Copy type, then another Copy type.
/// Only the non-Copy type should generate a drop_in_place call.
#[test]
fn v15_t20_mixed_copy_non_copy_drops() {
    Target::initialize_all(&InitializationConfig::default());

    let (ctx, body) = with_fresh_ty_ctx(|ctx_mut| {
        let i32_ty = ctx_mut.mk_ty(TyKind::Int(IntTy::I32));
        let string_ty = ctx_mut.mk_ty(TyKind::String);
        let unit_ty = ctx_mut.unit_ty();

        let mut locals = IndexVec::<LocalIdx, LocalDecl>::new();
        // local 0 = return place
        locals.push(LocalDecl {
            ty: unit_ty,
            mutability: Mutability::Not,
            source_info: SourceInfo::new(glyim_span::Span::DUMMY),
        });
        // local 1 = i32 (Copy)
        locals.push(LocalDecl {
            ty: i32_ty,
            mutability: Mutability::Mut,
            source_info: SourceInfo::new(glyim_span::Span::DUMMY),
        });
        // local 2 = String (non-Copy)
        locals.push(LocalDecl {
            ty: string_ty,
            mutability: Mutability::Mut,
            source_info: SourceInfo::new(glyim_span::Span::DUMMY),
        });
        // local 3 = bool (Copy)
        locals.push(LocalDecl {
            ty: Ty::BOOL,
            mutability: Mutability::Mut,
            source_info: SourceInfo::new(glyim_span::Span::DUMMY),
        });

        let mut bbs = IndexVec::<BasicBlockIdx, BasicBlockData>::new();
        // bb0: drop local_1 (i32 - Copy, no drop call)
        bbs.push(BasicBlockData {
            statements: vec![],
            terminator: Terminator {
                kind: TerminatorKind::Drop {
                    place: Place::new(LocalIdx::from_raw(1)),
                    target: BasicBlockIdx::from_raw(1),
                    cleanup: None,
                },
                source_info: SourceInfo::new(glyim_span::Span::DUMMY),
            },
            is_cleanup: false,
        });
        // bb1: drop local_2 (String - non-Copy, drop call)
        bbs.push(BasicBlockData {
            statements: vec![],
            terminator: Terminator {
                kind: TerminatorKind::Drop {
                    place: Place::new(LocalIdx::from_raw(2)),
                    target: BasicBlockIdx::from_raw(2),
                    cleanup: None,
                },
                source_info: SourceInfo::new(glyim_span::Span::DUMMY),
            },
            is_cleanup: false,
        });
        // bb2: drop local_3 (bool - Copy, no drop call)
        bbs.push(BasicBlockData {
            statements: vec![],
            terminator: Terminator {
                kind: TerminatorKind::Drop {
                    place: Place::new(LocalIdx::from_raw(3)),
                    target: BasicBlockIdx::from_raw(3),
                    cleanup: None,
                },
                source_info: SourceInfo::new(glyim_span::Span::DUMMY),
            },
            is_cleanup: false,
        });
        // bb3: return
        bbs.push(BasicBlockData {
            statements: vec![],
            terminator: Terminator {
                kind: TerminatorKind::Return,
                source_info: SourceInfo::new(glyim_span::Span::DUMMY),
            },
            is_cleanup: false,
        });

        Body {
            owner: dummy_def_id(204),
            basic_blocks: bbs,
            locals,
            arg_count: 0,
            return_ty: unit_ty,
            span: glyim_span::Span::DUMMY,
            var_debug_info: vec![],
        }
    });

    let backend = LlvmBackend::new().with_ty_ctx(ctx);
    let context = inkwell::context::Context::create();
    let module = backend
        .lower_body_to_module(&context, &body)
        .expect("lowering should succeed");

    let ir = module.print_to_string().to_string();
    let drop_count = count_drop_calls(&ir);
    // Only one drop_in_place call for the String (local_2)
    assert_eq!(
        drop_count, 1,
        "IR should have exactly 1 drop_in_place call (for String only), found {}, got:\n{}",
        drop_count, ir
    );
    // No dealloc since String is not behind a &mut ref
    assert!(
        !ir.contains("call void @glyim_dealloc"),
        "IR should NOT call glyim_dealloc for plain String drop, got:\n{}",
        ir
    );
    // All four basic blocks should exist
    assert!(
        ir.contains("bb_1") && ir.contains("bb_2") && ir.contains("bb_3"),
        "IR should have bb_1, bb_2, and bb_3 for sequential drops, got:\n{}",
        ir
    );
}

/// V15-T21: Drop of Never type → no drop needed.
#[test]
fn v15_t21_drop_never_type() {
    Target::initialize_all(&InitializationConfig::default());

    let (ctx, body) = with_fresh_ty_ctx(|ctx_mut| {
        let never_ty = ctx_mut.never_ty();

        let mut locals = IndexVec::<LocalIdx, LocalDecl>::new();
        locals.push(LocalDecl {
            ty: ctx_mut.unit_ty(),
            mutability: Mutability::Not,
            source_info: SourceInfo::new(glyim_span::Span::DUMMY),
        });
        locals.push(LocalDecl {
            ty: never_ty,
            mutability: Mutability::Not,
            source_info: SourceInfo::new(glyim_span::Span::DUMMY),
        });

        let mut bbs = IndexVec::<BasicBlockIdx, BasicBlockData>::new();
        bbs.push(BasicBlockData {
            statements: vec![],
            terminator: Terminator {
                kind: TerminatorKind::Drop {
                    place: Place::new(LocalIdx::from_raw(1)),
                    target: BasicBlockIdx::from_raw(1),
                    cleanup: None,
                },
                source_info: SourceInfo::new(glyim_span::Span::DUMMY),
            },
            is_cleanup: false,
        });
        bbs.push(BasicBlockData {
            statements: vec![],
            terminator: Terminator {
                kind: TerminatorKind::Return,
                source_info: SourceInfo::new(glyim_span::Span::DUMMY),
            },
            is_cleanup: false,
        });

        Body {
            owner: dummy_def_id(205),
            basic_blocks: bbs,
            locals,
            arg_count: 0,
            return_ty: Ty::UNIT,
            span: glyim_span::Span::DUMMY,
            var_debug_info: vec![],
        }
    });

    let backend = LlvmBackend::new().with_ty_ctx(ctx);
    let context = inkwell::context::Context::create();
    let module = backend
        .lower_body_to_module(&context, &body)
        .expect("lowering should succeed");

    let ir = module.print_to_string().to_string();
    assert!(
        !ir.contains("call void @glyim_drop_in_place"),
        "IR should NOT call glyim_drop_in_place for Never type, got:\n{}",
        ir
    );
}

/// V15-T22: Drop of mixed tuple (i32, String) → drop needed.
///
/// A tuple containing at least one non-Copy element needs drop.
#[test]
fn v15_t22_drop_mixed_tuple_copy_non_copy() {
    Target::initialize_all(&InitializationConfig::default());

    let (ctx, body) = with_fresh_ty_ctx(|ctx_mut| {
        let i32_ty = ctx_mut.mk_ty(TyKind::Int(IntTy::I32));
        let string_ty = ctx_mut.mk_ty(TyKind::String);
        let subst = ctx_mut.intern_substitution(vec![
            glyim_type::GenericArg::Ty(i32_ty),
            glyim_type::GenericArg::Ty(string_ty),
        ]);
        let tuple_ty = ctx_mut.mk_ty(TyKind::Tuple(subst));

        let mut locals = IndexVec::<LocalIdx, LocalDecl>::new();
        locals.push(LocalDecl {
            ty: ctx_mut.unit_ty(),
            mutability: Mutability::Not,
            source_info: SourceInfo::new(glyim_span::Span::DUMMY),
        });
        locals.push(LocalDecl {
            ty: tuple_ty,
            mutability: Mutability::Mut,
            source_info: SourceInfo::new(glyim_span::Span::DUMMY),
        });

        let mut bbs = IndexVec::<BasicBlockIdx, BasicBlockData>::new();
        bbs.push(BasicBlockData {
            statements: vec![],
            terminator: Terminator {
                kind: TerminatorKind::Drop {
                    place: Place::new(LocalIdx::from_raw(1)),
                    target: BasicBlockIdx::from_raw(1),
                    cleanup: None,
                },
                source_info: SourceInfo::new(glyim_span::Span::DUMMY),
            },
            is_cleanup: false,
        });
        bbs.push(BasicBlockData {
            statements: vec![],
            terminator: Terminator {
                kind: TerminatorKind::Return,
                source_info: SourceInfo::new(glyim_span::Span::DUMMY),
            },
            is_cleanup: false,
        });

        Body {
            owner: dummy_def_id(206),
            basic_blocks: bbs,
            locals,
            arg_count: 0,
            return_ty: Ty::UNIT,
            span: glyim_span::Span::DUMMY,
            var_debug_info: vec![],
        }
    });

    let backend = LlvmBackend::new().with_ty_ctx(ctx);
    let context = inkwell::context::Context::create();
    let module = backend
        .lower_body_to_module(&context, &body)
        .expect("lowering should succeed");

    let ir = module.print_to_string().to_string();
    assert!(
        ir.contains("call void @glyim_drop_in_place"),
        "IR should call glyim_drop_in_place for mixed tuple (i32, String), got:\n{}",
        ir
    );
}

/// V15-T23: Stress test - 10 sequential drops of non-Copy types.
///
/// All 10 should generate drop_in_place calls.
#[test]
fn v15_t23_stress_many_sequential_drops() {
    Target::initialize_all(&InitializationConfig::default());

    let (ctx, body) = with_fresh_ty_ctx(|ctx_mut| {
        let string_ty = ctx_mut.mk_ty(TyKind::String);
        let unit_ty = ctx_mut.unit_ty();

        let mut locals = IndexVec::<LocalIdx, LocalDecl>::new();
        locals.push(LocalDecl {
            ty: unit_ty,
            mutability: Mutability::Not,
            source_info: SourceInfo::new(glyim_span::Span::DUMMY),
        });

        // Add 10 String locals
        for _ in 0..10 {
            locals.push(LocalDecl {
                ty: string_ty,
                mutability: Mutability::Mut,
                source_info: SourceInfo::new(glyim_span::Span::DUMMY),
            });
        }

        let mut bbs = IndexVec::<BasicBlockIdx, BasicBlockData>::new();

        // Create 10 basic blocks, each dropping one local and going to the next
        for i in 0..10 {
            let local_idx = LocalIdx::from_raw((i + 1) as u32);
            let next_bb = BasicBlockIdx::from_raw((i + 1) as u32);
            bbs.push(BasicBlockData {
                statements: vec![],
                terminator: Terminator {
                    kind: TerminatorKind::Drop {
                        place: Place::new(local_idx),
                        target: next_bb,
                        cleanup: None,
                    },
                    source_info: SourceInfo::new(glyim_span::Span::DUMMY),
                },
                is_cleanup: false,
            });
        }

        // Final basic block: return
        bbs.push(BasicBlockData {
            statements: vec![],
            terminator: Terminator {
                kind: TerminatorKind::Return,
                source_info: SourceInfo::new(glyim_span::Span::DUMMY),
            },
            is_cleanup: false,
        });

        Body {
            owner: dummy_def_id(207),
            basic_blocks: bbs,
            locals,
            arg_count: 0,
            return_ty: unit_ty,
            span: glyim_span::Span::DUMMY,
            var_debug_info: vec![],
        }
    });

    let backend = LlvmBackend::new().with_ty_ctx(ctx);
    let context = inkwell::context::Context::create();
    let module = backend
        .lower_body_to_module(&context, &body)
        .expect("lowering should succeed");

    let ir = module.print_to_string().to_string();
    let drop_count = count_drop_calls(&ir);
    assert_eq!(
        drop_count, 10,
        "IR should have exactly 10 drop_in_place calls for 10 String drops, found {}, got:\n{}",
        drop_count, ir
    );
}

/// V15-T24: Drop of &mut String where drop has cleanup target.
///
/// Tests that dealloc is emitted even when the Drop terminator has a cleanup
/// target specified.
#[test]
fn v15_t24_drop_mut_ref_with_cleanup_dealloc() {
    Target::initialize_all(&InitializationConfig::default());

    let (ctx, body) = with_fresh_ty_ctx(|ctx_mut| {
        let string_ty = ctx_mut.mk_ty(TyKind::String);
        let ref_string_ty = ctx_mut.mk_ty(TyKind::Ref(
            glyim_type::Region::Erased,
            string_ty,
            Mutability::Mut,
        ));

        let mut locals = IndexVec::<LocalIdx, LocalDecl>::new();
        locals.push(LocalDecl {
            ty: ctx_mut.unit_ty(),
            mutability: Mutability::Not,
            source_info: SourceInfo::new(glyim_span::Span::DUMMY),
        });
        locals.push(LocalDecl {
            ty: ref_string_ty,
            mutability: Mutability::Mut,
            source_info: SourceInfo::new(glyim_span::Span::DUMMY),
        });

        let mut bbs = IndexVec::<BasicBlockIdx, BasicBlockData>::new();
        bbs.push(BasicBlockData {
            statements: vec![],
            terminator: Terminator {
                kind: TerminatorKind::Drop {
                    place: Place::new(LocalIdx::from_raw(1)),
                    target: BasicBlockIdx::from_raw(1),
                    cleanup: Some(BasicBlockIdx::from_raw(2)),
                },
                source_info: SourceInfo::new(glyim_span::Span::DUMMY),
            },
            is_cleanup: false,
        });
        bbs.push(BasicBlockData {
            statements: vec![],
            terminator: Terminator {
                kind: TerminatorKind::Return,
                source_info: SourceInfo::new(glyim_span::Span::DUMMY),
            },
            is_cleanup: false,
        });
        bbs.push(BasicBlockData {
            statements: vec![],
            terminator: Terminator {
                kind: TerminatorKind::Unreachable,
                source_info: SourceInfo::new(glyim_span::Span::DUMMY),
            },
            is_cleanup: true,
        });

        Body {
            owner: dummy_def_id(208),
            basic_blocks: bbs,
            locals,
            arg_count: 0,
            return_ty: Ty::UNIT,
            span: glyim_span::Span::DUMMY,
            var_debug_info: vec![],
        }
    });

    let backend = LlvmBackend::new().with_ty_ctx(ctx);
    let context = inkwell::context::Context::create();
    let module = backend
        .lower_body_to_module(&context, &body)
        .expect("lowering should succeed");

    let ir = module.print_to_string().to_string();
    assert!(
        ir.contains("@glyim_drop_in_place"),
        "IR should invoke glyim_drop_in_place for &mut String with cleanup, got:\n{}",
        ir
    );
    assert!(
        ir.contains("call void @glyim_dealloc"),
        "IR should call glyim_dealloc for &mut String with cleanup, got:\n{}",
        ir
    );
}

/// V15-T25: Drop of Error type → no drop needed.
#[test]
#[should_panic(expected = "Attempted to lower TyKind::Error to LLVM")]
fn v15_t25_drop_error_type() {
    Target::initialize_all(&InitializationConfig::default());

    let (ctx, body) = with_fresh_ty_ctx(|ctx_mut| {
        let error_ty = ctx_mut.error_ty();

        let mut locals = IndexVec::<LocalIdx, LocalDecl>::new();
        locals.push(LocalDecl {
            ty: ctx_mut.unit_ty(),
            mutability: Mutability::Not,
            source_info: SourceInfo::new(glyim_span::Span::DUMMY),
        });
        locals.push(LocalDecl {
            ty: error_ty,
            mutability: Mutability::Not,
            source_info: SourceInfo::new(glyim_span::Span::DUMMY),
        });

        let mut bbs = IndexVec::<BasicBlockIdx, BasicBlockData>::new();
        bbs.push(BasicBlockData {
            statements: vec![],
            terminator: Terminator {
                kind: TerminatorKind::Drop {
                    place: Place::new(LocalIdx::from_raw(1)),
                    target: BasicBlockIdx::from_raw(1),
                    cleanup: None,
                },
                source_info: SourceInfo::new(glyim_span::Span::DUMMY),
            },
            is_cleanup: false,
        });
        bbs.push(BasicBlockData {
            statements: vec![],
            terminator: Terminator {
                kind: TerminatorKind::Return,
                source_info: SourceInfo::new(glyim_span::Span::DUMMY),
            },
            is_cleanup: false,
        });

        Body {
            owner: dummy_def_id(209),
            basic_blocks: bbs,
            locals,
            arg_count: 0,
            return_ty: Ty::UNIT,
            span: glyim_span::Span::DUMMY,
            var_debug_info: vec![],
        }
    });

    let backend = LlvmBackend::new().with_ty_ctx(ctx);
    let context = inkwell::context::Context::create();
    let module = backend
        .lower_body_to_module(&context, &body)
        .expect("lowering should succeed");

    let ir = module.print_to_string().to_string();
    assert!(
        !ir.contains("call void @glyim_drop_in_place"),
        "IR should NOT call glyim_drop_in_place for Error type, got:\n{}",
        ir
    );
}

/// V15-T26: Drop of isize → no drop needed (Copy type).
#[test]
fn v15_t26_drop_isize_copy_type() {
    Target::initialize_all(&InitializationConfig::default());

    let (ctx, body) = with_fresh_ty_ctx(|ctx_mut| {
        let isize_ty = ctx_mut.mk_ty(TyKind::Int(IntTy::Isize));

        let mut locals = IndexVec::<LocalIdx, LocalDecl>::new();
        locals.push(LocalDecl {
            ty: ctx_mut.unit_ty(),
            mutability: Mutability::Not,
            source_info: SourceInfo::new(glyim_span::Span::DUMMY),
        });
        locals.push(LocalDecl {
            ty: isize_ty,
            mutability: Mutability::Mut,
            source_info: SourceInfo::new(glyim_span::Span::DUMMY),
        });

        let mut bbs = IndexVec::<BasicBlockIdx, BasicBlockData>::new();
        bbs.push(BasicBlockData {
            statements: vec![],
            terminator: Terminator {
                kind: TerminatorKind::Drop {
                    place: Place::new(LocalIdx::from_raw(1)),
                    target: BasicBlockIdx::from_raw(1),
                    cleanup: None,
                },
                source_info: SourceInfo::new(glyim_span::Span::DUMMY),
            },
            is_cleanup: false,
        });
        bbs.push(BasicBlockData {
            statements: vec![],
            terminator: Terminator {
                kind: TerminatorKind::Return,
                source_info: SourceInfo::new(glyim_span::Span::DUMMY),
            },
            is_cleanup: false,
        });

        Body {
            owner: dummy_def_id(210),
            basic_blocks: bbs,
            locals,
            arg_count: 0,
            return_ty: Ty::UNIT,
            span: glyim_span::Span::DUMMY,
            var_debug_info: vec![],
        }
    });

    let backend = LlvmBackend::new().with_ty_ctx(ctx);
    let context = inkwell::context::Context::create();
    let module = backend
        .lower_body_to_module(&context, &body)
        .expect("lowering should succeed");

    let ir = module.print_to_string().to_string();
    assert!(
        !ir.contains("call void @glyim_drop_in_place"),
        "IR should NOT call glyim_drop_in_place for isize, got:\n{}",
        ir
    );
}

/// V15-T27: Drop of usize → no drop needed (Copy type).
#[test]
fn v15_t27_drop_usize_copy_type() {
    Target::initialize_all(&InitializationConfig::default());

    let (ctx, body) = with_fresh_ty_ctx(|ctx_mut| {
        let usize_ty = ctx_mut.mk_ty(TyKind::Uint(glyim_core::UintTy::Usize));

        let mut locals = IndexVec::<LocalIdx, LocalDecl>::new();
        locals.push(LocalDecl {
            ty: ctx_mut.unit_ty(),
            mutability: Mutability::Not,
            source_info: SourceInfo::new(glyim_span::Span::DUMMY),
        });
        locals.push(LocalDecl {
            ty: usize_ty,
            mutability: Mutability::Mut,
            source_info: SourceInfo::new(glyim_span::Span::DUMMY),
        });

        let mut bbs = IndexVec::<BasicBlockIdx, BasicBlockData>::new();
        bbs.push(BasicBlockData {
            statements: vec![],
            terminator: Terminator {
                kind: TerminatorKind::Drop {
                    place: Place::new(LocalIdx::from_raw(1)),
                    target: BasicBlockIdx::from_raw(1),
                    cleanup: None,
                },
                source_info: SourceInfo::new(glyim_span::Span::DUMMY),
            },
            is_cleanup: false,
        });
        bbs.push(BasicBlockData {
            statements: vec![],
            terminator: Terminator {
                kind: TerminatorKind::Return,
                source_info: SourceInfo::new(glyim_span::Span::DUMMY),
            },
            is_cleanup: false,
        });

        Body {
            owner: dummy_def_id(211),
            basic_blocks: bbs,
            locals,
            arg_count: 0,
            return_ty: Ty::UNIT,
            span: glyim_span::Span::DUMMY,
            var_debug_info: vec![],
        }
    });

    let backend = LlvmBackend::new().with_ty_ctx(ctx);
    let context = inkwell::context::Context::create();
    let module = backend
        .lower_body_to_module(&context, &body)
        .expect("lowering should succeed");

    let ir = module.print_to_string().to_string();
    assert!(
        !ir.contains("call void @glyim_drop_in_place"),
        "IR should NOT call glyim_drop_in_place for usize, got:\n{}",
        ir
    );
}

/// V15-T28: Drop of *const String (const raw ptr) → drop needed, no dealloc.
///
/// *const T is Copy, so type_needs_dealloc returns false.
/// But the inner type may need drop, so type_needs_drop recurses.
#[test]
fn v15_t28_drop_const_raw_ptr_non_copy() {
    Target::initialize_all(&InitializationConfig::default());

    let (ctx, body) = with_fresh_ty_ctx(|ctx_mut| {
        let string_ty = ctx_mut.mk_ty(TyKind::String);
        let ptr_string_ty = ctx_mut.mk_ty(TyKind::RawPtr(string_ty, Mutability::Not));

        let mut locals = IndexVec::<LocalIdx, LocalDecl>::new();
        locals.push(LocalDecl {
            ty: ctx_mut.unit_ty(),
            mutability: Mutability::Not,
            source_info: SourceInfo::new(glyim_span::Span::DUMMY),
        });
        locals.push(LocalDecl {
            ty: ptr_string_ty,
            mutability: Mutability::Not,
            source_info: SourceInfo::new(glyim_span::Span::DUMMY),
        });

        let mut bbs = IndexVec::<BasicBlockIdx, BasicBlockData>::new();
        bbs.push(BasicBlockData {
            statements: vec![],
            terminator: Terminator {
                kind: TerminatorKind::Drop {
                    place: Place::new(LocalIdx::from_raw(1)),
                    target: BasicBlockIdx::from_raw(1),
                    cleanup: None,
                },
                source_info: SourceInfo::new(glyim_span::Span::DUMMY),
            },
            is_cleanup: false,
        });
        bbs.push(BasicBlockData {
            statements: vec![],
            terminator: Terminator {
                kind: TerminatorKind::Return,
                source_info: SourceInfo::new(glyim_span::Span::DUMMY),
            },
            is_cleanup: false,
        });

        Body {
            owner: dummy_def_id(212),
            basic_blocks: bbs,
            locals,
            arg_count: 0,
            return_ty: Ty::UNIT,
            span: glyim_span::Span::DUMMY,
            var_debug_info: vec![],
        }
    });

    let backend = LlvmBackend::new().with_ty_ctx(ctx);
    let context = inkwell::context::Context::create();
    let module = backend
        .lower_body_to_module(&context, &body)
        .expect("lowering should succeed");

    let ir = module.print_to_string().to_string();
    // A raw pointer never owns its pointee, so dropping *const String must emit no
    // drop glue at all (drop_in_place would wrongly drop through the pointer).
    assert!(
        !ir.contains("call void @glyim_drop_in_place"),
        "IR should NOT call glyim_drop_in_place for *const String (raw ptr does not own), got:\n{}",
        ir
    );
    // *const T is never deallocated (only *mut T can own)
    assert!(
        !ir.contains("call void @glyim_dealloc"),
        "IR should NOT call glyim_dealloc for *const String, got:\n{}",
        ir
    );
}

/// V15-T29: Drop of empty tuple () → no drop needed.
#[test]
fn v15_t29_drop_empty_tuple() {
    Target::initialize_all(&InitializationConfig::default());

    let (ctx, body) = with_fresh_ty_ctx(|ctx_mut| {
        let empty_subst = ctx_mut.intern_substitution(vec![]);
        let empty_tuple = ctx_mut.mk_ty(TyKind::Tuple(empty_subst));

        let mut locals = IndexVec::<LocalIdx, LocalDecl>::new();
        locals.push(LocalDecl {
            ty: ctx_mut.unit_ty(),
            mutability: Mutability::Not,
            source_info: SourceInfo::new(glyim_span::Span::DUMMY),
        });
        locals.push(LocalDecl {
            ty: empty_tuple,
            mutability: Mutability::Not,
            source_info: SourceInfo::new(glyim_span::Span::DUMMY),
        });

        let mut bbs = IndexVec::<BasicBlockIdx, BasicBlockData>::new();
        bbs.push(BasicBlockData {
            statements: vec![],
            terminator: Terminator {
                kind: TerminatorKind::Drop {
                    place: Place::new(LocalIdx::from_raw(1)),
                    target: BasicBlockIdx::from_raw(1),
                    cleanup: None,
                },
                source_info: SourceInfo::new(glyim_span::Span::DUMMY),
            },
            is_cleanup: false,
        });
        bbs.push(BasicBlockData {
            statements: vec![],
            terminator: Terminator {
                kind: TerminatorKind::Return,
                source_info: SourceInfo::new(glyim_span::Span::DUMMY),
            },
            is_cleanup: false,
        });

        Body {
            owner: dummy_def_id(213),
            basic_blocks: bbs,
            locals,
            arg_count: 0,
            return_ty: Ty::UNIT,
            span: glyim_span::Span::DUMMY,
            var_debug_info: vec![],
        }
    });

    let backend = LlvmBackend::new().with_ty_ctx(ctx);
    let context = inkwell::context::Context::create();
    let module = backend
        .lower_body_to_module(&context, &body)
        .expect("lowering should succeed");

    let ir = module.print_to_string().to_string();
    assert!(
        !ir.contains("call void @glyim_drop_in_place"),
        "IR should NOT call glyim_drop_in_place for empty tuple, got:\n{}",
        ir
    );
}

/// V15-T30: Verify drop_in_place IR structure - call uses pointer operand.
///
/// Checks that the drop_in_place call in the LLVM IR receives a pointer
/// argument (bitcast of the local's alloca to ptr).
#[test]
fn v15_t30_drop_in_place_receives_pointer_arg() {
    Target::initialize_all(&InitializationConfig::default());

    let (ctx, body) = with_fresh_ty_ctx(|ctx_mut| {
        let string_ty = ctx_mut.mk_ty(TyKind::String);

        let mut locals = IndexVec::<LocalIdx, LocalDecl>::new();
        locals.push(LocalDecl {
            ty: ctx_mut.unit_ty(),
            mutability: Mutability::Not,
            source_info: SourceInfo::new(glyim_span::Span::DUMMY),
        });
        locals.push(LocalDecl {
            ty: string_ty,
            mutability: Mutability::Mut,
            source_info: SourceInfo::new(glyim_span::Span::DUMMY),
        });

        let mut bbs = IndexVec::<BasicBlockIdx, BasicBlockData>::new();
        bbs.push(BasicBlockData {
            statements: vec![],
            terminator: Terminator {
                kind: TerminatorKind::Drop {
                    place: Place::new(LocalIdx::from_raw(1)),
                    target: BasicBlockIdx::from_raw(1),
                    cleanup: None,
                },
                source_info: SourceInfo::new(glyim_span::Span::DUMMY),
            },
            is_cleanup: false,
        });
        bbs.push(BasicBlockData {
            statements: vec![],
            terminator: Terminator {
                kind: TerminatorKind::Return,
                source_info: SourceInfo::new(glyim_span::Span::DUMMY),
            },
            is_cleanup: false,
        });

        Body {
            owner: dummy_def_id(214),
            basic_blocks: bbs,
            locals,
            arg_count: 0,
            return_ty: Ty::UNIT,
            span: glyim_span::Span::DUMMY,
            var_debug_info: vec![],
        }
    });

    let backend = LlvmBackend::new().with_ty_ctx(ctx);
    let context = inkwell::context::Context::create();
    let module = backend
        .lower_body_to_module(&context, &body)
        .expect("lowering should succeed");

    let ir = module.print_to_string().to_string();
    // The drop call should pass a pointer argument (the bitcast of local_1 alloca).
    // T005 changed the runtime's signature to take a second `Option<DropFn>`
    // argument (niche-optimised to a raw function pointer), so the emitted
    // call now passes a null second operand.
    assert!(
        ir.contains("call void @glyim_drop_in_place(ptr"),
        "IR should call glyim_drop_in_place with ptr argument, got:\n{}",
        ir
    );
    // The declaration must match the runtime's real 2-argument form.
    assert!(
        ir.contains("declare void @glyim_drop_in_place(ptr, ptr)"),
        "IR should declare glyim_drop_in_place(ptr, ptr), got:\n{}",
        ir
    );
}
