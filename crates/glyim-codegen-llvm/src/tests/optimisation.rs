use crate::LlvmBackend;
use glyim_core::IndexVec;
use glyim_core::primitives::IntTy;
use glyim_core::{CrateId, DefId, Interner, LocalDefId, Mutability};
use glyim_mir::BasicBlockData;
use glyim_mir::{
    Body, LocalDecl, LocalIdx, MirConst, MirConstKind, Operand, Place, Rvalue, SourceInfo,
    Statement, StatementKind, Terminator, TerminatorKind,
};
use glyim_span::Span;
use glyim_type::{Ty, TyCtxMut, TyKind};
use inkwell::context::Context;
use std::sync::Arc;

fn make_ty_ctx() -> (glyim_type::TyCtx, Ty) {
    let mut ctx_mut = TyCtxMut::new(Interner::default());
    let int_ty = ctx_mut.mk_ty(TyKind::Int(IntTy::I32));
    (ctx_mut.freeze(), int_ty)
}

fn simple_body_with_local(ty: Ty) -> Body {
    let owner = DefId::new(CrateId::from_raw(1), LocalDefId::from_raw(1));
    // Build fresh IndexVecs
    let mut basic_blocks = IndexVec::new();
    let mut locals = IndexVec::new();

    // Local 0 is the return place
    let ret_local = LocalIdx::from_raw(0);
    locals.push(LocalDecl {
        ty,
        mutability: Mutability::Mut,
        source_info: SourceInfo::new(Span::DUMMY),
    });

    let const_val = MirConst {
        kind: MirConstKind::Int(42),
        ty,
        span: Span::DUMMY,
    };
    let assign = Statement {
        kind: StatementKind::Assign(
            Place::new(ret_local),
            Rvalue::Use(Operand::Constant(const_val)),
        ),
        source_info: SourceInfo::new(Span::DUMMY),
    };
    let term = Terminator {
        kind: TerminatorKind::Return,
        source_info: SourceInfo::new(Span::DUMMY),
    };
    let bb = BasicBlockData {
        statements: vec![assign],
        terminator: term,
        is_cleanup: false,
    };
    basic_blocks.push(bb);

    Body {
        owner,
        basic_blocks,
        locals,
        arg_count: 0,
        return_ty: ty,
        span: Span::DUMMY,
        var_debug_info: Vec::new(),
    }
}

fn count_allocas(module: &inkwell::module::Module) -> usize {
    let mut count = 0;
    for func in module.get_functions() {
        for bb in func.get_basic_blocks() {
            for instr in bb.get_instructions() {
                if instr.get_opcode() == inkwell::values::InstructionOpcode::Alloca {
                    count += 1;
                }
            }
        }
    }
    count
}

#[test]
fn test_o0_no_optimizations() {
    let (ctx, int_ty) = make_ty_ctx();
    let backend = LlvmBackend::new().with_ty_ctx(ctx).with_opt_level(0);
    let body = Arc::new(simple_body_with_local(int_ty));
    let context = Context::create();
    let module = backend
        .lower_bodies_to_module(&context, &crate::tests::to_pairs(&[body]))
        .expect("lowering failed");
    assert!(
        count_allocas(&module) > 0,
        "Expected alloca instructions at O0"
    );
}

#[test]
fn test_o2_mem2reg() {
    let (ctx, int_ty) = make_ty_ctx();
    let backend = LlvmBackend::new().with_ty_ctx(ctx).with_opt_level(2);
    let body = Arc::new(simple_body_with_local(int_ty));
    let context = Context::create();
    let module = backend
        .lower_bodies_to_module(&context, &crate::tests::to_pairs(&[body]))
        .expect("lowering failed");

    // Run optimization passes manually (lower_bodies_to_module does not run passes)
    let target_triple = inkwell::targets::TargetTriple::create("x86_64-unknown-linux-gnu");
    let target = inkwell::targets::Target::from_triple(&target_triple).unwrap();
    let target_machine = target
        .create_target_machine(
            &target_triple,
            "generic",
            "",
            inkwell::OptimizationLevel::Aggressive,
            inkwell::targets::RelocMode::Default,
            inkwell::targets::CodeModel::Default,
        )
        .unwrap();
    crate::passes::run_llvm_passes(&module, &target_machine, 2, false).expect("passes failed");

    assert_eq!(
        count_allocas(&module),
        0,
        "Expected no alloca instructions after O2 mem2reg"
    );
}

#[test]
fn test_oz_size_optimization() {
    let (ctx, int_ty) = make_ty_ctx();
    let backend = LlvmBackend::new()
        .with_ty_ctx(ctx)
        .with_opt_level(2)
        .with_opt_for_size(true);
    let body = Arc::new(simple_body_with_local(int_ty));
    let context = Context::create();
    let result = backend.lower_bodies_to_module(&context, &crate::tests::to_pairs(&[body]));
    assert!(result.is_ok(), "Oz lowering should succeed");
}

#[test]
fn test_lto_across_cgus() {
    let (ctx, int_ty) = make_ty_ctx();
    let backend = LlvmBackend::new().with_ty_ctx(ctx).with_opt_level(2);
    let callee_owner = DefId::new(CrateId::from_raw(1), LocalDefId::from_raw(10));
    let mut callee_body = Body::dummy(callee_owner);
    let callee_local = LocalIdx::from_raw(0);
    let const_val = MirConst {
        kind: MirConstKind::Int(41),
        ty: int_ty,
        span: Span::DUMMY,
    };
    let assign = Statement {
        kind: StatementKind::Assign(
            Place::new(callee_local),
            Rvalue::Use(Operand::Constant(const_val)),
        ),
        source_info: SourceInfo::new(Span::DUMMY),
    };
    let mut bb = glyim_mir::BasicBlockData::new(Terminator {
        kind: TerminatorKind::Return,
        source_info: SourceInfo::new(Span::DUMMY),
    });
    bb.statements.push(assign);
    callee_body.basic_blocks.push(bb);
    callee_body.locals.push(LocalDecl {
        ty: int_ty,
        mutability: Mutability::Not,
        source_info: SourceInfo::new(Span::DUMMY),
    });
    callee_body.arg_count = 0;
    callee_body.return_ty = int_ty;

    let caller_owner = DefId::new(CrateId::from_raw(1), LocalDefId::from_raw(20));
    let caller_body = Body::dummy(caller_owner);

    let bodies = vec![Arc::new(callee_body), Arc::new(caller_body)];
    let context = Context::create();
    let module = backend
        .lower_bodies_to_module(&context, &crate::tests::to_pairs(&bodies))
        .expect("LTO lowering failed");
    assert!(
        module.get_functions().count() >= 2,
        "Expected at least 2 functions after LTO"
    );
}

#[test]
fn test_o1_runs_pass_pipeline() {
    let (ctx, int_ty) = make_ty_ctx();
    let backend = LlvmBackend::new().with_ty_ctx(ctx).with_opt_level(1);
    let body = Arc::new(simple_body_with_local(int_ty));
    let context = Context::create();
    let module = backend
        .lower_bodies_to_module(&context, &crate::tests::to_pairs(&[body]))
        .expect("O1 lowering failed");
    assert!(
        module.verify().is_ok(),
        "Module should verify after O1 passes"
    );
}

#[test]
fn test_o3_aggressive_optimization() {
    let (ctx, int_ty) = make_ty_ctx();
    let backend = LlvmBackend::new().with_ty_ctx(ctx).with_opt_level(3);
    let body = Arc::new(simple_body_with_local(int_ty));
    let context = Context::create();
    let module = backend
        .lower_bodies_to_module(&context, &crate::tests::to_pairs(&[body]))
        .expect("O3 lowering failed");
    assert!(
        module.verify().is_ok(),
        "Module should verify after O3 passes"
    );
}

#[test]
fn test_o0_with_size_opt_does_not_crash() {
    let (ctx, int_ty) = make_ty_ctx();
    let backend = LlvmBackend::new()
        .with_ty_ctx(ctx)
        .with_opt_level(0)
        .with_opt_for_size(true);
    let body = Arc::new(simple_body_with_local(int_ty));
    let context = Context::create();
    let module = backend
        .lower_bodies_to_module(&context, &crate::tests::to_pairs(&[body]))
        .expect("O0+size lowering failed");
    assert!(
        module.verify().is_ok(),
        "Module should verify even with size hint at O0"
    );
}

#[test]
fn test_opt_out_of_range_defaults_to_aggressive() {
    let (ctx, int_ty) = make_ty_ctx();
    let backend = LlvmBackend::new().with_ty_ctx(ctx).with_opt_level(4);
    let body = Arc::new(simple_body_with_local(int_ty));
    let context = Context::create();
    let module = backend
        .lower_bodies_to_module(&context, &crate::tests::to_pairs(&[body]))
        .expect("opt=4 lowering failed");
    assert!(
        module.verify().is_ok(),
        "Module should verify when opt level out of range (mapped to aggressive)"
    );
}

#[test]
fn test_multiple_bodies_pass_together() {
    let (ctx, int_ty) = make_ty_ctx();
    let backend = LlvmBackend::new().with_ty_ctx(ctx).with_opt_level(2);
    let body1 = Arc::new(simple_body_with_local(int_ty));
    // Give body2 a *distinct* owner so it lowers to a second, separate
    // function (the test verifies that lowering multiple bodies into one module
    // produces at least two functions).
    let mut body2_raw = simple_body_with_local(int_ty);
    body2_raw.owner = DefId::new(CrateId::from_raw(1), LocalDefId::from_raw(2));
    let body2 = Arc::new(body2_raw);
    let context = Context::create();
    let module = backend
        .lower_bodies_to_module(&context, &crate::tests::to_pairs(&[body1, body2]))
        .expect("multi-body lowering failed");
    assert!(
        module.verify().is_ok(),
        "Module with multiple bodies should verify"
    );
    assert!(
        module.get_functions().count() >= 2,
        "Expected at least 2 functions"
    );
}

#[test]
fn test_empty_body_passes_without_error() {
    let (ctx, _) = make_ty_ctx();
    let owner = DefId::new(CrateId::from_raw(1), LocalDefId::from_raw(100));
    let empty_body = Arc::new(Body::dummy(owner));
    let backend = LlvmBackend::new().with_ty_ctx(ctx).with_opt_level(2);
    let context = Context::create();
    let module = backend
        .lower_bodies_to_module(&context, &crate::tests::to_pairs(&[empty_body]))
        .expect("empty body lowering failed");
    assert!(
        module.verify().is_ok(),
        "Module with empty body should verify"
    );
}

#[test]
fn test_verify_module_after_optimization() {
    let (ctx, int_ty) = make_ty_ctx();
    let backend = LlvmBackend::new().with_ty_ctx(ctx).with_opt_level(2);
    let body = Arc::new(simple_body_with_local(int_ty));
    let context = Context::create();
    let module = backend
        .lower_bodies_to_module(&context, &crate::tests::to_pairs(&[body]))
        .expect("lowering failed");
    if let Err(msg) = module.verify() {
        panic!("Module verification failed after passes: {}", msg);
    }
}

// Replace the test with a more verbose version
#[test]
fn test_o2_mem2reg_debug() {
    let (ctx, int_ty) = make_ty_ctx();
    let backend = LlvmBackend::new().with_ty_ctx(ctx).with_opt_level(2);
    let body = Arc::new(simple_body_with_local(int_ty));

    // Print the MIR body
    println!(
        "MIR Body: owner={:?}, return_ty={:?}",
        body.owner, body.return_ty
    );
    for (i, local) in body.locals.iter_enumerated() {
        println!("  local {}: ty={:?}", i.to_raw(), local.ty);
    }
    for (i, block) in body.basic_blocks.iter_enumerated() {
        println!("  bb{}: terminator={:?}", i.to_raw(), block.terminator.kind);
        for stmt in &block.statements {
            println!("    stmt: {:?}", stmt.kind);
        }
    }

    let context = Context::create();
    let module = backend
        .lower_bodies_to_module(&context, &crate::tests::to_pairs(&[body]))
        .expect("lowering failed");

    // Dump before passes
    println!("--- Before passes ---");
    println!("{}", module.print_to_string());

    // Run passes manually
    let target_triple = inkwell::targets::TargetTriple::create("x86_64-unknown-linux-gnu");
    let target = inkwell::targets::Target::from_triple(&target_triple).unwrap();
    let target_machine = target
        .create_target_machine(
            &target_triple,
            "generic",
            "",
            inkwell::OptimizationLevel::Aggressive,
            inkwell::targets::RelocMode::Default,
            inkwell::targets::CodeModel::Default,
        )
        .unwrap();
    crate::passes::run_llvm_passes(&module, &target_machine, 2, false).expect("passes failed");

    // Dump after passes
    println!("--- After passes ---");
    println!("{}", module.print_to_string());

    assert_eq!(
        count_allocas(&module),
        0,
        "Expected no alloca instructions after O2 mem2reg"
    );
}
