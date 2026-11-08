use crate::LlvmBackend;
use glyim_core::{FloatTy, IntTy, Mutability, TargetInfo};
use glyim_layout::{LayoutComputer, PassMode};
use glyim_mir::*;
use glyim_test::with_fresh_ty_ctx;
use glyim_type::{TyCtxMut, TyKind};

fn make_fn_sig(
    ctx_mut: &mut TyCtxMut,
    inputs: Vec<glyim_type::Ty>,
    output: glyim_type::Ty,
    variadic: bool,
) -> glyim_type::FnSig {
    let input_subst = ctx_mut.intern_substitution(
        inputs
            .iter()
            .map(|&ty| glyim_type::GenericArg::Ty(ty))
            .collect(),
    );
    glyim_type::FnSig {
        inputs: input_subst,
        output,
        c_variadic: variadic,
        unsafety: glyim_core::Safety::Safe,
        abi: glyim_core::Abi::Glyim,
    }
}

fn dummy_def_id(idx: u32) -> glyim_core::DefId {
    glyim_core::DefId::new(
        glyim_core::CrateId::from_raw(0),
        glyim_core::LocalDefId::from_raw(idx),
    )
}

#[test]
fn v27_t01_sret_large_struct_return() {
    let (ctx, (sig, _fn_ptr_ty, body)) = with_fresh_ty_ctx(|ctx_mut| {
        let i64_ty = ctx_mut.mk_ty(TyKind::Int(IntTy::I64));
        let subst = ctx_mut.intern_substitution(vec![
            glyim_type::GenericArg::Ty(i64_ty),
            glyim_type::GenericArg::Ty(i64_ty),
            glyim_type::GenericArg::Ty(i64_ty),
            glyim_type::GenericArg::Ty(i64_ty),
        ]);
        let ret_ty = ctx_mut.mk_ty(TyKind::Tuple(subst));
        let sig = make_fn_sig(ctx_mut, vec![], ret_ty, false);
        let fn_ptr_ty = ctx_mut.mk_ty(TyKind::FnPtr(sig.clone()));

        let owner = dummy_def_id(0);
        let mut body = Body::dummy(owner);
        // local 0 = return place (already), local 1 = fn_ptr
        body.locals.push(LocalDecl {
            ty: fn_ptr_ty,
            mutability: Mutability::Not,
            source_info: SourceInfo::new(glyim_span::Span::DUMMY),
        });

        let mut bbs = glyim_core::arena::IndexVec::new();
        bbs.push(BasicBlockData::new(Terminator {
            kind: TerminatorKind::Call {
                func: Operand::Move(Place::new(LocalIdx::from_raw(1))),
                args: vec![],
                destination: Place::new(LocalIdx::from_raw(0)),
                target: Some(BasicBlockIdx::from_raw(1)),
                cleanup: None,
            },
            source_info: SourceInfo::new(glyim_span::Span::DUMMY),
        }));
        bbs.push(BasicBlockData::new(Terminator {
            kind: TerminatorKind::Return,
            source_info: SourceInfo::new(glyim_span::Span::DUMMY),
        }));
        body.basic_blocks = bbs;

        (sig, fn_ptr_ty, body)
    });

    let target_info = TargetInfo::default();
    let layout_computer = crate::abi::FullLayoutComputer::new(&ctx, target_info);
    let fn_abi = layout_computer.fn_abi_of(&sig).expect("fn_abi_of");
    assert!(matches!(fn_abi.ret.mode, PassMode::Indirect { .. }));

    let backend = LlvmBackend::new().with_ty_ctx(ctx);
    let context = inkwell::context::Context::create();
    let module = backend
        .lower_body_to_module(&context, &body)
        .expect("lowering");
    let ir = module.print_to_string().to_string();
    assert!(
        ir.contains("sret"),
        "IR should contain sret attribute, got:\n{ir}"
    );
    assert!(ir.contains("call"), "IR should contain a call instruction");
}

#[test]
fn v27_t02_argument_byval_with_abi_alignment() {
    let (ctx, (_sig, _fn_ptr_ty, _arg_ty, body)) = with_fresh_ty_ctx(|ctx_mut| {
        let i64_ty = ctx_mut.mk_ty(TyKind::Int(IntTy::I64));
        let i32_ty = ctx_mut.mk_ty(TyKind::Int(IntTy::I32));
        let subst = ctx_mut.intern_substitution(vec![
            glyim_type::GenericArg::Ty(i64_ty),
            glyim_type::GenericArg::Ty(i32_ty),
        ]);
        let struct_ty = ctx_mut.mk_ty(TyKind::Tuple(subst));
        let unit_ty = ctx_mut.unit_ty();
        let sig = make_fn_sig(ctx_mut, vec![struct_ty], unit_ty, false);
        let fn_ptr_ty = ctx_mut.mk_ty(TyKind::FnPtr(sig.clone()));

        let owner = dummy_def_id(1);
        let mut body = Body::dummy(owner);
        // local 0 = return place, local 1 = fn_ptr, local 2 = arg
        body.locals.push(LocalDecl {
            ty: fn_ptr_ty,
            mutability: Mutability::Not,
            source_info: SourceInfo::new(glyim_span::Span::DUMMY),
        });
        body.locals.push(LocalDecl {
            ty: struct_ty,
            mutability: Mutability::Not,
            source_info: SourceInfo::new(glyim_span::Span::DUMMY),
        });

        let mut bbs = glyim_core::arena::IndexVec::new();
        bbs.push(BasicBlockData::new(Terminator {
            kind: TerminatorKind::Call {
                func: Operand::Move(Place::new(LocalIdx::from_raw(1))),
                args: vec![Operand::Move(Place::new(LocalIdx::from_raw(2)))],
                destination: Place::new(LocalIdx::from_raw(0)),
                target: Some(BasicBlockIdx::from_raw(1)),
                cleanup: None,
            },
            source_info: SourceInfo::new(glyim_span::Span::DUMMY),
        }));
        bbs.push(BasicBlockData::new(Terminator {
            kind: TerminatorKind::Return,
            source_info: SourceInfo::new(glyim_span::Span::DUMMY),
        }));
        body.basic_blocks = bbs;

        (sig, fn_ptr_ty, struct_ty, body)
    });

    let backend = LlvmBackend::new().with_ty_ctx(ctx);
    let context = inkwell::context::Context::create();
    let module = backend
        .lower_body_to_module(&context, &body)
        .expect("lowering");
    let ir = module.print_to_string().to_string();
    assert!(
        ir.contains("call"),
        "IR should contain a call instruction, got:\n{ir}"
    );
}

#[test]
fn v27_t03_variadic_function_call() {
    let (ctx, (_sig, _fn_ptr_ty, body)) = with_fresh_ty_ctx(|ctx_mut| {
        let unit_ty = ctx_mut.unit_ty();
        let sig = glyim_type::FnSig {
            inputs: ctx_mut.intern_substitution(vec![]),
            output: unit_ty,
            c_variadic: true,
            unsafety: glyim_core::Safety::Safe,
            abi: glyim_core::Abi::C,
        };
        let fn_ptr_ty = ctx_mut.mk_ty(TyKind::FnPtr(sig.clone()));

        let owner = dummy_def_id(2);
        let mut body = Body::dummy(owner);
        // local 0 = return place, local 1 = fn_ptr
        body.locals.push(LocalDecl {
            ty: fn_ptr_ty,
            mutability: Mutability::Not,
            source_info: SourceInfo::new(glyim_span::Span::DUMMY),
        });

        let mut bbs = glyim_core::arena::IndexVec::new();
        bbs.push(BasicBlockData::new(Terminator {
            kind: TerminatorKind::Call {
                func: Operand::Move(Place::new(LocalIdx::from_raw(1))),
                args: vec![],
                destination: Place::new(LocalIdx::from_raw(0)),
                target: Some(BasicBlockIdx::from_raw(1)),
                cleanup: None,
            },
            source_info: SourceInfo::new(glyim_span::Span::DUMMY),
        }));
        bbs.push(BasicBlockData::new(Terminator {
            kind: TerminatorKind::Return,
            source_info: SourceInfo::new(glyim_span::Span::DUMMY),
        }));
        body.basic_blocks = bbs;

        (sig, fn_ptr_ty, body)
    });

    let backend = LlvmBackend::new().with_ty_ctx(ctx);
    let context = inkwell::context::Context::create();
    let module = backend
        .lower_body_to_module(&context, &body)
        .expect("lowering");
    let ir = module.print_to_string().to_string();
    assert!(
        ir.contains("..."),
        "Variadic function should have '...' in declaration, got:\n{ir}"
    );
}

#[test]
fn v27_t04_arm_hva_struct_four_floats_simd() {
    let (ctx, (_sig, _fn_ptr_ty, body)) = with_fresh_ty_ctx(|ctx_mut| {
        let f32_ty = ctx_mut.mk_ty(TyKind::Float(FloatTy::F32));
        let subst = ctx_mut.intern_substitution(vec![
            glyim_type::GenericArg::Ty(f32_ty),
            glyim_type::GenericArg::Ty(f32_ty),
            glyim_type::GenericArg::Ty(f32_ty),
            glyim_type::GenericArg::Ty(f32_ty),
        ]);
        let struct_ty = ctx_mut.mk_ty(TyKind::Tuple(subst));
        let unit_ty = ctx_mut.unit_ty();
        let sig = make_fn_sig(ctx_mut, vec![struct_ty], unit_ty, false);
        let fn_ptr_ty = ctx_mut.mk_ty(TyKind::FnPtr(sig.clone()));

        let owner = dummy_def_id(3);
        let mut body = Body::dummy(owner);
        // local 0 = return place, local 1 = fn_ptr, local 2 = arg
        body.locals.push(LocalDecl {
            ty: fn_ptr_ty,
            mutability: Mutability::Not,
            source_info: SourceInfo::new(glyim_span::Span::DUMMY),
        });
        body.locals.push(LocalDecl {
            ty: struct_ty,
            mutability: Mutability::Not,
            source_info: SourceInfo::new(glyim_span::Span::DUMMY),
        });

        let mut bbs = glyim_core::arena::IndexVec::new();
        bbs.push(BasicBlockData::new(Terminator {
            kind: TerminatorKind::Call {
                func: Operand::Move(Place::new(LocalIdx::from_raw(1))),
                args: vec![Operand::Move(Place::new(LocalIdx::from_raw(2)))],
                destination: Place::new(LocalIdx::from_raw(0)),
                target: Some(BasicBlockIdx::from_raw(1)),
                cleanup: None,
            },
            source_info: SourceInfo::new(glyim_span::Span::DUMMY),
        }));
        bbs.push(BasicBlockData::new(Terminator {
            kind: TerminatorKind::Return,
            source_info: SourceInfo::new(glyim_span::Span::DUMMY),
        }));
        body.basic_blocks = bbs;

        (sig, fn_ptr_ty, body)
    });

    let backend = LlvmBackend::new().with_ty_ctx(ctx);
    let context = inkwell::context::Context::create();
    let module = backend
        .lower_body_to_module(&context, &body)
        .expect("lowering");
    let ir = module.print_to_string().to_string();
    assert!(
        ir.contains("call"),
        "IR should contain a call instruction, got:\n{ir}"
    );
}
