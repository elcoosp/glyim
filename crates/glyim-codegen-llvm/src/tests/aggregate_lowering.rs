use crate::LlvmBackend;
use glyim_core::Interner;
use glyim_core::arena::IndexVec;
use glyim_core::def_id::{CrateId, DefId, LocalDefId};
use glyim_core::primitives::{IntTy, Mutability};
use glyim_mir::{
    BasicBlockData, Body, LocalDecl, LocalIdx, MirConst, MirConstKind, Operand, Place, Rvalue,
    SourceInfo, Statement, StatementKind, Terminator, TerminatorKind,
};
use glyim_span::{ByteIdx, FileId, Span};
use glyim_type::{TyCtxMut, TyKind};

fn build_array_body(ctx: &mut TyCtxMut) -> Body {
    let i32_ty = ctx.mk_ty(TyKind::Int(IntTy::I32));
    let count = glyim_type::Const {
        kind: glyim_type::ConstKind::Uint(3),
        ty: i32_ty,
    };
    let array_ty = ctx.mk_ty(TyKind::Array(i32_ty, count));

    let mut locals: IndexVec<LocalIdx, LocalDecl> = IndexVec::new();
    locals.push(LocalDecl {
        ty: array_ty,
        mutability: Mutability::Not,
        source_info: SourceInfo::new(Span::new(
            FileId::BOGUS,
            ByteIdx::ZERO,
            ByteIdx::ZERO,
            glyim_span::SyntaxContext::ROOT,
        )),
    });
    for _ in 0..3 {
        locals.push(LocalDecl {
            ty: i32_ty,
            mutability: Mutability::Not,
            source_info: SourceInfo::new(Span::DUMMY),
        });
    }

    let const_10 = MirConst {
        kind: MirConstKind::Int(10),
        ty: i32_ty,
        span: Span::DUMMY,
    };
    let const_20 = MirConst {
        kind: MirConstKind::Int(20),
        ty: i32_ty,
        span: Span::DUMMY,
    };
    let const_30 = MirConst {
        kind: MirConstKind::Int(30),
        ty: i32_ty,
        span: Span::DUMMY,
    };

    let mut basic_blocks: IndexVec<glyim_mir::BasicBlockIdx, BasicBlockData> = IndexVec::new();

    let stmts = vec![
        Statement {
            kind: StatementKind::Assign(
                Place::new(LocalIdx::from_raw(1)),
                Rvalue::Use(Operand::Constant(const_10)),
            ),
            source_info: SourceInfo::new(Span::DUMMY),
        },
        Statement {
            kind: StatementKind::Assign(
                Place::new(LocalIdx::from_raw(2)),
                Rvalue::Use(Operand::Constant(const_20)),
            ),
            source_info: SourceInfo::new(Span::DUMMY),
        },
        Statement {
            kind: StatementKind::Assign(
                Place::new(LocalIdx::from_raw(3)),
                Rvalue::Use(Operand::Constant(const_30)),
            ),
            source_info: SourceInfo::new(Span::DUMMY),
        },
        Statement {
            kind: StatementKind::Assign(
                Place::new(LocalIdx::from_raw(0)),
                Rvalue::Aggregate(
                    glyim_mir::AggregateKind::Array(i32_ty),
                    vec![
                        Operand::Copy(Place::new(LocalIdx::from_raw(1))),
                        Operand::Copy(Place::new(LocalIdx::from_raw(2))),
                        Operand::Copy(Place::new(LocalIdx::from_raw(3))),
                    ],
                ),
            ),
            source_info: SourceInfo::new(Span::DUMMY),
        },
    ];

    let mut bb0 = BasicBlockData::new(Terminator {
        kind: TerminatorKind::Return,
        source_info: SourceInfo::new(Span::DUMMY),
    });
    bb0.statements = stmts;
    basic_blocks.push(bb0);

    Body {
        owner: DefId::new(CrateId::from_raw(0), LocalDefId::from_raw(1)),
        basic_blocks,
        locals,
        arg_count: 0,
        return_ty: array_ty,
        span: Span::DUMMY,
        var_debug_info: Vec::new(),
    }
}

#[test]
fn u05_t02_array_literal_lowered_to_llvm_array() {
    let mut ctx = TyCtxMut::new(Interner::default());
    let body = build_array_body(&mut ctx);
    let frozen = ctx.freeze();

    let backend = LlvmBackend::new().with_ty_ctx(frozen);
    let ir = backend
        .generate_ir(&body)
        .expect("generate_ir should succeed");

    assert!(
        ir.contains("[3 x i32]"),
        "IR should contain array type [3 x i32], got:\n{ir}"
    );
    assert!(
        ir.contains("insertvalue"),
        "IR should contain insertvalue for array construction, got:\n{ir}"
    );
}
