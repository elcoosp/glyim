//! Inline binary-op operand cost harness (plan PF-12 / M2.7 quick win).
//!
//! Dependency-free (`harness = false`), mirroring the `glyim-opt`
//! `pass_plans` bench style. Measures the shape the M2.7
//! "`Box<(Operand,Operand)>` removal" changed:
//!
//! 1. constructing N `Rvalue::binary_op(..)` rvalues — the smart-constructor
//!    path every lowering producer takes (the removed `Box::new` pair
//!    allocation used to sit inside this loop);
//! 2. validating a body of N binary-op statements — the M5.5 validator's
//!    operand walk over the inline `[Operand; 2]` payload.
//!
//! Run with:
//!
//! ```text
//! cargo bench -p glyim-mir --bench binary_op_inline
//! ```

use std::hint::black_box;
use std::time::Instant;

use glyim_core::def_id::{CrateId, DefId, LocalDefId};
use glyim_core::primitives::{BinOp, IntTy};
use glyim_core::{IndexVec, Mutability};
use glyim_mir::{
    BasicBlockData, Body, LocalDecl, LocalIdx, MirConst, MirConstKind, Operand, Place, Rvalue,
    SourceInfo, Statement, StatementKind, Terminator, TerminatorKind, validate::validate_body,
};
use glyim_span::Span;
use glyim_test::TyCtxBuilder;
use glyim_type::{Ty, TyKind};

const N: usize = 100_000;

fn int_const(val: i128, ty: Ty) -> MirConst {
    MirConst {
        kind: MirConstKind::Int(val),
        ty,
        span: Span::DUMMY,
    }
}

/// One-block body whose only statement is a binary-op assignment.
fn binary_op_body(i32_ty: Ty) -> Body {
    let mut basic_blocks = IndexVec::new();
    basic_blocks.push(BasicBlockData::new(Terminator {
        kind: TerminatorKind::Return,
        source_info: SourceInfo::new(Span::DUMMY),
    }));
    let mut locals = IndexVec::new();
    locals.push(LocalDecl {
        ty: i32_ty,
        mutability: Mutability::Not,
        source_info: SourceInfo::new(Span::DUMMY),
    });
    locals.push(LocalDecl {
        ty: i32_ty,
        mutability: Mutability::Mut,
        source_info: SourceInfo::new(Span::DUMMY),
    });
    Body {
        owner: DefId::new(CrateId::from_raw(0), LocalDefId::from_raw(0)),
        basic_blocks,
        locals,
        arg_count: 0,
        return_ty: i32_ty,
        span: Span::DUMMY,
        var_debug_info: Vec::new(),
    }
}

fn main() {
    let mut ctx_mut = TyCtxBuilder::new().build_mut();
    let i32_ty = ctx_mut.mk_ty(TyKind::Int(IntTy::I32));
    let ctx = ctx_mut.freeze();

    // 1. smart-constructor construction cost.
    let start = Instant::now();
    let mut acc = 0usize;
    for i in 0..N {
        let rv = Rvalue::binary_op(
            BinOp::Add,
            Operand::Constant(int_const(i as i128, i32_ty)),
            Operand::Copy(Place::new(LocalIdx::from_raw(1))),
        );
        // Consume the payload like a pass would (operand walk).
        if let Rvalue::BinaryOp(op, [l, r]) = rv {
            acc = acc
                .wrapping_add(op as u32 as usize)
                .wrapping_add(usize::from(matches!(l, Operand::Constant(_))))
                .wrapping_add(usize::from(matches!(r, Operand::Copy(_))));
        }
    }
    let construct = start.elapsed();
    println!("construct {N} binary-op rvalues: {construct:?} (acc={acc})");

    // 2. validator operand-walk cost over an N-statement body.
    let mut body = binary_op_body(i32_ty);
    let stmts: Vec<Statement> = (0..N)
        .map(|i| Statement {
            kind: StatementKind::Assign(
                Place::new(LocalIdx::from_raw(0)),
                Rvalue::binary_op(
                    BinOp::Add,
                    Operand::Constant(int_const(i as i128, i32_ty)),
                    Operand::Copy(Place::new(LocalIdx::from_raw(1))),
                ),
            ),
            source_info: SourceInfo::new(Span::DUMMY),
        })
        .collect();
    body.basic_blocks[glyim_mir::BasicBlockIdx::from_raw(0)].statements = stmts;
    let start = Instant::now();
    let verdict = validate_body(&ctx, &body);
    let validate = start.elapsed();
    let well_formed = verdict.is_ok();
    println!("validate {N} binary-op statements: {validate:?} (ok={well_formed})");
    black_box((acc, well_formed));
}
