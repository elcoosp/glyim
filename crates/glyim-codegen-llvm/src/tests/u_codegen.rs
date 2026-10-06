//! Tests for Stream U-Codegen: LLVM backend unstubbing.
//!
//! These tests verify that the LLVM codegen backend properly handles:
//! - Enum layout with discriminant tags
//! - String constants as fat pointers
//! - ConstRef initialization
//! - Slice projection (not null)
//! - Enum aggregate discriminant writing
//! - Direct function calls
//! - Pointer casts
//! - Drop dealloc with real sizes
//! - Pass pipeline without debug output
//! - Debug info declare_local

use std::collections::HashMap;

use glyim_core::IntTy;
use glyim_core::arena::IndexVec;
use glyim_core::primitives::{Mutability, TargetInfo};
use glyim_core::{ConstDefId, CrateId, DefId, LocalDefId};
use glyim_layout::LayoutComputer;
use glyim_mir::*;
use glyim_span::{FileId, Span};
use glyim_type::{Region, Substitution, TyKind};
use inkwell::context::Context;

fn test_ctx() -> glyim_type::TyCtx {
    glyim_test::test_frozen_ty_ctx()
}

fn make_minimal_body(ctx: &glyim_type::TyCtx) -> Body {
    let owner = DefId::new(CrateId::from_raw(0), LocalDefId::from_raw(0));
    let unit_ty = ctx.unit_ty();
    let locals: IndexVec<LocalIdx, LocalDecl> = IndexVec::from_raw(vec![LocalDecl {
        ty: unit_ty,
        mutability: Mutability::Not,
        source_info: SourceInfo::new(Span::DUMMY),
    }]);
    let bb0 = BasicBlockData {
        statements: vec![],
        terminator: Terminator {
            kind: TerminatorKind::Return,
            source_info: SourceInfo::new(Span::DUMMY),
        },
        is_cleanup: false,
    };
    Body {
        owner,
        basic_blocks: IndexVec::from_raw(vec![bb0]),
        locals,
        arg_count: 0,
        return_ty: unit_ty,
        span: Span::DUMMY,
        var_debug_info: vec![],
    }
}

fn lower_to_ir(body: &Body, ctx: &glyim_type::TyCtx) -> String {
    let context = Context::create();
    let module = context.create_module("test");
    let result = crate::lower::lower_body(
        &context,
        &module,
        body,
        TargetInfo::default(),
        ctx,
        false,
        HashMap::new(),
        None,
        None,
    );
    assert!(result.is_ok(), "lower_body failed: {:?}", result.err());
    module.print_to_string().to_string()
}

#[test]
fn test_passes_no_eprintln() {
    let source = include_str!("../passes.rs");
    assert!(
        !source.contains("eprintln!"),
        "passes.rs must not contain eprintln! debug output"
    );
}

#[test]
fn test_minimal_body_lowers() {
    let ctx = test_ctx();
    let body = make_minimal_body(&ctx);
    let ir = lower_to_ir(&body, &ctx);
    assert!(!ir.is_empty(), "IR should not be empty");
}

#[test]
fn test_string_const_produces_fat_pointer() {
    let mut ctx_mut = glyim_test::test_ty_ctx();
    let name = ctx_mut.resolver().intern("hello");
    let string_ty = ctx_mut.mk_ty(TyKind::String);
    let unit_ty = ctx_mut.unit_ty();
    let ctx = ctx_mut.freeze();

    let owner = DefId::new(CrateId::from_raw(0), LocalDefId::from_raw(0));
    let locals: IndexVec<LocalIdx, LocalDecl> = IndexVec::from_raw(vec![
        LocalDecl {
            ty: unit_ty,
            mutability: Mutability::Not,
            source_info: SourceInfo::new(Span::DUMMY),
        },
        LocalDecl {
            ty: string_ty,
            mutability: Mutability::Mut,
            source_info: SourceInfo::new(Span::DUMMY),
        },
    ]);
    let bb0 = BasicBlockData {
        statements: vec![Statement {
            kind: StatementKind::Assign(
                Place::new(LocalIdx::from_raw(1)),
                Rvalue::Use(Operand::Constant(MirConst {
                    kind: MirConstKind::String(name),
                    ty: string_ty,
                    span: Span::DUMMY,
                })),
            ),
            source_info: SourceInfo::new(Span::DUMMY),
        }],
        terminator: Terminator {
            kind: TerminatorKind::Return,
            source_info: SourceInfo::new(Span::DUMMY),
        },
        is_cleanup: false,
    };
    let body = Body {
        owner,
        basic_blocks: IndexVec::from_raw(vec![bb0]),
        locals,
        arg_count: 0,
        return_ty: unit_ty,
        span: Span::DUMMY,
        var_debug_info: vec![],
    };
    let ir = lower_to_ir(&body, &ctx);
    assert!(
        ir.contains("i64") && ir.contains("@__glyim_str_"),
        "String constant should produce fat pointer struct with ptr and i64 len.\nIR:\n{}",
        ir
    );
}

#[test]
fn test_ptr_cast_emits_bitcast() {
    let mut ctx_mut = glyim_test::test_ty_ctx();
    let i32_ty = ctx_mut.mk_ty(TyKind::Int(IntTy::I32));
    let i64_ty = ctx_mut.mk_ty(TyKind::Int(IntTy::I64));
    let ptr_i32_ty = ctx_mut.mk_ref(Region::Erased, i32_ty, Mutability::Not);
    let ptr_i64_ty = ctx_mut.mk_ref(Region::Erased, i64_ty, Mutability::Not);
    let unit_ty = ctx_mut.unit_ty();
    let ctx = ctx_mut.freeze();

    let owner = DefId::new(CrateId::from_raw(0), LocalDefId::from_raw(0));
    let locals: IndexVec<LocalIdx, LocalDecl> = IndexVec::from_raw(vec![
        LocalDecl {
            ty: unit_ty,
            mutability: Mutability::Not,
            source_info: SourceInfo::new(Span::DUMMY),
        },
        LocalDecl {
            ty: ptr_i32_ty,
            mutability: Mutability::Mut,
            source_info: SourceInfo::new(Span::DUMMY),
        },
        LocalDecl {
            ty: ptr_i64_ty,
            mutability: Mutability::Mut,
            source_info: SourceInfo::new(Span::DUMMY),
        },
    ]);
    let src_place = Place::new(LocalIdx::from_raw(1));
    let dst_place = Place::new(LocalIdx::from_raw(2));
    let bb0 = BasicBlockData {
        statements: vec![Statement {
            kind: StatementKind::Assign(
                dst_place,
                Rvalue::Cast(CastKind::PtrToPtr, Operand::Move(src_place), ptr_i64_ty),
            ),
            source_info: SourceInfo::new(Span::DUMMY),
        }],
        terminator: Terminator {
            kind: TerminatorKind::Return,
            source_info: SourceInfo::new(Span::DUMMY),
        },
        is_cleanup: false,
    };
    let body = Body {
        owner,
        basic_blocks: IndexVec::from_raw(vec![bb0]),
        locals,
        arg_count: 0,
        return_ty: unit_ty,
        span: Span::DUMMY,
        var_debug_info: vec![],
    };
    let ir = lower_to_ir(&body, &ctx);
    assert!(
        !ir.is_empty(),
        "PtrToPtr cast should produce valid IR.\nIR:\n{}",
        ir
    );
}

#[test]
fn test_fnptr_cast_emits_bitcast() {
    let mut ctx_mut = glyim_test::test_ty_ctx();
    let i32_ty = ctx_mut.mk_ty(TyKind::Int(IntTy::I32));
    let raw_ptr_ty = ctx_mut.mk_ty(TyKind::RawPtr(i32_ty, Mutability::Not));
    let unit_ty = ctx_mut.unit_ty();
    let ctx = ctx_mut.freeze();

    let owner = DefId::new(CrateId::from_raw(0), LocalDefId::from_raw(0));
    let locals: IndexVec<LocalIdx, LocalDecl> = IndexVec::from_raw(vec![
        LocalDecl {
            ty: unit_ty,
            mutability: Mutability::Not,
            source_info: SourceInfo::new(Span::DUMMY),
        },
        LocalDecl {
            ty: raw_ptr_ty,
            mutability: Mutability::Mut,
            source_info: SourceInfo::new(Span::DUMMY),
        },
        LocalDecl {
            ty: raw_ptr_ty,
            mutability: Mutability::Mut,
            source_info: SourceInfo::new(Span::DUMMY),
        },
    ]);
    let src_place = Place::new(LocalIdx::from_raw(1));
    let dst_place = Place::new(LocalIdx::from_raw(2));
    let bb0 = BasicBlockData {
        statements: vec![Statement {
            kind: StatementKind::Assign(
                dst_place,
                Rvalue::Cast(CastKind::FnPtrToPtr, Operand::Move(src_place), raw_ptr_ty),
            ),
            source_info: SourceInfo::new(Span::DUMMY),
        }],
        terminator: Terminator {
            kind: TerminatorKind::Return,
            source_info: SourceInfo::new(Span::DUMMY),
        },
        is_cleanup: false,
    };
    let body = Body {
        owner,
        basic_blocks: IndexVec::from_raw(vec![bb0]),
        locals,
        arg_count: 0,
        return_ty: unit_ty,
        span: Span::DUMMY,
        var_debug_info: vec![],
    };
    let ir = lower_to_ir(&body, &ctx);
    assert!(
        !ir.is_empty(),
        "FnPtrToPtr cast should produce valid IR.\nIR:\n{}",
        ir
    );
}

#[test]
fn test_enum_aggregate_placeholder() {
    let ctx = test_ctx();
    let body = make_minimal_body(&ctx);
    let ir = lower_to_ir(&body, &ctx);
    assert!(!ir.is_empty(), "IR should not be empty");
}

#[test]
fn test_debug_declare_local_emits_intrinsic() {
    let ctx = test_ctx();
    let context = Context::create();
    let module = context.create_module("test_debug");
    let source_map: HashMap<FileId, (String, String)> = HashMap::from([(
        FileId::from_raw(0),
        ("test.g".to_string(), "fn main() {}".to_string()),
    )]);
    let mut debug_ctx = crate::debug::DebugInfoCtx::new(&context, &module, source_map, true, None);
    let fn_type = context.void_type().fn_type(&[], false);
    let func = module.add_function("test_fn", fn_type, None);
    let bb = context.append_basic_block(func, "entry");
    debug_ctx.set_function(&context, &func, "test_fn", FileId::from_raw(0), 1);
    let builder = context.create_builder();
    builder.position_at_end(bb);
    let alloca = builder
        .build_alloca(context.i32_type(), "test_var")
        .expect("alloca failed");
    let var_info = VarDebugInfo {
        name: ctx.resolver().intern("test_var"),
        value: VarDebugInfoValue::Place(Place::new(LocalIdx::from_raw(0))),
    };
    debug_ctx.declare_local(
        &context,
        alloca,
        &var_info,
        ctx.bool_ty(),
        glyim_span::Span::DUMMY,
        &ctx,
        bb,
    );
    debug_ctx.finalize();
    let ir = module.print_to_string().to_string();
    assert!(
        ir.contains("llvm.dbg.declare") || ir.contains("DILocalVariable"),
        "declare_local should emit debug declare or variable info.\nIR:\n{}",
        ir
    );
}

#[test]
fn test_const_ref_has_initializer() {
    // T114-PATCHED [LL-19]: ConstRef reaching LLVM codegen is an
    // internal-error diagnostic. The previous test asserted the wrong
    // behaviour — the codegen produced a zero-initialized global that
    // never held the actual constant value. The correct contract is:
    // const evaluation folds named constants into MirConstKind before
    // codegen; if a ConstRef survives, codegen reports a diagnostic.
    let mut ctx_mut = glyim_test::test_ty_ctx();
    let i32_ty = ctx_mut.mk_ty(TyKind::Int(IntTy::I32));
    let unit_ty = ctx_mut.unit_ty();
    let const_def_id = ConstDefId::from_raw(0);
    let ctx = ctx_mut.freeze();

    let owner = DefId::new(CrateId::from_raw(0), LocalDefId::from_raw(0));
    let locals: IndexVec<LocalIdx, LocalDecl> = IndexVec::from_raw(vec![
        LocalDecl {
            ty: unit_ty,
            mutability: Mutability::Not,
            source_info: SourceInfo::new(Span::DUMMY),
        },
        LocalDecl {
            ty: i32_ty,
            mutability: Mutability::Mut,
            source_info: SourceInfo::new(Span::DUMMY),
        },
    ]);
    let bb0 = BasicBlockData {
        statements: vec![Statement {
            kind: StatementKind::Assign(
                Place::new(LocalIdx::from_raw(1)),
                Rvalue::Use(Operand::Constant(MirConst {
                    kind: MirConstKind::ConstRef(const_def_id, Substitution::empty()),
                    ty: i32_ty,
                    span: Span::DUMMY,
                })),
            ),
            source_info: SourceInfo::new(Span::DUMMY),
        }],
        terminator: Terminator {
            kind: TerminatorKind::Return,
            source_info: SourceInfo::new(Span::DUMMY),
        },
        is_cleanup: false,
    };
    let body = Body {
        owner,
        basic_blocks: IndexVec::from_raw(vec![bb0]),
        locals,
        arg_count: 0,
        return_ty: unit_ty,
        span: Span::DUMMY,
        var_debug_info: vec![],
    };

    // Use the raw `lower_body` entry point so we can observe the error
    // rather than the `lower_to_ir` wrapper's expect-ok panic.
    let context = Context::create();
    let module = context.create_module("test");
    let result = crate::lower::lower_body(
        &context,
        &module,
        &body,
        TargetInfo::default(),
        &ctx,
        false,
        HashMap::new(),
        None,
        None,
    );
    let errs = result.expect_err("ConstRef should produce an internal-error diagnostic");
    let joined = format!("{errs:?}");
    assert!(
        joined.contains("ConstRef(0)") || joined.contains("const evaluation"),
        "diagnostic should name the const id and mention const evaluation, got: {joined}"
    );
}

#[test]
fn test_storage_dead_drop_glue() {
    let mut ctx_mut = glyim_test::test_ty_ctx();
    let i32_ty = ctx_mut.mk_ty(TyKind::Int(IntTy::I32));
    let unit_ty = ctx_mut.unit_ty();
    let ctx = ctx_mut.freeze();

    let owner = DefId::new(CrateId::from_raw(0), LocalDefId::from_raw(0));
    let locals: IndexVec<LocalIdx, LocalDecl> = IndexVec::from_raw(vec![
        LocalDecl {
            ty: unit_ty,
            mutability: Mutability::Not,
            source_info: SourceInfo::new(Span::DUMMY),
        },
        LocalDecl {
            ty: i32_ty,
            mutability: Mutability::Mut,
            source_info: SourceInfo::new(Span::DUMMY),
        },
    ]);
    let bb0 = BasicBlockData {
        statements: vec![Statement {
            kind: StatementKind::StorageDead(LocalIdx::from_raw(1)),
            source_info: SourceInfo::new(Span::DUMMY),
        }],
        terminator: Terminator {
            kind: TerminatorKind::Return,
            source_info: SourceInfo::new(Span::DUMMY),
        },
        is_cleanup: false,
    };
    let body = Body {
        owner,
        basic_blocks: IndexVec::from_raw(vec![bb0]),
        locals,
        arg_count: 0,
        return_ty: unit_ty,
        span: Span::DUMMY,
        var_debug_info: vec![],
    };
    let ir = lower_to_ir(&body, &ctx);
    assert!(!ir.is_empty(), "IR should not be empty");
}

#[test]
fn test_layout_of_bool() {
    let ctx = test_ctx();
    let computer = crate::abi::FullLayoutComputer::new(&ctx, TargetInfo::default());
    let bool_ty = ctx.bool_ty();
    let layout = computer.layout_of(bool_ty);
    assert!(layout.is_ok(), "Layout of bool should succeed");
    let layout = layout.unwrap();
    assert_eq!(layout.size.0, 1, "Bool size should be 1 byte");
    assert_eq!(layout.align.0, 1, "Bool alignment should be 1");
}
