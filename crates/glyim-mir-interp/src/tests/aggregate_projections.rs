use crate::{InterpValue, Interpreter};
use glyim_core::{CrateId, DefId, IndexVec, IntTy, Interner, LocalDefId, Mutability};
use glyim_mir::*;
use glyim_span::Span;
use glyim_type::{FieldIdx, GenericArg, TyCtx, TyCtxMut, TyKind};

fn setup_test_body(f: impl FnOnce(&mut TyCtxMut) -> Body) -> (TyCtx, Body) {
    let mut ctx_mut = TyCtxMut::new(Interner::default());
    let body = f(&mut ctx_mut);
    (ctx_mut.freeze(), body)
}

#[test]
fn aggregate_tuple_field_write() {
    let (ctx, body) = setup_test_body(|ctx_mut| {
        let i32_ty = ctx_mut.mk_ty(TyKind::Int(IntTy::I32));
        let sub = ctx_mut.intern_substitution(vec![GenericArg::Ty(i32_ty), GenericArg::Ty(i32_ty)]);
        let tuple_ty = ctx_mut.mk_ty(TyKind::Tuple(sub));
        let mut locals = IndexVec::new();
        let ret_local = locals.push(LocalDecl {
            ty: i32_ty,
            mutability: Mutability::Not,
            source_info: SourceInfo::new(Span::DUMMY),
        });
        let mut_var = locals.push(LocalDecl {
            ty: tuple_ty,
            mutability: Mutability::Mut,
            source_info: SourceInfo::new(Span::DUMMY),
        });
        let mut blocks = IndexVec::new();

        let stmt_init = Statement {
            kind: StatementKind::Assign(
                Place::new(mut_var),
                Rvalue::Aggregate(
                    AggregateKind::Tuple,
                    vec![
                        Operand::Constant(MirConst {
                            kind: MirConstKind::Int(0),
                            ty: i32_ty,
                            span: Span::DUMMY,
                        }),
                        Operand::Constant(MirConst {
                            kind: MirConstKind::Int(0),
                            ty: i32_ty,
                            span: Span::DUMMY,
                        }),
                    ],
                ),
            ),
            source_info: SourceInfo::new(Span::DUMMY),
        };

        let place = Place {
            local: mut_var,
            projection: Box::new([ProjectionElem::Field(FieldIdx::from_usize(0))]),
        };
        let stmt_write = Statement {
            kind: StatementKind::Assign(
                place,
                Rvalue::Use(Operand::Constant(MirConst {
                    kind: MirConstKind::Int(42),
                    ty: i32_ty,
                    span: Span::DUMMY,
                })),
            ),
            source_info: SourceInfo::new(Span::DUMMY),
        };
        let place2 = Place {
            local: mut_var,
            projection: Box::new([ProjectionElem::Field(FieldIdx::from_usize(0))]),
        };
        let stmt_read = Statement {
            kind: StatementKind::Assign(Place::new(ret_local), Rvalue::Use(Operand::Copy(place2))),
            source_info: SourceInfo::new(Span::DUMMY),
        };
        let term = Terminator {
            kind: TerminatorKind::Return,
            source_info: SourceInfo::new(Span::DUMMY),
        };
        blocks.push(BasicBlockData {
            statements: vec![stmt_init, stmt_write, stmt_read],
            terminator: term,
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
    let result = interp.run_body(&body);
    assert!(result.is_ok(), "Failed: {:?}", result.err());
    assert_eq!(interp.get_return_value().unwrap(), InterpValue::Int(42));
}

#[test]
fn aggregate_nested_projection_write() {
    let (ctx, body) = setup_test_body(|ctx_mut| {
        let i32_ty = ctx_mut.mk_ty(TyKind::Int(IntTy::I32));
        let inner_sub = ctx_mut.intern_substitution(vec![GenericArg::Ty(i32_ty)]);
        let inner_tuple = ctx_mut.mk_ty(TyKind::Tuple(inner_sub));
        let outer_sub = ctx_mut.intern_substitution(vec![GenericArg::Ty(inner_tuple)]);
        let outer_tuple = ctx_mut.mk_ty(TyKind::Tuple(outer_sub));
        let mut locals = IndexVec::new();
        let ret_local = locals.push(LocalDecl {
            ty: i32_ty,
            mutability: Mutability::Not,
            source_info: SourceInfo::new(Span::DUMMY),
        });
        let mut_var = locals.push(LocalDecl {
            ty: outer_tuple,
            mutability: Mutability::Mut,
            source_info: SourceInfo::new(Span::DUMMY),
        });
        let inner_tmp = locals.push(LocalDecl {
            ty: inner_tuple,
            mutability: Mutability::Not,
            source_info: SourceInfo::new(Span::DUMMY),
        });
        let mut blocks = IndexVec::new();

        let stmt_init_inner = Statement {
            kind: StatementKind::Assign(
                Place::new(inner_tmp),
                Rvalue::Aggregate(
                    AggregateKind::Tuple,
                    vec![Operand::Constant(MirConst {
                        kind: MirConstKind::Int(0),
                        ty: i32_ty,
                        span: Span::DUMMY,
                    })],
                ),
            ),
            source_info: SourceInfo::new(Span::DUMMY),
        };
        let stmt_init_outer = Statement {
            kind: StatementKind::Assign(
                Place::new(mut_var),
                Rvalue::Aggregate(
                    AggregateKind::Tuple,
                    vec![Operand::Copy(Place::new(inner_tmp))],
                ),
            ),
            source_info: SourceInfo::new(Span::DUMMY),
        };

        let place = Place {
            local: mut_var,
            projection: Box::new([
                ProjectionElem::Field(FieldIdx::from_usize(0)),
                ProjectionElem::Field(FieldIdx::from_usize(0)),
            ]),
        };
        let stmt_write = Statement {
            kind: StatementKind::Assign(
                place,
                Rvalue::Use(Operand::Constant(MirConst {
                    kind: MirConstKind::Int(99),
                    ty: i32_ty,
                    span: Span::DUMMY,
                })),
            ),
            source_info: SourceInfo::new(Span::DUMMY),
        };
        let place2 = Place {
            local: mut_var,
            projection: Box::new([
                ProjectionElem::Field(FieldIdx::from_usize(0)),
                ProjectionElem::Field(FieldIdx::from_usize(0)),
            ]),
        };
        let stmt_read = Statement {
            kind: StatementKind::Assign(Place::new(ret_local), Rvalue::Use(Operand::Copy(place2))),
            source_info: SourceInfo::new(Span::DUMMY),
        };
        let term = Terminator {
            kind: TerminatorKind::Return,
            source_info: SourceInfo::new(Span::DUMMY),
        };
        blocks.push(BasicBlockData {
            statements: vec![stmt_init_inner, stmt_init_outer, stmt_write, stmt_read],
            terminator: term,
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
    let result = interp.run_body(&body);
    assert!(result.is_ok(), "Failed: {:?}", result.err());
    assert_eq!(interp.get_return_value().unwrap(), InterpValue::Int(99));
}
