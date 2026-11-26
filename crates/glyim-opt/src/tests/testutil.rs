//! Test utilities for constructing MIR bodies.

use glyim_core::{CrateId, DefId, IndexVec, IntTy, LocalDefId, Mutability, UintTy};
use glyim_mir::*;
use glyim_span::Span;
use glyim_type::{Ty, TyKind};

/// Build a test MIR body from locals, basic blocks, arg count, and return type.
pub fn build_test_body(
    locals: Vec<(Ty, Mutability)>,
    blocks: Vec<BasicBlockData>,
    arg_count: usize,
    return_ty: Ty,
) -> Body {
    let local_decls: Vec<LocalDecl> = locals
        .into_iter()
        .map(|(ty, mutability)| LocalDecl {
            ty,
            mutability,
            source_info: SourceInfo::new(Span::DUMMY),
        })
        .collect();

    Body {
        owner: DefId::new(CrateId::from_usize(0), LocalDefId::from_usize(0)),
        basic_blocks: IndexVec::from_vec(blocks),
        locals: IndexVec::from_vec(local_decls),
        arg_count,
        return_ty,
        span: Span::DUMMY,
        var_debug_info: Vec::new(),
    }
}

/// Create an `i32` type.
pub fn ty_i32(ctx: &mut glyim_type::TyCtxMut) -> Ty {
    ctx.mk_ty(TyKind::Int(IntTy::I32))
}

/// Create a `u32` type.
pub fn ty_u32(ctx: &mut glyim_type::TyCtxMut) -> Ty {
    ctx.mk_ty(TyKind::Uint(UintTy::U32))
}

/// Create a `bool` type.
pub fn ty_bool(ctx: &mut glyim_type::TyCtxMut) -> Ty {
    ctx.bool_ty()
}

/// Create a constant `i128` operand.
pub fn const_int(val: i128, ty: Ty) -> Operand {
    Operand::Constant(MirConst {
        kind: MirConstKind::Int(val),
        ty,
        span: Span::DUMMY,
    })
}

/// Create a constant `u128` operand.
pub fn const_uint(val: u128, ty: Ty) -> Operand {
    Operand::Constant(MirConst {
        kind: MirConstKind::Uint(val),
        ty,
        span: Span::DUMMY,
    })
}

/// Create a constant `bool` operand.
pub fn const_bool_val(val: bool) -> Operand {
    Operand::Constant(MirConst {
        kind: MirConstKind::Bool(val),
        ty: Ty::BOOL,
        span: Span::DUMMY,
    })
}

/// Create a `Use(Operand::Constant)` rvalue from an i128 constant.
pub fn const_int_rvalue(val: i128, ty: Ty) -> Rvalue {
    Rvalue::Use(const_int(val, ty))
}

/// Create a `Use(Operand::Constant)` rvalue from a u128 constant.
pub fn const_uint_rvalue(val: u128, ty: Ty) -> Rvalue {
    Rvalue::Use(const_uint(val, ty))
}

/// Create a `Copy(place)` operand.
pub fn copy_op(local: LocalIdx) -> Operand {
    Operand::Copy(Place::new(local))
}

/// Create an `Assign` statement.
pub fn assign_stmt(place: Place, rvalue: Rvalue) -> Statement {
    Statement {
        kind: StatementKind::Assign(place, rvalue),
        source_info: SourceInfo::new(Span::DUMMY),
    }
}

/// Create a `StorageLive` statement.
pub fn storage_live_stmt(local: LocalIdx) -> Statement {
    Statement {
        kind: StatementKind::StorageLive(local),
        source_info: SourceInfo::new(Span::DUMMY),
    }
}

/// Create a `StorageDead` statement.
pub fn storage_dead_stmt(local: LocalIdx) -> Statement {
    Statement {
        kind: StatementKind::StorageDead(local),
        source_info: SourceInfo::new(Span::DUMMY),
    }
}

/// Create a `Goto` terminator.
pub fn goto_term(target: BasicBlockIdx) -> Terminator {
    Terminator {
        kind: TerminatorKind::Goto { target },
        source_info: SourceInfo::new(Span::DUMMY),
    }
}

/// Create a `Return` terminator.
pub fn return_term() -> Terminator {
    Terminator {
        kind: TerminatorKind::Return,
        source_info: SourceInfo::new(Span::DUMMY),
    }
}

/// Create a `SwitchInt` terminator for a boolean switch.
pub fn bool_switch_term(
    discr: Operand,
    true_bb: BasicBlockIdx,
    false_bb: BasicBlockIdx,
) -> Terminator {
    Terminator {
        kind: TerminatorKind::SwitchInt {
            discr,
            switch_ty: Ty::BOOL,
            targets: SwitchTargets::new(vec![(1, true_bb)].into_boxed_slice(), false_bb),
        },
        source_info: SourceInfo::new(Span::DUMMY),
    }
}

/// Create a basic block with statements and terminator.
pub fn make_block(stmts: Vec<Statement>, term: Terminator) -> BasicBlockData {
    BasicBlockData {
        statements: stmts,
        terminator: term,
        is_cleanup: false,
    }
}

/// Helper to create a `LocalIdx` from a raw u32.
pub fn local(idx: u32) -> LocalIdx {
    LocalIdx::from_usize(idx as usize)
}

/// Helper to create a `BasicBlockIdx` from a raw u32.
pub fn bb(idx: u32) -> BasicBlockIdx {
    BasicBlockIdx::from_usize(idx as usize)
}
