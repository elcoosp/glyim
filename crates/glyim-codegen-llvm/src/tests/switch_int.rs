use crate::LlvmBackend;
use glyim_core::arena::IndexVec;
use glyim_core::primitives::*;
use glyim_core::{CrateId, DefId, IntTy, Interner, LocalDefId};
use glyim_mir::*;
use glyim_type::{Ty, TyCtxMut, TyKind};
use inkwell::context::Context;

fn make_i32_ty(ctx: &mut TyCtxMut) -> Ty {
    ctx.mk_ty(TyKind::Int(IntTy::I32))
}

fn make_bool_ty(ctx: &mut TyCtxMut) -> Ty {
    ctx.bool_ty()
}

fn make_unit_ty(ctx: &mut TyCtxMut) -> Ty {
    ctx.unit_ty()
}

fn make_test_body_switch_small_int() -> (TyCtxMut, Body) {
    let mut ctx = TyCtxMut::new(Interner::default());
    let i32_ty = make_i32_ty(&mut ctx);
    let unit_ty = make_unit_ty(&mut ctx);

    let mut locals: IndexVec<LocalIdx, LocalDecl> = IndexVec::new();
    let _return_local = locals.push(LocalDecl {
        ty: unit_ty,
        mutability: Mutability::Not,
        source_info: SourceInfo::new(glyim_span::Span::DUMMY),
    });
    let _discr_local = locals.push(LocalDecl {
        ty: i32_ty,
        mutability: Mutability::Not,
        source_info: SourceInfo::new(glyim_span::Span::DUMMY),
    });
    let arg_count = 1;

    let mut basic_blocks: IndexVec<BasicBlockIdx, BasicBlockData> = IndexVec::new();

    let bb0 = BasicBlockData::new(Terminator {
        kind: TerminatorKind::SwitchInt {
            discr: Operand::Copy(Place::new(LocalIdx::from_raw(1))),
            switch_ty: i32_ty,
            targets: SwitchTargets::new(
                Box::new([
                    (1u128, BasicBlockIdx::from_raw(1)),
                    (2u128, BasicBlockIdx::from_raw(2)),
                ]),
                BasicBlockIdx::from_raw(3),
            ),
        },
        source_info: SourceInfo::new(glyim_span::Span::DUMMY),
    });
    basic_blocks.push(bb0);

    for _ in 1..=3 {
        basic_blocks.push(BasicBlockData::new(Terminator {
            kind: TerminatorKind::Return,
            source_info: SourceInfo::new(glyim_span::Span::DUMMY),
        }));
    }

    let body = Body {
        owner: DefId::new(CrateId::from_raw(0), LocalDefId::from_raw(0)),
        basic_blocks,
        locals,
        arg_count,
        return_ty: unit_ty,
        span: glyim_span::Span::DUMMY,
        var_debug_info: vec![],
    };

    (ctx, body)
}

fn make_test_body_switch_with_default() -> (TyCtxMut, Body) {
    let mut ctx = TyCtxMut::new(Interner::default());
    let i32_ty = make_i32_ty(&mut ctx);
    let unit_ty = make_unit_ty(&mut ctx);

    let mut locals: IndexVec<LocalIdx, LocalDecl> = IndexVec::new();
    let _return_local = locals.push(LocalDecl {
        ty: unit_ty,
        mutability: Mutability::Not,
        source_info: SourceInfo::new(glyim_span::Span::DUMMY),
    });
    let _discr_local = locals.push(LocalDecl {
        ty: i32_ty,
        mutability: Mutability::Not,
        source_info: SourceInfo::new(glyim_span::Span::DUMMY),
    });
    let arg_count = 1;

    let mut basic_blocks: IndexVec<BasicBlockIdx, BasicBlockData> = IndexVec::new();

    let bb0 = BasicBlockData::new(Terminator {
        kind: TerminatorKind::SwitchInt {
            discr: Operand::Copy(Place::new(LocalIdx::from_raw(1))),
            switch_ty: i32_ty,
            targets: SwitchTargets::new(
                Box::new([(10u128, BasicBlockIdx::from_raw(1))]),
                BasicBlockIdx::from_raw(2),
            ),
        },
        source_info: SourceInfo::new(glyim_span::Span::DUMMY),
    });
    basic_blocks.push(bb0);

    for _ in 1..=2 {
        basic_blocks.push(BasicBlockData::new(Terminator {
            kind: TerminatorKind::Return,
            source_info: SourceInfo::new(glyim_span::Span::DUMMY),
        }));
    }

    let body = Body {
        owner: DefId::new(CrateId::from_raw(0), LocalDefId::from_raw(0)),
        basic_blocks,
        locals,
        arg_count,
        return_ty: unit_ty,
        span: glyim_span::Span::DUMMY,
        var_debug_info: vec![],
    };

    (ctx, body)
}

fn make_test_body_switch_bool() -> (TyCtxMut, Body) {
    let mut ctx = TyCtxMut::new(Interner::default());
    let bool_ty = make_bool_ty(&mut ctx);
    let unit_ty = make_unit_ty(&mut ctx);

    let mut locals: IndexVec<LocalIdx, LocalDecl> = IndexVec::new();
    let _return_local = locals.push(LocalDecl {
        ty: unit_ty,
        mutability: Mutability::Not,
        source_info: SourceInfo::new(glyim_span::Span::DUMMY),
    });
    let _discr_local = locals.push(LocalDecl {
        ty: bool_ty,
        mutability: Mutability::Not,
        source_info: SourceInfo::new(glyim_span::Span::DUMMY),
    });
    let arg_count = 1;

    let mut basic_blocks: IndexVec<BasicBlockIdx, BasicBlockData> = IndexVec::new();

    let bb0 = BasicBlockData::new(Terminator {
        kind: TerminatorKind::SwitchInt {
            discr: Operand::Copy(Place::new(LocalIdx::from_raw(1))),
            switch_ty: bool_ty,
            targets: SwitchTargets::new(
                Box::new([
                    (0u128, BasicBlockIdx::from_raw(1)),
                    (1u128, BasicBlockIdx::from_raw(2)),
                ]),
                BasicBlockIdx::from_raw(3),
            ),
        },
        source_info: SourceInfo::new(glyim_span::Span::DUMMY),
    });
    basic_blocks.push(bb0);

    for _ in 1..=3 {
        basic_blocks.push(BasicBlockData::new(Terminator {
            kind: TerminatorKind::Return,
            source_info: SourceInfo::new(glyim_span::Span::DUMMY),
        }));
    }

    let body = Body {
        owner: DefId::new(CrateId::from_raw(0), LocalDefId::from_raw(0)),
        basic_blocks,
        locals,
        arg_count,
        return_ty: unit_ty,
        span: glyim_span::Span::DUMMY,
        var_debug_info: vec![],
    };

    (ctx, body)
}

fn make_test_body_switch_large() -> (TyCtxMut, Body) {
    let mut ctx = TyCtxMut::new(Interner::default());
    let i32_ty = make_i32_ty(&mut ctx);
    let unit_ty = make_unit_ty(&mut ctx);

    let mut locals: IndexVec<LocalIdx, LocalDecl> = IndexVec::new();
    let _return_local = locals.push(LocalDecl {
        ty: unit_ty,
        mutability: Mutability::Not,
        source_info: SourceInfo::new(glyim_span::Span::DUMMY),
    });
    let _discr_local = locals.push(LocalDecl {
        ty: i32_ty,
        mutability: Mutability::Not,
        source_info: SourceInfo::new(glyim_span::Span::DUMMY),
    });
    let arg_count = 1;

    let num_cases = 25u128;
    let mut branches: Vec<(u128, BasicBlockIdx)> = Vec::new();
    for i in 0..num_cases {
        branches.push((i, BasicBlockIdx::from_raw((i + 1) as u32)));
    }
    let otherwise_bb = BasicBlockIdx::from_raw((num_cases + 1) as u32);

    let mut basic_blocks: IndexVec<BasicBlockIdx, BasicBlockData> = IndexVec::new();

    let bb0 = BasicBlockData::new(Terminator {
        kind: TerminatorKind::SwitchInt {
            discr: Operand::Copy(Place::new(LocalIdx::from_raw(1))),
            switch_ty: i32_ty,
            targets: SwitchTargets::new(branches.into_boxed_slice(), otherwise_bb),
        },
        source_info: SourceInfo::new(glyim_span::Span::DUMMY),
    });
    basic_blocks.push(bb0);

    for _ in 0..=num_cases {
        basic_blocks.push(BasicBlockData::new(Terminator {
            kind: TerminatorKind::Return,
            source_info: SourceInfo::new(glyim_span::Span::DUMMY),
        }));
    }

    let body = Body {
        owner: DefId::new(CrateId::from_raw(0), LocalDefId::from_raw(0)),
        basic_blocks,
        locals,
        arg_count,
        return_ty: unit_ty,
        span: glyim_span::Span::DUMMY,
        var_debug_info: vec![],
    };

    (ctx, body)
}

fn lower_and_get_ir_with_ctx(ctx: TyCtxMut, body: &Body) -> String {
    let frozen = ctx.freeze();
    let backend = LlvmBackend::new().with_ty_ctx(frozen);
    let context = Context::create();
    let module = backend
        .lower_body_to_module(&context, body)
        .expect("lowering failed");
    module.print_to_string().to_string()
}

#[test]
fn v29_t01_switch_small_integer_range() {
    let (ctx, body) = make_test_body_switch_small_int();
    let ir = lower_and_get_ir_with_ctx(ctx, &body);
    assert!(
        ir.contains("switch") || ir.contains("icmp eq"),
        "Expected LLVM switch or icmp eq in IR, got:\n{ir}"
    );
    assert!(
        ir.contains("i32 1") || ir.contains("1,"),
        "Expected case value 1 in IR, got:\n{ir}"
    );
    assert!(
        ir.contains("i32 2") || ir.contains("2,"),
        "Expected case value 2 in IR, got:\n{ir}"
    );
}

#[test]
fn v29_t02_switch_with_default_branch() {
    let (ctx, body) = make_test_body_switch_with_default();
    let ir = lower_and_get_ir_with_ctx(ctx, &body);
    assert!(
        ir.contains("switch") || ir.contains("icmp eq"),
        "Expected LLVM switch or icmp eq in IR, got:\n{ir}"
    );
    assert!(
        ir.contains(", 10") || ir.contains("i32 10"),
        "Expected case value 10 in IR, got:\n{ir}"
    );
}

#[test]
fn v29_t03_switch_on_bool_icmp_branch() {
    let (ctx, body) = make_test_body_switch_bool();
    let ir = lower_and_get_ir_with_ctx(ctx, &body);
    assert!(
        ir.contains("switch i1"),
        "Expected LLVM switch i1 for bool switch, got:\n{ir}"
    );
}

#[test]
fn v29_t04_large_switch_jump_table() {
    let (ctx, body) = make_test_body_switch_large();
    let ir = lower_and_get_ir_with_ctx(ctx, &body);
    assert!(
        ir.contains("switch"),
        "Expected LLVM switch instruction for large switch, got:\n{ir}"
    );
    let switch_count = ir.matches("switch").count();
    assert!(
        switch_count >= 1,
        "Expected at least one switch instruction, got {switch_count} in:\n{ir}"
    );
}
