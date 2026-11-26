use crate::{InterpValue, Interpreter};
use glyim_core::{CrateId, DefId, IndexVec, IntTy, Interner, LocalDefId, Mutability};
use glyim_mir::*;
use glyim_span::Span;
use glyim_type::{TyCtx, TyCtxMut, TyKind};

fn setup_test_body(f: impl FnOnce(&mut TyCtxMut) -> Body) -> (TyCtx, Body) {
    let mut ctx_mut = TyCtxMut::new(Interner::default());
    let body = f(&mut ctx_mut);
    (ctx_mut.freeze(), body)
}

#[test]
fn switch_int_three_targets() {
    let (ctx, body) = setup_test_body(|ctx_mut| {
        let i32_ty = ctx_mut.mk_ty(TyKind::Int(IntTy::I32));
        let mut locals = IndexVec::new();
        let ret_local = locals.push(LocalDecl {
            ty: i32_ty,
            mutability: Mutability::Not,
            source_info: SourceInfo::new(Span::DUMMY),
        });
        let mut blocks = IndexVec::new();
        let term0 = Terminator {
            kind: TerminatorKind::SwitchInt {
                discr: Operand::Constant(MirConst {
                    kind: MirConstKind::Int(1),
                    ty: i32_ty,
                    span: Span::DUMMY,
                }),
                switch_ty: i32_ty,
                targets: SwitchTargets::new(
                    Box::new([
                        (1, BasicBlockIdx::from_usize(1)),
                        (2, BasicBlockIdx::from_usize(2)),
                    ]),
                    BasicBlockIdx::from_usize(3),
                ),
            },
            source_info: SourceInfo::new(Span::DUMMY),
        };
        blocks.push(BasicBlockData {
            statements: vec![],
            terminator: term0,
            is_cleanup: false,
        });

        let stmt1 = Statement {
            kind: StatementKind::Assign(
                Place::new(ret_local),
                Rvalue::Use(Operand::Constant(MirConst {
                    kind: MirConstKind::Int(10),
                    ty: i32_ty,
                    span: Span::DUMMY,
                })),
            ),
            source_info: SourceInfo::new(Span::DUMMY),
        };
        let term1 = Terminator {
            kind: TerminatorKind::Goto {
                target: BasicBlockIdx::from_usize(4),
            },
            source_info: SourceInfo::new(Span::DUMMY),
        };
        blocks.push(BasicBlockData {
            statements: vec![stmt1],
            terminator: term1,
            is_cleanup: false,
        });

        let stmt2 = Statement {
            kind: StatementKind::Assign(
                Place::new(ret_local),
                Rvalue::Use(Operand::Constant(MirConst {
                    kind: MirConstKind::Int(20),
                    ty: i32_ty,
                    span: Span::DUMMY,
                })),
            ),
            source_info: SourceInfo::new(Span::DUMMY),
        };
        let term2 = Terminator {
            kind: TerminatorKind::Goto {
                target: BasicBlockIdx::from_usize(4),
            },
            source_info: SourceInfo::new(Span::DUMMY),
        };
        blocks.push(BasicBlockData {
            statements: vec![stmt2],
            terminator: term2,
            is_cleanup: false,
        });

        let stmt3 = Statement {
            kind: StatementKind::Assign(
                Place::new(ret_local),
                Rvalue::Use(Operand::Constant(MirConst {
                    kind: MirConstKind::Int(30),
                    ty: i32_ty,
                    span: Span::DUMMY,
                })),
            ),
            source_info: SourceInfo::new(Span::DUMMY),
        };
        let term3 = Terminator {
            kind: TerminatorKind::Goto {
                target: BasicBlockIdx::from_usize(4),
            },
            source_info: SourceInfo::new(Span::DUMMY),
        };
        blocks.push(BasicBlockData {
            statements: vec![stmt3],
            terminator: term3,
            is_cleanup: false,
        });

        let term4 = Terminator {
            kind: TerminatorKind::Return,
            source_info: SourceInfo::new(Span::DUMMY),
        };
        blocks.push(BasicBlockData {
            statements: vec![],
            terminator: term4,
            is_cleanup: false,
        });

        Body {
            owner: DefId::new(CrateId::from_usize(0), LocalDefId::from_usize(0)),
            basic_blocks: blocks,
            locals,
            arg_count: 0,
            return_ty: i32_ty,
            span: Span::DUMMY,
            var_debug_info: vec![],
        }
    });
    let mut interp = Interpreter::new(&ctx);
    assert!(interp.run_body(&body).is_ok());
    assert_eq!(interp.get_return_value().unwrap(), InterpValue::Int(10));
}
