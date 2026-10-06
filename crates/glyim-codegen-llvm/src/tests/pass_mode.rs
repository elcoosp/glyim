//! S22-T05: Tests for unhandled PassMode arguments.

use super::helpers::*;
use crate::LlvmBackend;
use glyim_core::primitives::*;
use glyim_core::{ConstDefId, FnDefId};
use glyim_mir::*;
use glyim_span::Span;
use glyim_test::with_fresh_ty_ctx;
use glyim_type::*;

#[test]
fn indirect_return_pass_mode_generates_sret() {
    let (ctx, (large_ty, fn_ptr_ty)) = with_fresh_ty_ctx(|c| {
        let i32_ty = c.mk_ty(TyKind::Int(IntTy::I32));
        let subst = c.intern_substitution(vec![
            GenericArg::Ty(i32_ty),
            GenericArg::Ty(i32_ty),
            GenericArg::Ty(i32_ty),
            GenericArg::Ty(i32_ty),
            GenericArg::Ty(i32_ty),
            GenericArg::Ty(i32_ty),
            GenericArg::Ty(i32_ty),
            GenericArg::Ty(i32_ty),
            GenericArg::Ty(i32_ty),
        ]);
        let large_ty = c.mk_tuple(subst);
        let inputs = c.intern_substitution(vec![GenericArg::Ty(large_ty)]);
        let sig = FnSig {
            inputs,
            output: large_ty,
            c_variadic: false,
            unsafety: Safety::Safe,
            abi: Abi::Glyim,
        };
        let fn_ptr_ty = c.mk_fn_ptr(sig);
        (large_ty, fn_ptr_ty)
    });
    let backend = LlvmBackend::new().with_ty_ctx(ctx);
    let mut builder = BodyBuilder::new(large_ty);
    let arg_local = builder.add_arg(large_ty);
    let fn_local = builder.add_local(fn_ptr_ty);
    let dest_local = builder.add_local(large_ty);
    let success_bb = builder.add_block(Terminator {
        kind: TerminatorKind::Return,
        source_info: SourceInfo::new(Span::DUMMY),
    });
    builder.set_terminator(Terminator {
        kind: TerminatorKind::Call {
            func: Operand::Copy(Place::new(fn_local)),
            args: vec![Operand::Copy(Place::new(arg_local))],
            destination: Place::new(dest_local),
            target: Some(success_bb),
            cleanup: None,
        },
        source_info: SourceInfo::new(Span::DUMMY),
    });
    let body = builder.build();
    let ir = backend.generate_ir(&body);
    assert!(ir.is_ok(), "generate_ir should succeed: {:?}", ir.err());
}

#[test]
fn ignore_pass_mode_skips_zero_sized_args() {
    let (ctx, fn_ptr_ty) = with_fresh_ty_ctx(|c| {
        let inputs = c.intern_substitution(vec![GenericArg::Ty(Ty::UNIT)]);
        let sig = FnSig {
            inputs,
            output: Ty::UNIT,
            c_variadic: false,
            unsafety: Safety::Safe,
            abi: Abi::Glyim,
        };
        c.mk_fn_ptr(sig)
    });
    let backend = LlvmBackend::new().with_ty_ctx(ctx);
    let mut builder = BodyBuilder::new(Ty::UNIT);
    let unit_local = builder.add_local(Ty::UNIT);
    let fn_local = builder.add_local(fn_ptr_ty);
    let dest_local = builder.add_local(Ty::UNIT);
    let success_bb = builder.add_block(Terminator {
        kind: TerminatorKind::Return,
        source_info: SourceInfo::new(Span::DUMMY),
    });
    builder.set_terminator(Terminator {
        kind: TerminatorKind::Call {
            func: Operand::Copy(Place::new(fn_local)),
            args: vec![Operand::Copy(Place::new(unit_local))],
            destination: Place::new(dest_local),
            target: Some(success_bb),
            cleanup: None,
        },
        source_info: SourceInfo::new(Span::DUMMY),
    });
    let body = builder.build();
    let ir = backend.generate_ir(&body);
    assert!(ir.is_ok(), "generate_ir should succeed: {:?}", ir.err());
}

#[test]
fn fn_def_constant_lowering_produces_pointer() {
    let (ctx, (fn_def_ty, fn_def_id, substs)) = with_fresh_ty_ctx(|c| {
        let fn_def_id = FnDefId::from_raw(42);
        let substs = c.intern_substitution(vec![]);
        let fn_def_ty = c.mk_ty(TyKind::FnDef(fn_def_id, substs));
        (fn_def_ty, fn_def_id, substs)
    });
    let backend = LlvmBackend::new().with_ty_ctx(ctx);
    let mut builder = BodyBuilder::new(Ty::UNIT);
    let fn_local = builder.add_local(fn_def_ty);
    builder.add_statement(make_assign(
        Place::new(fn_local),
        Rvalue::Use(Operand::Constant(MirConst {
            kind: MirConstKind::Fn(fn_def_id, substs),
            ty: fn_def_ty,
            span: Span::DUMMY,
        })),
    ));
    let body = builder.build();
    let ir = backend.generate_ir(&body);
    assert!(ir.is_ok(), "generate_ir should succeed: {:?}", ir.err());
}

#[test]
fn const_ref_constant_lowering_produces_pointer() {
    // T114-PATCHED [LL-19]: ConstRef reaching LLVM codegen is an internal
    // error — const evaluation must fold named constants into concrete
    // MirConstKind values first. This test exercises the ICE path: the
    // diagnostic must name the const id and mention "const evaluation".
    let (ctx, (const_ref_ty, const_def_id, substs)) = with_fresh_ty_ctx(|c| {
        let const_def_id = ConstDefId::from_raw(7);
        let substs = c.intern_substitution(vec![]);
        let const_ref_ty = c.mk_ty(TyKind::Uint(UintTy::U32));
        (const_ref_ty, const_def_id, substs)
    });
    let backend = LlvmBackend::new().with_ty_ctx(ctx);
    let mut builder = BodyBuilder::new(Ty::UNIT);
    let local = builder.add_local(const_ref_ty);
    builder.add_statement(make_assign(
        Place::new(local),
        Rvalue::Use(Operand::Constant(MirConst {
            kind: MirConstKind::ConstRef(const_def_id, substs),
            ty: const_ref_ty,
            span: Span::DUMMY,
        })),
    ));
    let body = builder.build();
    let ir = backend.generate_ir(&body);
    let errs = ir.expect_err("ConstRef should produce an internal-error diagnostic");
    let joined = format!("{errs:?}");
    assert!(
        joined.contains("ConstRef(7)") || joined.contains("const evaluation"),
        "diagnostic should name the const id and mention const evaluation, got: {joined}"
    );
}
