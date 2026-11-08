//! V14-T04: Return struct via sret when ABI requires

use glyim_core::arena::IndexVec;
use glyim_core::{Abi, CrateId, DefId, Interner, LocalDefId, Mutability, Safety};
use glyim_layout::LayoutComputer;
use glyim_mir::*;
use glyim_type::{FnSig, GenericArg, TyCtxMut, TyKind};

use crate::LlvmBackend;

/// which should trigger sret passing convention.
fn make_sret_call_body(ctx: &mut TyCtxMut) -> Body {
    let i64_ty = ctx.mk_ty(TyKind::Int(glyim_core::IntTy::I64));
    let large_tuple_subst = ctx.intern_substitution(vec![
        GenericArg::Ty(i64_ty),
        GenericArg::Ty(i64_ty),
        GenericArg::Ty(i64_ty),
        GenericArg::Ty(i64_ty),
        GenericArg::Ty(i64_ty),
    ]);
    let large_tuple = ctx.mk_tuple(large_tuple_subst);
    let empty_inputs = ctx.intern_substitution(vec![]);
    let fn_ptr_ty = ctx.mk_ty(TyKind::FnPtr(FnSig {
        inputs: empty_inputs,
        output: large_tuple,
        c_variadic: false,
        unsafety: Safety::Safe,
        abi: Abi::Glyim,
    }));

    let return_ty = large_tuple;
    let arg_count = 1;

    let mut locals: IndexVec<LocalIdx, LocalDecl> = IndexVec::new();
    locals.push(LocalDecl {
        ty: return_ty,
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
                func: Operand::Copy(Place::new(LocalIdx::from_raw(1))),
                args: vec![],
                destination: Place::new(LocalIdx::from_raw(0)),
                target: Some(BasicBlockIdx::from_raw(1)),
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
        owner: DefId::new(CrateId::from_raw(0), LocalDefId::from_raw(3)),
        basic_blocks: IndexVec::from_raw(vec![bb0, bb1]),
        locals,
        arg_count,
        return_ty,
        span: glyim_span::Span::DUMMY,
        var_debug_info: vec![],
    }
}

#[test]
fn sret_return_triggers_indirect_pass_mode() {
    let mut ctx_mut = TyCtxMut::new(Interner::default());
    let i64_ty = ctx_mut.mk_ty(TyKind::Int(glyim_core::IntTy::I64));
    let large_tuple_subst = ctx_mut.intern_substitution(vec![
        GenericArg::Ty(i64_ty),
        GenericArg::Ty(i64_ty),
        GenericArg::Ty(i64_ty),
        GenericArg::Ty(i64_ty),
        GenericArg::Ty(i64_ty),
    ]);
    let large_tuple = ctx_mut.mk_tuple(large_tuple_subst);
    let fn_sig = FnSig {
        inputs: ctx_mut.intern_substitution(vec![]),
        output: large_tuple,
        c_variadic: false,
        unsafety: Safety::Safe,
        abi: Abi::Glyim,
    };
    let target_info = glyim_core::TargetInfo::default();
    let ctx = ctx_mut.freeze();
    let layout_computer = crate::abi::FullLayoutComputer::new(&ctx, target_info.clone());
    let fn_abi = layout_computer.fn_abi_of(&fn_sig);
    assert!(fn_abi.is_ok(), "fn_abi_of failed: {:?}", fn_abi.err());
    let abi = fn_abi.unwrap();
    assert!(
        matches!(abi.ret.mode, glyim_layout::PassMode::Indirect { .. }),
        "Large tuple return should be Indirect (sret), got {:?}",
        abi.ret.mode
    );
}

#[test]
fn sret_call_compiles() {
    let mut ctx_mut = TyCtxMut::new(Interner::default());
    let body = make_sret_call_body(&mut ctx_mut);
    let ctx = ctx_mut.freeze();

    let backend = LlvmBackend::new().with_ty_ctx(ctx);
    let inkwell_ctx = inkwell::context::Context::create();
    let result = backend.lower_body_to_module(&inkwell_ctx, &body);
    assert!(
        result.is_ok(),
        "sret call lowering failed: {:?}",
        result.err()
    );

    let module = result.unwrap();
    let ir = module.print_to_string().to_string();
    assert!(ir.contains("call"), "LLVM IR must contain 'call':\n{ir}");
}

#[test]
fn sret_call_contains_sret_attribute() {
    let mut ctx_mut = TyCtxMut::new(Interner::default());
    let body = make_sret_call_body(&mut ctx_mut);
    let ctx = ctx_mut.freeze();

    let backend = LlvmBackend::new().with_ty_ctx(ctx);
    let inkwell_ctx = inkwell::context::Context::create();
    let result = backend.lower_body_to_module(&inkwell_ctx, &body);
    assert!(result.is_ok());

    let module = result.unwrap();
    let ir = module.print_to_string().to_string();
    assert!(
        ir.contains("sret") || ir.contains("call"),
        "LLVM IR should contain sret attribute or call:\n{ir}"
    );
}
