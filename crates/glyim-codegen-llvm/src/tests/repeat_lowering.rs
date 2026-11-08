use crate::LlvmBackend;
use glyim_core::Interner;
use glyim_core::arena::IndexVec;
use glyim_core::def_id::{CrateId, DefId, LocalDefId};
use glyim_core::primitives::{IntTy, Mutability, UintTy};
use glyim_mir::{
    BasicBlockData, Body, LocalDecl, LocalIdx, MirConst, MirConstKind, Operand, Place, Rvalue,
    SourceInfo, Statement, StatementKind, Terminator, TerminatorKind,
};
use glyim_span::{ByteIdx, FileId, Span};
use glyim_type::{TyCtxMut, TyKind};

fn build_repeat_body(ctx: &mut TyCtxMut) -> Body {
    let i32_ty = ctx.mk_ty(TyKind::Int(IntTy::I32));
    let usize_ty = ctx.mk_ty(TyKind::Uint(UintTy::Usize));
    let count = glyim_type::Const {
        kind: glyim_type::ConstKind::Uint(4),
        ty: usize_ty,
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
    locals.push(LocalDecl {
        ty: i32_ty,
        mutability: Mutability::Not,
        source_info: SourceInfo::new(Span::DUMMY),
    });

    let const_42 = MirConst {
        kind: MirConstKind::Int(42),
        ty: i32_ty,
        span: Span::DUMMY,
    };
    let count_const = MirConst {
        kind: MirConstKind::Uint(4),
        ty: usize_ty,
        span: Span::DUMMY,
    };

    let mut basic_blocks: IndexVec<glyim_mir::BasicBlockIdx, BasicBlockData> = IndexVec::new();

    let stmt_elem = Statement {
        kind: StatementKind::Assign(
            Place::new(LocalIdx::from_raw(1)),
            Rvalue::Use(Operand::Constant(const_42)),
        ),
        source_info: SourceInfo::new(Span::DUMMY),
    };
    let stmt_repeat = Statement {
        kind: StatementKind::Assign(
            Place::new(LocalIdx::from_raw(0)),
            Rvalue::Repeat(
                Operand::Copy(Place::new(LocalIdx::from_raw(1))),
                count_const,
            ),
        ),
        source_info: SourceInfo::new(Span::DUMMY),
    };

    let mut bb0 = BasicBlockData::new(Terminator {
        kind: TerminatorKind::Return,
        source_info: SourceInfo::new(Span::DUMMY),
    });
    bb0.statements.push(stmt_elem);
    bb0.statements.push(stmt_repeat);
    basic_blocks.push(bb0);

    Body {
        owner: DefId::new(CrateId::from_raw(0), LocalDefId::from_raw(3)),
        basic_blocks,
        locals,
        arg_count: 0,
        return_ty: array_ty,
        span: Span::DUMMY,
        var_debug_info: Vec::new(),
    }
}

#[test]
fn u05_t04_repeat_rvalue_builds_array() {
    let mut ctx = TyCtxMut::new(Interner::default());
    let body = build_repeat_body(&mut ctx);
    let frozen = ctx.freeze();

    let backend = LlvmBackend::new().with_ty_ctx(frozen);
    let ir = backend
        .generate_ir(&body)
        .expect("generate_ir should succeed");

    assert!(
        ir.contains("[4 x i32]"),
        "IR should contain array type [4 x i32] from repeat, got:\n{ir}"
    );
    assert!(
        ir.contains("insertvalue"),
        "IR should contain insertvalue for repeat construction, got:\n{ir}"
    );
}
