//! Test W4-C02-T01: constant propagation replaces operands with constants
//! (binary operation remains, but operands become constants)
use glyim_core::{CrateId, DefId, IndexVec, LocalDefId, primitives::IntTy};
use glyim_mir::*;
use glyim_span::Span;
use glyim_test::test_ty_ctx;

fn dummy_def_id() -> DefId {
    DefId::new(CrateId::from_raw(0), LocalDefId::from_raw(0))
}

#[test]
fn constant_prop_single_block() {
    let mut ctx_mut = test_ty_ctx();
    let i32_ty = ctx_mut.mk_ty(glyim_type::TyKind::Int(IntTy::I32));
    let mut body = Body::dummy(dummy_def_id());
    let local0 = LocalIdx::from_raw(0);
    let local1 = LocalIdx::from_raw(1);
    let local2 = LocalIdx::from_raw(2);
    body.locals.push(LocalDecl {
        ty: i32_ty,
        mutability: glyim_core::primitives::Mutability::Mut,
        source_info: SourceInfo::new(Span::DUMMY),
    });
    body.locals.push(LocalDecl {
        ty: i32_ty,
        mutability: glyim_core::primitives::Mutability::Mut,
        source_info: SourceInfo::new(Span::DUMMY),
    });
    body.locals.push(LocalDecl {
        ty: i32_ty,
        mutability: glyim_core::primitives::Mutability::Mut,
        source_info: SourceInfo::new(Span::DUMMY),
    });
    body.basic_blocks = IndexVec::from_raw(vec![BasicBlockData {
        statements: vec![
            Statement {
                kind: StatementKind::Assign(
                    Place::new(local1),
                    Rvalue::Use(Operand::Constant(MirConst {
                        kind: MirConstKind::Int(5),
                        ty: i32_ty,
                        span: Span::DUMMY,
                    })),
                ),
                source_info: SourceInfo::new(Span::DUMMY),
            },
            Statement {
                kind: StatementKind::Assign(
                    Place::new(local2),
                    Rvalue::BinaryOp(
                        glyim_core::primitives::BinOp::Add,
                        Box::new((
                            Operand::Copy(Place::new(local1)),
                            Operand::Constant(MirConst {
                                kind: MirConstKind::Int(1),
                                ty: i32_ty,
                                span: Span::DUMMY,
                            }),
                        )),
                    ),
                ),
                source_info: SourceInfo::new(Span::DUMMY),
            },
            Statement {
                kind: StatementKind::Assign(
                    Place::new(local0),
                    Rvalue::Use(Operand::Move(Place::new(local2))),
                ),
                source_info: SourceInfo::new(Span::DUMMY),
            },
        ],
        terminator: Terminator {
            kind: TerminatorKind::Return,
            source_info: SourceInfo::new(Span::DUMMY),
        },
        is_cleanup: false,
    }]);
    let ctx = ctx_mut.freeze();
    crate::constant_prop::run(&ctx, &mut body);
    let block = &body.basic_blocks[BasicBlockIdx::from_raw(0)];
    let stmt = &block.statements[1];
    match &stmt.kind {
        StatementKind::Assign(place, rvalue) => {
            assert_eq!(place.local, LocalIdx::from_raw(2));
            // Expect binary op with constant operands (5 and 1), not folded into 6
            match rvalue {
                Rvalue::BinaryOp(op, box_ops) => {
                    assert_eq!(*op, glyim_core::primitives::BinOp::Add);
                    match (&box_ops.0, &box_ops.1) {
                        (Operand::Constant(lc), Operand::Constant(rc)) => {
                            match &lc.kind {
                                MirConstKind::Int(5) => {}
                                _ => panic!("Expected Int(5)"),
                            };
                            match &rc.kind {
                                MirConstKind::Int(1) => {}
                                _ => panic!("Expected Int(1)"),
                            };
                        }
                        _ => panic!("Expected constant operands"),
                    }
                }
                _ => panic!("Expected BinaryOp after propagation"),
            }
        }
        _ => panic!("Expected assign"),
    }
}

/// MIR-1 regression: const-prop must simulate a block from its *entry* state,
/// not apply the block's exit map to every statement.
///
/// `_1 = const 10; _2 = move _1; _1 = const 3; _3 = move _1;` — the exit map
/// says `_1 = 3`, so the old code rewrote `_2 = move _1` to `_2 = const 3`.
/// It must be `_2 = const 10`.
#[test]
fn const_prop_uses_entry_state_not_exit_state() {
    let mut ctx_mut = test_ty_ctx();
    let i32_ty = ctx_mut.mk_ty(glyim_type::TyKind::Int(IntTy::I32));
    let mut body = Body::dummy(dummy_def_id());
    let mk_local = |body: &mut Body| {
        body.locals.push(LocalDecl {
            ty: i32_ty,
            mutability: glyim_core::primitives::Mutability::Mut,
            source_info: SourceInfo::new(Span::DUMMY),
        });
    };
    // locals 0..=3
    for _ in 0..4 {
        mk_local(&mut body);
    }
    let c = |n: i128| Operand::Constant(MirConst {
        kind: MirConstKind::Int(n),
        ty: i32_ty,
        span: Span::DUMMY,
    });
    let asn = |dst: u32, rv: Rvalue| Statement {
        kind: StatementKind::Assign(Place::new(LocalIdx::from_raw(dst)), rv),
        source_info: SourceInfo::new(Span::DUMMY),
    };
    body.basic_blocks = IndexVec::from_raw(vec![BasicBlockData {
        statements: vec![
            asn(1, Rvalue::Use(c(10))),                              // _1 = 10
            asn(2, Rvalue::Use(Operand::Move(Place::new(LocalIdx::from_raw(1))))), // _2 = move _1
            asn(1, Rvalue::Use(c(3))),                               // _1 = 3
            asn(3, Rvalue::Use(Operand::Move(Place::new(LocalIdx::from_raw(1))))), // _3 = move _1
        ],
        terminator: Terminator {
            kind: TerminatorKind::Return,
            source_info: SourceInfo::new(Span::DUMMY),
        },
        is_cleanup: false,
    }]);
    let ctx = ctx_mut.freeze();
    crate::constant_prop::run(&ctx, &mut body);
    let block = &body.basic_blocks[BasicBlockIdx::from_raw(0)];

    // `_2 = move _1` must see `_1 == 10`, the value at that *point*, not 3.
    match &block.statements[1].kind {
        StatementKind::Assign(_, Rvalue::Use(Operand::Constant(cst))) => match &cst.kind {
            MirConstKind::Int(10) => {}
            other => panic!("MIR-1: `_2 = move _1` folded to {other:?}, expected Int(10)"),
        },
        other => panic!("expected `_2 = const 10`, got {other:?}"),
    }
    // `_3 = move _1` must see `_1 == 3`.
    match &block.statements[3].kind {
        StatementKind::Assign(_, Rvalue::Use(Operand::Constant(cst))) => match &cst.kind {
            MirConstKind::Int(3) => {}
            other => panic!("`_3 = move _1` folded to {other:?}, expected Int(3)"),
        },
        other => panic!("expected `_3 = const 3`, got {other:?}"),
    }
}
