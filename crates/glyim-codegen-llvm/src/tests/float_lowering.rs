use crate::LlvmBackend;
use glyim_core::Interner;
use glyim_core::arena::IndexVec;
use glyim_core::def_id::{CrateId, DefId, LocalDefId};
use glyim_core::primitives::{FloatTy, Mutability};
use glyim_mir::{
    BasicBlockData, Body, LocalDecl, LocalIdx, MirConst, MirConstKind, Operand, Place, Rvalue,
    SourceInfo, Statement, StatementKind, Terminator, TerminatorKind,
};
use glyim_span::{ByteIdx, FileId, Span};
use glyim_type::{TyCtxMut, TyKind};

fn build_f32_body(ctx: &mut TyCtxMut) -> Body {
    let f32_ty = ctx.mk_ty(TyKind::Float(FloatTy::F32));

    let mut locals: IndexVec<LocalIdx, LocalDecl> = IndexVec::new();
    locals.push(LocalDecl {
        ty: f32_ty,
        mutability: Mutability::Not,
        source_info: SourceInfo::new(Span::new(
            FileId::BOGUS,
            ByteIdx::ZERO,
            ByteIdx::ZERO,
            glyim_span::SyntaxContext::ROOT,
        )),
    });

    let const_f32_1 = MirConst {
        kind: MirConstKind::FloatBits(u64::from(f32::to_bits(1.0f32))),
        ty: f32_ty,
        span: Span::DUMMY,
    };

    let mut basic_blocks: IndexVec<glyim_mir::BasicBlockIdx, BasicBlockData> = IndexVec::new();
    let stmt = Statement {
        kind: StatementKind::Assign(
            Place::new(LocalIdx::from_raw(0)),
            Rvalue::Use(Operand::Constant(const_f32_1)),
        ),
        source_info: SourceInfo::new(Span::DUMMY),
    };
    let mut bb0 = BasicBlockData::new(Terminator {
        kind: TerminatorKind::Return,
        source_info: SourceInfo::new(Span::DUMMY),
    });
    bb0.statements.push(stmt);
    basic_blocks.push(bb0);

    Body {
        owner: DefId::new(CrateId::from_raw(0), LocalDefId::from_raw(4)),
        basic_blocks,
        locals,
        arg_count: 0,
        return_ty: f32_ty,
        span: Span::DUMMY,
        var_debug_info: Vec::new(),
    }
}

fn build_f64_body(ctx: &mut TyCtxMut) -> Body {
    let f64_ty = ctx.mk_ty(TyKind::Float(FloatTy::F64));

    let mut locals: IndexVec<LocalIdx, LocalDecl> = IndexVec::new();
    locals.push(LocalDecl {
        ty: f64_ty,
        mutability: Mutability::Not,
        source_info: SourceInfo::new(Span::new(
            FileId::BOGUS,
            ByteIdx::ZERO,
            ByteIdx::ZERO,
            glyim_span::SyntaxContext::ROOT,
        )),
    });

    let const_f64_1 = MirConst {
        kind: MirConstKind::FloatBits(1.0f64.to_bits()),
        ty: f64_ty,
        span: Span::DUMMY,
    };

    let mut basic_blocks: IndexVec<glyim_mir::BasicBlockIdx, BasicBlockData> = IndexVec::new();
    let stmt = Statement {
        kind: StatementKind::Assign(
            Place::new(LocalIdx::from_raw(0)),
            Rvalue::Use(Operand::Constant(const_f64_1)),
        ),
        source_info: SourceInfo::new(Span::DUMMY),
    };
    let mut bb0 = BasicBlockData::new(Terminator {
        kind: TerminatorKind::Return,
        source_info: SourceInfo::new(Span::DUMMY),
    });
    bb0.statements.push(stmt);
    basic_blocks.push(bb0);

    Body {
        owner: DefId::new(CrateId::from_raw(0), LocalDefId::from_raw(5)),
        basic_blocks,
        locals,
        arg_count: 0,
        return_ty: f64_ty,
        span: Span::DUMMY,
        var_debug_info: Vec::new(),
    }
}

#[test]
fn u05_t05_f32_type_uses_llvm_f32() {
    let mut ctx = TyCtxMut::new(Interner::default());
    let body = build_f32_body(&mut ctx);
    let frozen = ctx.freeze();

    let backend = LlvmBackend::new().with_ty_ctx(frozen);
    let ir = backend
        .generate_ir(&body)
        .expect("generate_ir should succeed");

    assert!(
        ir.contains("float"),
        "IR should contain 'float' (LLVM f32), got:\n{ir}"
    );
}

#[test]
fn u05_t05_f64_type_uses_llvm_f64() {
    let mut ctx = TyCtxMut::new(Interner::default());
    let body = build_f64_body(&mut ctx);
    let frozen = ctx.freeze();

    let backend = LlvmBackend::new().with_ty_ctx(frozen);
    let ir = backend
        .generate_ir(&body)
        .expect("generate_ir should succeed");

    assert!(
        ir.contains("double"),
        "IR should contain 'double' (LLVM f64), got:\n{ir}"
    );
}
