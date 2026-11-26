//! Test W4-C02-T04: Unreachable block after `return` is removed
use glyim_core::{CrateId, DefId, IndexVec, LocalDefId, primitives::IntTy};
use glyim_mir::*;
use glyim_span::Span;
use glyim_test::test_ty_ctx;

fn dummy_def_id() -> DefId {
    DefId::new(CrateId::from_usize(0), LocalDefId::from_usize(0))
}

#[test]
fn remove_block_after_return() {
    let mut ctx_mut = test_ty_ctx();
    let i32_ty = ctx_mut.mk_ty(glyim_type::TyKind::Int(IntTy::I32));
    let mut body = Body::dummy(dummy_def_id());
    let local0 = LocalIdx::from_usize(0);
    body.locals.push(LocalDecl {
        ty: i32_ty,
        mutability: glyim_core::primitives::Mutability::Mut,
        source_info: SourceInfo::new(Span::DUMMY),
    });
    let bb0 = BasicBlockIdx::from_usize(0);
    let _bb1 = BasicBlockIdx::from_usize(1);
    body.basic_blocks = IndexVec::from_vec(vec![
        BasicBlockData {
            statements: vec![Statement {
                kind: StatementKind::Assign(
                    Place::new(local0),
                    Rvalue::Use(Operand::Constant(MirConst {
                        kind: MirConstKind::Int(42),
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
        },
        BasicBlockData {
            statements: vec![],
            terminator: Terminator {
                kind: TerminatorKind::Unreachable,
                source_info: SourceInfo::new(Span::DUMMY),
            },
            is_cleanup: false,
        },
    ]);
    let ctx = ctx_mut.freeze();
    crate::unreachable_elim::run(&ctx, &mut body);
    assert_eq!(body.basic_blocks.len(), 1);
    match &body.basic_blocks[bb0].terminator.kind {
        TerminatorKind::Return => {}
        _ => panic!("Expected Return"),
    }
}
