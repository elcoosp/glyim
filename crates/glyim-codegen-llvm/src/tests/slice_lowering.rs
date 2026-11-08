use crate::LlvmBackend;
use glyim_core::arena::IndexVec;
use glyim_core::primitives::*;
use glyim_core::{CrateId, DefId, LocalDefId, Mutability};
use glyim_mir::{
    BasicBlockData, Body, LocalDecl, LocalIdx, MirConst, MirConstKind, Operand, Place,
    ProjectionElem, Rvalue, SourceInfo, Statement, StatementKind, Terminator, TerminatorKind,
};
use glyim_span::Span;
use glyim_test::with_fresh_ty_ctx;
use glyim_type::TyKind;

#[test]
fn slice_type_lowering() {
    let (ctx, body) = with_fresh_ty_ctx(|ctx_mut| {
        let i32_ty = ctx_mut.mk_ty(TyKind::Int(IntTy::I32));
        let slice_ty = ctx_mut.mk_ty(TyKind::Slice(i32_ty));
        let idx_ty = ctx_mut.mk_ty(TyKind::Int(IntTy::I32));

        let mut locals = IndexVec::new();
        locals.push(LocalDecl {
            ty: i32_ty,
            mutability: Mutability::Mut,
            source_info: SourceInfo { span: Span::DUMMY },
        });
        locals.push(LocalDecl {
            ty: slice_ty,
            mutability: Mutability::Not,
            source_info: SourceInfo { span: Span::DUMMY },
        });
        locals.push(LocalDecl {
            ty: idx_ty,
            mutability: Mutability::Not,
            source_info: SourceInfo { span: Span::DUMMY },
        });
        locals.push(LocalDecl {
            ty: i32_ty,
            mutability: Mutability::Mut,
            source_info: SourceInfo { span: Span::DUMMY },
        });

        let zero_const = MirConst {
            kind: MirConstKind::Int(0),
            ty: idx_ty,
            span: Span::DUMMY,
        };

        let basic_blocks = {
            let mut bbs = IndexVec::new();
            let mut statements = Vec::new();
            statements.push(Statement {
                kind: StatementKind::Assign(
                    Place::new(LocalIdx::from_raw(2)),
                    Rvalue::Use(Operand::Constant(zero_const.clone())),
                ),
                source_info: SourceInfo { span: Span::DUMMY },
            });
            let elem_place = Place {
                local: LocalIdx::from_raw(1),
                projection: vec![ProjectionElem::Index(LocalIdx::from_raw(2))].into_boxed_slice(),
            };
            statements.push(Statement {
                kind: StatementKind::Assign(
                    Place::new(LocalIdx::from_raw(3)),
                    Rvalue::Use(Operand::Copy(elem_place)),
                ),
                source_info: SourceInfo { span: Span::DUMMY },
            });
            let terminator = Terminator {
                kind: TerminatorKind::Return,
                source_info: SourceInfo { span: Span::DUMMY },
            };
            bbs.push(BasicBlockData {
                statements,
                terminator,
                is_cleanup: false,
            });
            bbs
        };

        Body {
            owner: DefId::new(CrateId::from_raw(0), LocalDefId::from_raw(1)),
            basic_blocks,
            locals,
            arg_count: 1,
            return_ty: i32_ty,
            span: Span::DUMMY,
            var_debug_info: vec![],
        }
    });

    let backend = LlvmBackend::new().with_ty_ctx(ctx);
    let ir = backend.generate_ir(&body).expect("IR generation failed");
    assert!(
        ir.contains("{ i32*, i64 }") || ir.contains("{ ptr, i64 }"),
        "Slice type not lowered to fat pointer, IR:\n{ir}"
    );
}

#[test]
fn slice_index_emits_gep_and_load() {
    let (ctx, body) = with_fresh_ty_ctx(|ctx_mut| {
        let i32_ty = ctx_mut.mk_ty(TyKind::Int(IntTy::I32));
        let slice_ty = ctx_mut.mk_ty(TyKind::Slice(i32_ty));
        let idx_ty = ctx_mut.mk_ty(TyKind::Int(IntTy::I32));

        let mut locals = IndexVec::new();
        locals.push(LocalDecl {
            ty: i32_ty,
            mutability: Mutability::Mut,
            source_info: SourceInfo { span: Span::DUMMY },
        });
        locals.push(LocalDecl {
            ty: slice_ty,
            mutability: Mutability::Not,
            source_info: SourceInfo { span: Span::DUMMY },
        });
        locals.push(LocalDecl {
            ty: idx_ty,
            mutability: Mutability::Not,
            source_info: SourceInfo { span: Span::DUMMY },
        });
        locals.push(LocalDecl {
            ty: i32_ty,
            mutability: Mutability::Mut,
            source_info: SourceInfo { span: Span::DUMMY },
        });

        let zero_const = MirConst {
            kind: MirConstKind::Int(0),
            ty: idx_ty,
            span: Span::DUMMY,
        };

        let basic_blocks = {
            let mut bbs = IndexVec::new();
            let mut statements = Vec::new();
            statements.push(Statement {
                kind: StatementKind::Assign(
                    Place::new(LocalIdx::from_raw(2)),
                    Rvalue::Use(Operand::Constant(zero_const.clone())),
                ),
                source_info: SourceInfo { span: Span::DUMMY },
            });
            let elem_place = Place {
                local: LocalIdx::from_raw(1),
                projection: vec![ProjectionElem::Index(LocalIdx::from_raw(2))].into_boxed_slice(),
            };
            statements.push(Statement {
                kind: StatementKind::Assign(
                    Place::new(LocalIdx::from_raw(3)),
                    Rvalue::Use(Operand::Copy(elem_place)),
                ),
                source_info: SourceInfo { span: Span::DUMMY },
            });
            let terminator = Terminator {
                kind: TerminatorKind::Return,
                source_info: SourceInfo { span: Span::DUMMY },
            };
            bbs.push(BasicBlockData {
                statements,
                terminator,
                is_cleanup: false,
            });
            bbs
        };

        Body {
            owner: DefId::new(CrateId::from_raw(0), LocalDefId::from_raw(1)),
            basic_blocks,
            locals,
            arg_count: 1,
            return_ty: i32_ty,
            span: Span::DUMMY,
            var_debug_info: vec![],
        }
    });

    let backend = LlvmBackend::new().with_ty_ctx(ctx);
    let ir = backend.generate_ir(&body).expect("IR generation failed");
    assert!(
        ir.contains("getelementptr") && ir.contains("load"),
        "Expected GEP and load instructions for slice indexing, IR:\n{ir}"
    );
    assert!(
        ir.contains("load i32"),
        "Expected load of i32, got different type, IR:\n{ir}"
    );
}
