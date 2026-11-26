//! Binary-op operand walkthrough: the inline `[Operand; 2]` payload.
//!
//! Plan PF-12 (docs/roadmaps/glyim-master-plan.md §4.4) restated as the M2.7
//! quick win "`Box<(Operand,Operand)>` removal" stores a `BinaryOp`'s two
//! operands **inline** in the `Rvalue::BinaryOp` variant — no heap
//! allocation per binary op just to hold the pair. Producers construct the
//! rvalue through the [`Rvalue::binary_op`] smart constructor (which keeps
//! the payload shape replaceable), consumers index the pair like a slice.
//!
//! This walkthrough builds the shapes a pass or backend author actually
//! makes: a binary-op rvalue from a smart constructor, a statement that
//! assigns it, a body round-trip through the M5.5 validator, and the
//! operand access the validator/interpreter/backends perform. Run with:
//!
//! ```text
//! cargo run -p glyim-mir --example binary_operands_inline
//! ```

use glyim_core::def_id::{CrateId, DefId, LocalDefId};
use glyim_core::primitives::{BinOp, IntTy};
use glyim_core::{IndexVec, Mutability};
use glyim_mir::{
    BasicBlockData, LocalDecl, LocalIdx, MirConst, MirConstKind, Operand, Place, Rvalue,
    SourceInfo, Statement, StatementKind, Terminator, TerminatorKind, validate::validate_body,
};
use glyim_span::Span;
use glyim_test::TyCtxBuilder;
use glyim_type::TyKind;

fn main() {
    // Interned `i32` for the locals involved.
    let mut ctx_mut = TyCtxBuilder::new().build_mut();
    let i32_ty = ctx_mut.mk_ty(TyKind::Int(IntTy::I32));
    let ctx = ctx_mut.freeze();

    // The smart constructor: two operands in, one inline-payload rvalue out.
    let lhs = Operand::Constant(MirConst {
        kind: MirConstKind::Int(2),
        ty: i32_ty,
        span: Span::DUMMY,
    });
    let rhs = Operand::Copy(Place::new(LocalIdx::from_usize(1)));
    let rv = Rvalue::binary_op(BinOp::Add, lhs, rhs);
    println!("rvalue = {rv:?}");

    // The payload is an inline array: index it, don't deref a box.
    let Rvalue::BinaryOp(op, operands) = &rv else {
        panic!("expected BinaryOp");
    };
    println!("op = {op:?}, operands.len = {}", operands.len());

    // A one-block body: `_0 = 2 + _1; return`. The statement carrying the
    // inline payload is pushed like any other.
    let mut basic_blocks = IndexVec::new();
    let mut entry = BasicBlockData::new(Terminator {
        kind: TerminatorKind::Return,
        source_info: SourceInfo::new(Span::DUMMY),
    });
    entry.statements.push(Statement {
        kind: StatementKind::Assign(Place::new(LocalIdx::from_usize(0)), rv),
        source_info: SourceInfo::new(Span::DUMMY),
    });
    basic_blocks.push(entry);
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
    let body = glyim_mir::Body {
        owner: DefId::new(CrateId::from_usize(0), LocalDefId::from_usize(0)),
        basic_blocks,
        locals,
        arg_count: 0,
        return_ty: i32_ty,
        span: Span::DUMMY,
        var_debug_info: Vec::new(),
    };

    // `validate_body` (M5.5) accepts the inline payload like any rvalue.
    match validate_body(&ctx, &body) {
        Ok(()) => println!("validator: body with inline BinaryOp is well-formed"),
        Err(violations) => println!("validator: {violations:?}"),
    }
}
