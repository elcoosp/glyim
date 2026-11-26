//! Pass a large struct argument that should be passed indirectly

use glyim_core::arena::IndexVec;
use glyim_core::{Abi, CrateId, DefId, IntTy, Interner, LocalDefId, Mutability, Safety};
use glyim_layout::LayoutComputer;
use glyim_mir::*;
use glyim_type::{FnSig, GenericArg, TyCtxMut, TyKind};

use crate::LlvmBackend;

fn make_large_struct_arg_body(ctx: &mut TyCtxMut) -> Body {
    let i32_ty = ctx.mk_ty(TyKind::Int(IntTy::I32));
    let i64_ty = ctx.mk_ty(TyKind::Int(IntTy::I64));
    let large_tuple_subst = ctx.intern_substitution(vec![
        GenericArg::Ty(i64_ty),
        GenericArg::Ty(i64_ty),
        GenericArg::Ty(i64_ty),
        GenericArg::Ty(i64_ty),
        GenericArg::Ty(i64_ty),
    ]);
    let large_tuple = ctx.mk_tuple(large_tuple_subst);
    let fn_inputs = ctx.intern_substitution(vec![GenericArg::Ty(large_tuple)]);
    let fn_ptr_ty = ctx.mk_ty(TyKind::FnPtr(FnSig {
        inputs: fn_inputs,
        output: i32_ty,
        c_variadic: false,
        unsafety: Safety::Safe,
        abi: Abi::Glyim,
    }));

    let return_ty = i32_ty;
    let arg_count = 2;

    let mut locals: IndexVec<LocalIdx, LocalDecl> = IndexVec::new();
    locals.push(LocalDecl {
        ty: return_ty,
        mutability: Mutability::Not,
        source_info: SourceInfo::new(glyim_span::Span::DUMMY),
    });
    locals.push(LocalDecl {
        ty: large_tuple,
        mutability: Mutability::Not,
        source_info: SourceInfo::new(glyim_span::Span::DUMMY),
    });
    locals.push(LocalDecl {
        ty: fn_ptr_ty,
        mutability: Mutability::Not,
        source_info: SourceInfo::new(glyim_span::Span::DUMMY),
    });

    let bb0 = BasicBlockData {
        statements: vec![],
        terminator: Terminator {
            kind: TerminatorKind::Call {
                func: Operand::Copy(Place::new(LocalIdx::from_usize(2))),
                args: vec![Operand::Move(Place::new(LocalIdx::from_usize(1)))],
                destination: Place::new(LocalIdx::from_usize(0)),
                target: Some(BasicBlockIdx::from_usize(1)),
                cleanup: None,
            },
            source_info: SourceInfo::new(glyim_span::Span::DUMMY),
        },
        is_cleanup: false,
    };

    let bb1 = BasicBlockData {
        statements: vec![],
        terminator: Terminator {
            kind: TerminatorKind::Return,
            source_info: SourceInfo::new(glyim_span::Span::DUMMY),
        },
        is_cleanup: false,
    };

    Body {
        owner: DefId::new(CrateId::from_usize(0), LocalDefId::from_usize(13)),
        basic_blocks: IndexVec::from_vec(vec![bb0, bb1]),
        locals,
        arg_count,
        return_ty,
        span: glyim_span::Span::DUMMY,
        var_debug_info: vec![],
    }
}

#[test]
fn call_with_large_struct_arg_compiles() {
    let mut ctx_mut = TyCtxMut::new(Interner::default());
    let body = make_large_struct_arg_body(&mut ctx_mut);
    let ctx = ctx_mut.freeze();

    let backend = LlvmBackend::new().with_ty_ctx(ctx);
    let inkwell_ctx = inkwell::context::Context::create();
    let result = backend.lower_body_to_module(&inkwell_ctx, &body);
    assert!(
        result.is_ok(),
        "large struct arg lowering failed: {:?}",
        result.err()
    );
}

#[test]
fn large_struct_arg_classified_indirect() {
    let mut ctx_mut = TyCtxMut::new(Interner::default());
    let i64_ty = ctx_mut.mk_ty(TyKind::Int(IntTy::I64));
    let large_tuple_subst = ctx_mut.intern_substitution(vec![
        GenericArg::Ty(i64_ty),
        GenericArg::Ty(i64_ty),
        GenericArg::Ty(i64_ty),
        GenericArg::Ty(i64_ty),
        GenericArg::Ty(i64_ty),
    ]);
    let large_tuple = ctx_mut.mk_tuple(large_tuple_subst);
    let fn_sig = FnSig {
        inputs: ctx_mut.intern_substitution(vec![GenericArg::Ty(large_tuple)]),
        output: ctx_mut.mk_ty(TyKind::Int(IntTy::I32)),
        c_variadic: false,
        unsafety: Safety::Safe,
        abi: Abi::Glyim,
    };
    let target_info = glyim_core::TargetInfo::default();
    let ctx = ctx_mut.freeze();
    let layout_computer = crate::abi::FullLayoutComputer::new(&ctx, target_info);
    let fn_abi = layout_computer
        .fn_abi_of(&fn_sig)
        .expect("fn_abi_of should succeed");
    assert!(
        matches!(fn_abi.args[0].mode, glyim_layout::PassMode::Indirect { .. }),
        "Large struct argument should be Indirect, got {:?}",
        fn_abi.args[0].mode
    );
}
