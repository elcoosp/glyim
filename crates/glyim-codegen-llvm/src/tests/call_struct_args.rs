//! V14-T03: Pass struct arguments by value according to ABI

use glyim_core::arena::IndexVec;
use glyim_core::{Abi, CrateId, DefId, Interner, LocalDefId, Mutability, Safety};
use glyim_mir::*;
use glyim_type::{FnSig, GenericArg, TyCtxMut, TyKind};

use crate::LlvmBackend;

fn make_struct_call_body(ctx: &mut TyCtxMut) -> Body {
    let i32_ty = ctx.mk_ty(TyKind::Int(glyim_core::IntTy::I32));
    let tuple_subst = ctx.intern_substitution(vec![GenericArg::Ty(i32_ty), GenericArg::Ty(i32_ty)]);
    let tuple_ty = ctx.mk_tuple(tuple_subst);

    let fn_sig = FnSig {
        inputs: ctx.intern_substitution(vec![GenericArg::Ty(tuple_ty)]),
        output: i32_ty,
        c_variadic: false,
        unsafety: Safety::Safe,
        abi: Abi::Glyim,
    };
    let fn_ptr_ty = ctx.mk_ty(TyKind::FnPtr(fn_sig.clone()));

    let return_ty = i32_ty;
    let arg_count = 2;

    let mut locals: IndexVec<LocalIdx, LocalDecl> = IndexVec::new();
    locals.push(LocalDecl {
        ty: return_ty,
        mutability: Mutability::Not,
        source_info: SourceInfo::new(glyim_span::Span::DUMMY),
    });
    locals.push(LocalDecl {
        ty: tuple_ty,
        mutability: Mutability::Not,
        source_info: SourceInfo::new(glyim_span::Span::DUMMY),
    });
    locals.push(LocalDecl {
        ty: fn_ptr_ty,
        mutability: Mutability::Not,
        source_info: SourceInfo::new(glyim_span::Span::DUMMY),
    });
    locals.push(LocalDecl {
        ty: i32_ty,
        mutability: Mutability::Not,
        source_info: SourceInfo::new(glyim_span::Span::DUMMY),
    });
    locals.push(LocalDecl {
        ty: i32_ty,
        mutability: Mutability::Not,
        source_info: SourceInfo::new(glyim_span::Span::DUMMY),
    });

    let const_1 = MirConst {
        kind: MirConstKind::Uint(1),
        ty: i32_ty,
        span: glyim_span::Span::DUMMY,
    };
    let const_2 = MirConst {
        kind: MirConstKind::Uint(2),
        ty: i32_ty,
        span: glyim_span::Span::DUMMY,
    };

    let bb0 = BasicBlockData {
        statements: vec![
            Statement {
                kind: StatementKind::Assign(
                    Place::new(LocalIdx::from_raw(3)),
                    Rvalue::Use(Operand::Constant(const_1)),
                ),
                source_info: SourceInfo::new(glyim_span::Span::DUMMY),
            },
            Statement {
                kind: StatementKind::Assign(
                    Place::new(LocalIdx::from_raw(4)),
                    Rvalue::Use(Operand::Constant(const_2)),
                ),
                source_info: SourceInfo::new(glyim_span::Span::DUMMY),
            },
            Statement {
                kind: StatementKind::Assign(
                    Place::new(LocalIdx::from_raw(1)),
                    Rvalue::Aggregate(
                        AggregateKind::Tuple,
                        vec![
                            Operand::Copy(Place::new(LocalIdx::from_raw(3))),
                            Operand::Copy(Place::new(LocalIdx::from_raw(4))),
                        ],
                    ),
                ),
                source_info: SourceInfo::new(glyim_span::Span::DUMMY),
            },
        ],
        terminator: Terminator {
            kind: TerminatorKind::Call {
                func: Operand::Copy(Place::new(LocalIdx::from_raw(2))),
                args: vec![Operand::Move(Place::new(LocalIdx::from_raw(1)))],
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
        owner: DefId::new(CrateId::from_raw(0), LocalDefId::from_raw(2)),
        basic_blocks: IndexVec::from_raw(vec![bb0, bb1]),
        locals,
        arg_count,
        return_ty,
        span: glyim_span::Span::DUMMY,
        var_debug_info: vec![],
    }
}

#[test]
fn call_with_struct_args_compiles() {
    let mut ctx_mut = TyCtxMut::new(Interner::default());
    let body = make_struct_call_body(&mut ctx_mut);
    let ctx = ctx_mut.freeze();

    let backend = LlvmBackend::new().with_ty_ctx(ctx);
    let inkwell_ctx = inkwell::context::Context::create();
    let result = backend.lower_body_to_module(&inkwell_ctx, &body);
    assert!(
        result.is_ok(),
        "lower_body_to_module with struct args failed: {:?}",
        result.err()
    );

    let module = result.unwrap();
    let ir = module.print_to_string().to_string();
    assert!(ir.contains("call"), "LLVM IR must contain 'call':\n{ir}");
}
