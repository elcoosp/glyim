use crate::LlvmBackend;
use glyim_core::arena::IndexVec;
use glyim_core::primitives::*;
use glyim_core::{CrateId, DefId, IntTy, Interner, LocalDefId, UintTy};
use glyim_mir::*;
use glyim_type::{Ty, TyCtxMut, TyKind};
use inkwell::context::Context;

fn make_i32_ty(ctx: &mut TyCtxMut) -> Ty {
    ctx.mk_ty(TyKind::Int(IntTy::I32))
}

fn make_u8_ty(ctx: &mut TyCtxMut) -> Ty {
    ctx.mk_ty(TyKind::Uint(UintTy::U8))
}

fn make_u64_ty(ctx: &mut TyCtxMut) -> Ty {
    ctx.mk_ty(TyKind::Uint(UintTy::U64))
}

fn make_i64_ty(ctx: &mut TyCtxMut) -> Ty {
    ctx.mk_ty(TyKind::Int(IntTy::I64))
}

fn make_bool_ty(ctx: &mut TyCtxMut) -> Ty {
    ctx.bool_ty()
}

fn make_unit_ty(ctx: &mut TyCtxMut) -> Ty {
    ctx.unit_ty()
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

fn build_switch_body(ctx: &mut TyCtxMut, switch_ty: Ty, targets: SwitchTargets) -> Body {
    let unit_ty = make_unit_ty(ctx);

    let mut locals: IndexVec<LocalIdx, LocalDecl> = IndexVec::new();
    let _return_local = locals.push(LocalDecl {
        ty: unit_ty,
        mutability: Mutability::Not,
        source_info: SourceInfo::new(glyim_span::Span::DUMMY),
    });
    let _discr_local = locals.push(LocalDecl {
        ty: switch_ty,
        mutability: Mutability::Not,
        source_info: SourceInfo::new(glyim_span::Span::DUMMY),
    });
    let arg_count = 1;

    let otherwise = targets.otherwise();
    let max_bb = targets
        .iter()
        .map(|(_, bb)| bb.to_raw())
        .max()
        .unwrap_or(0)
        .max(otherwise.to_raw());

    let mut basic_blocks: IndexVec<BasicBlockIdx, BasicBlockData> = IndexVec::new();

    let bb0 = BasicBlockData::new(Terminator {
        kind: TerminatorKind::SwitchInt {
            discr: Operand::Copy(Place::new(LocalIdx::from_raw(1))),
            switch_ty,
            targets,
        },
        source_info: SourceInfo::new(glyim_span::Span::DUMMY),
    });
    basic_blocks.push(bb0);

    for _ in 1..=(max_bb as usize) {
        basic_blocks.push(BasicBlockData::new(Terminator {
            kind: TerminatorKind::Return,
            source_info: SourceInfo::new(glyim_span::Span::DUMMY),
        }));
    }

    Body {
        owner: DefId::new(CrateId::from_raw(0), LocalDefId::from_raw(0)),
        basic_blocks,
        locals,
        arg_count,
        return_ty: unit_ty,
        span: glyim_span::Span::DUMMY,
        var_debug_info: vec![],
    }
}

#[test]
fn v29_t05_bool_switch_only_true_branch() {
    let mut ctx = TyCtxMut::new(Interner::default());
    let bool_ty = make_bool_ty(&mut ctx);

    let targets = SwitchTargets::new(
        Box::new([(1u128, BasicBlockIdx::from_raw(1))]),
        BasicBlockIdx::from_raw(2),
    );

    let body = build_switch_body(&mut ctx, bool_ty, targets);
    let ir = lower_and_get_ir_with_ctx(ctx, &body);

    assert!(
        ir.contains("switch i1"),
        "Expected LLVM switch for bool switch with only true branch, got:\n{ir}"
    );
    assert!(
        !ir.contains("switch_eq_0"),
        "Expected no 'switch_eq_0' label for production switch, got:\n{ir}"
    );
}

#[test]
fn v29_t06_bool_switch_only_false_branch() {
    let mut ctx = TyCtxMut::new(Interner::default());
    let bool_ty = make_bool_ty(&mut ctx);

    let targets = SwitchTargets::new(
        Box::new([(0u128, BasicBlockIdx::from_raw(1))]),
        BasicBlockIdx::from_raw(2),
    );

    let body = build_switch_body(&mut ctx, bool_ty, targets);
    let ir = lower_and_get_ir_with_ctx(ctx, &body);

    assert!(
        ir.contains("switch i1"),
        "Expected LLVM switch for bool switch with only false branch, got:\n{ir}"
    );
}

#[test]
fn v29_t07_empty_switch_always_default() {
    let mut ctx = TyCtxMut::new(Interner::default());
    let i32_ty = make_i32_ty(&mut ctx);

    let targets = SwitchTargets::new(Box::new([]), BasicBlockIdx::from_raw(1));

    let body = build_switch_body(&mut ctx, i32_ty, targets);
    let ir = lower_and_get_ir_with_ctx(ctx, &body);

    assert!(
        ir.contains("br label"),
        "Expected unconditional branch for empty switch, got:\n{ir}"
    );
}

#[test]
fn v29_t08_u8_switch_medium_range() {
    let mut ctx = TyCtxMut::new(Interner::default());
    let u8_ty = make_u8_ty(&mut ctx);

    let targets = SwitchTargets::new(
        Box::new([
            (0u128, BasicBlockIdx::from_raw(1)),
            (1u128, BasicBlockIdx::from_raw(2)),
            (2u128, BasicBlockIdx::from_raw(3)),
            (3u128, BasicBlockIdx::from_raw(4)),
            (4u128, BasicBlockIdx::from_raw(5)),
        ]),
        BasicBlockIdx::from_raw(6),
    );

    let body = build_switch_body(&mut ctx, u8_ty, targets);
    let ir = lower_and_get_ir_with_ctx(ctx, &body);

    assert!(
        ir.contains("switch"),
        "Expected LLVM switch for u8 medium-range switch (5 cases), got:\n{ir}"
    );
    assert!(
        ir.contains("i8"),
        "Expected i8 type in switch for u8 discriminant, got:\n{ir}"
    );
}

#[test]
fn v29_t09_u64_switch_large_values() {
    let mut ctx = TyCtxMut::new(Interner::default());
    let u64_ty = make_u64_ty(&mut ctx);

    let targets = SwitchTargets::new(
        Box::new([
            (100u128, BasicBlockIdx::from_raw(1)),
            (1000u128, BasicBlockIdx::from_raw(2)),
            (1000000u128, BasicBlockIdx::from_raw(3)),
            (999999999u128, BasicBlockIdx::from_raw(4)),
            (u128::from(u64::MAX), BasicBlockIdx::from_raw(5)),
        ]),
        BasicBlockIdx::from_raw(6),
    );

    let body = build_switch_body(&mut ctx, u64_ty, targets);
    let ir = lower_and_get_ir_with_ctx(ctx, &body);

    assert!(
        ir.contains("switch"),
        "Expected LLVM switch for u64 large values, got:\n{ir}"
    );
    assert!(
        ir.contains("i64"),
        "Expected i64 type in switch for u64 discriminant, got:\n{ir}"
    );
}

#[test]
fn v29_t10_i64_signed_switch() {
    let mut ctx = TyCtxMut::new(Interner::default());
    let i64_ty = make_i64_ty(&mut ctx);

    let targets = SwitchTargets::new(
        Box::new([
            (0u128, BasicBlockIdx::from_raw(1)),
            (1u128, BasicBlockIdx::from_raw(2)),
            (2u128, BasicBlockIdx::from_raw(3)),
            (3u128, BasicBlockIdx::from_raw(4)),
            (4u128, BasicBlockIdx::from_raw(5)),
        ]),
        BasicBlockIdx::from_raw(6),
    );

    let body = build_switch_body(&mut ctx, i64_ty, targets);
    let ir = lower_and_get_ir_with_ctx(ctx, &body);

    assert!(
        ir.contains("switch"),
        "Expected LLVM switch for i64 signed switch (5 cases), got:\n{ir}"
    );
}

#[test]
fn v29_t11_bool_switch_empty_branches() {
    let mut ctx = TyCtxMut::new(Interner::default());
    let bool_ty = make_bool_ty(&mut ctx);

    let targets = SwitchTargets::new(Box::new([]), BasicBlockIdx::from_raw(1));

    let body = build_switch_body(&mut ctx, bool_ty, targets);
    let ir = lower_and_get_ir_with_ctx(ctx, &body);

    assert!(
        ir.contains("br label"),
        "Expected unconditional branch for empty bool switch, got:\n{ir}"
    );
}

#[test]
fn v29_t12_three_case_switch_uses_llvm_switch() {
    let mut ctx = TyCtxMut::new(Interner::default());
    let i32_ty = make_i32_ty(&mut ctx);

    let targets = SwitchTargets::new(
        Box::new([
            (5u128, BasicBlockIdx::from_raw(1)),
            (10u128, BasicBlockIdx::from_raw(2)),
            (15u128, BasicBlockIdx::from_raw(3)),
        ]),
        BasicBlockIdx::from_raw(4),
    );

    let body = build_switch_body(&mut ctx, i32_ty, targets);
    let ir = lower_and_get_ir_with_ctx(ctx, &body);

    assert!(
        ir.contains("switch"),
        "Expected LLVM switch for 3-case switch, got:\n{ir}"
    );
    assert!(
        ir.contains("i32 5"),
        "Expected case i32 5 in switch, got:\n{ir}"
    );
    assert!(
        ir.contains("i32 10"),
        "Expected case i32 10 in switch, got:\n{ir}"
    );
    assert!(
        ir.contains("i32 15"),
        "Expected case i32 15 in switch, got:\n{ir}"
    );
}

#[test]
fn v29_t13_four_case_switch_uses_llvm_switch() {
    let mut ctx = TyCtxMut::new(Interner::default());
    let i32_ty = make_i32_ty(&mut ctx);

    let targets = SwitchTargets::new(
        Box::new([
            (1u128, BasicBlockIdx::from_raw(1)),
            (2u128, BasicBlockIdx::from_raw(2)),
            (3u128, BasicBlockIdx::from_raw(3)),
            (4u128, BasicBlockIdx::from_raw(4)),
        ]),
        BasicBlockIdx::from_raw(5),
    );

    let body = build_switch_body(&mut ctx, i32_ty, targets);
    let ir = lower_and_get_ir_with_ctx(ctx, &body);

    assert!(
        ir.contains("switch"),
        "Expected LLVM switch for 4-case switch (exceeds small switch threshold), got:\n{ir}"
    );
}

#[test]
fn v29_t14_switch_with_same_target_blocks() {
    let mut ctx = TyCtxMut::new(Interner::default());
    let i32_ty = make_i32_ty(&mut ctx);

    let targets = SwitchTargets::new(
        Box::new([
            (1u128, BasicBlockIdx::from_raw(1)),
            (2u128, BasicBlockIdx::from_raw(1)),
            (3u128, BasicBlockIdx::from_raw(2)),
        ]),
        BasicBlockIdx::from_raw(3),
    );

    let body = build_switch_body(&mut ctx, i32_ty, targets);
    let ir = lower_and_get_ir_with_ctx(ctx, &body);

    assert!(
        ir.contains("icmp eq") || ir.contains("switch"),
        "Expected icmp or switch for same-target switch, got:\n{ir}"
    );
}

#[test]
fn v29_t15_single_case_switch_icmp() {
    let mut ctx = TyCtxMut::new(Interner::default());
    let i32_ty = make_i32_ty(&mut ctx);

    let targets = SwitchTargets::new(
        Box::new([(42u128, BasicBlockIdx::from_raw(1))]),
        BasicBlockIdx::from_raw(2),
    );

    let body = build_switch_body(&mut ctx, i32_ty, targets);
    let ir = lower_and_get_ir_with_ctx(ctx, &body);

    assert!(
        ir.contains("switch i32"),
        "Expected LLVM switch for single-case switch, got:\n{ir}"
    );
    assert!(
        !ir.contains("switch_eq"),
        "Expected no switch_eq label for production switch, got:\n{ir}"
    );
    assert!(
        ir.contains("i32 42"),
        "Expected case value 42 in switch, got:\n{ir}"
    );
}

#[test]
fn v29_t16_two_case_switch_icmp_ladder() {
    let mut ctx = TyCtxMut::new(Interner::default());
    let i32_ty = make_i32_ty(&mut ctx);

    let targets = SwitchTargets::new(
        Box::new([
            (7u128, BasicBlockIdx::from_raw(1)),
            (13u128, BasicBlockIdx::from_raw(2)),
        ]),
        BasicBlockIdx::from_raw(3),
    );

    let body = build_switch_body(&mut ctx, i32_ty, targets);
    let ir = lower_and_get_ir_with_ctx(ctx, &body);

    assert!(
        ir.contains("switch i32"),
        "Expected LLVM switch for 2-case small switch, got:\n{ir}"
    );
    assert!(
        !ir.contains("switch_eq_0"),
        "Expected no switch_eq_0 label, got:\n{ir}"
    );
    assert!(
        !ir.contains("switch_eq_1"),
        "Expected no switch_eq_1 label, got:\n{ir}"
    );
}

#[test]
fn v29_t17_switch_with_zero_case_value() {
    let mut ctx = TyCtxMut::new(Interner::default());
    let i32_ty = make_i32_ty(&mut ctx);

    let targets = SwitchTargets::new(
        Box::new([
            (0u128, BasicBlockIdx::from_raw(1)),
            (1u128, BasicBlockIdx::from_raw(2)),
        ]),
        BasicBlockIdx::from_raw(3),
    );

    let body = build_switch_body(&mut ctx, i32_ty, targets);
    let ir = lower_and_get_ir_with_ctx(ctx, &body);

    assert!(
        ir.contains("switch i32"),
        "Expected LLVM switch for zero-case-value switch, got:\n{ir}"
    );
}

#[test]
fn v29_t18_large_switch_many_cases_all_unique_blocks() {
    let mut ctx = TyCtxMut::new(Interner::default());
    let i32_ty = make_i32_ty(&mut ctx);

    let num_cases = 50u128;
    let mut branches: Vec<(u128, BasicBlockIdx)> = Vec::new();
    for i in 0..num_cases {
        branches.push((i * 3, BasicBlockIdx::from_raw((i + 1) as u32)));
    }
    let otherwise_bb = BasicBlockIdx::from_raw((num_cases + 1) as u32);

    let targets = SwitchTargets::new(branches.into_boxed_slice(), otherwise_bb);

    let body = build_switch_body(&mut ctx, i32_ty, targets);
    let ir = lower_and_get_ir_with_ctx(ctx, &body);

    assert!(
        ir.contains("switch"),
        "Expected LLVM switch for 50-case switch, got:\n{ir}"
    );
}

#[test]
fn v29_t19_bool_switch_both_branches_same_block() {
    let mut ctx = TyCtxMut::new(Interner::default());
    let bool_ty = make_bool_ty(&mut ctx);

    let targets = SwitchTargets::new(
        Box::new([
            (0u128, BasicBlockIdx::from_raw(1)),
            (1u128, BasicBlockIdx::from_raw(1)),
        ]),
        BasicBlockIdx::from_raw(2),
    );

    let body = build_switch_body(&mut ctx, bool_ty, targets);
    let ir = lower_and_get_ir_with_ctx(ctx, &body);

    assert!(
        ir.contains("switch i1"),
        "Expected LLVM switch for bool switch with same target, got:\n{ir}"
    );
}

#[test]
fn v29_t20_switch_with_max_u32_value() {
    let mut ctx = TyCtxMut::new(Interner::default());
    let i32_ty = make_i32_ty(&mut ctx);

    let targets = SwitchTargets::new(
        Box::new([
            (0u128, BasicBlockIdx::from_raw(1)),
            (1u128, BasicBlockIdx::from_raw(2)),
            (2u128, BasicBlockIdx::from_raw(3)),
            (u128::from(u32::MAX), BasicBlockIdx::from_raw(4)),
            (2147483647u128, BasicBlockIdx::from_raw(5)),
        ]),
        BasicBlockIdx::from_raw(6),
    );

    let body = build_switch_body(&mut ctx, i32_ty, targets);
    let ir = lower_and_get_ir_with_ctx(ctx, &body);

    assert!(
        ir.contains("switch"),
        "Expected LLVM switch for large-value i32 switch (5 cases), got:\n{ir}"
    );
}
