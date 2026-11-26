//! M2.7 quick win / PF-12 first clause — inline binary-op operands.
//!
//! Plan line pinned (docs/roadmaps/glyim-master-plan.md §4.4 PF-12, restated
//! as M2.7 item 4 "`Box<(Operand,Operand)>` removal"):
//!
//! > **MIR representation diet.** Inline `[Operand;2]` for binary ops (no
//! > `Box`), …
//!
//! The acceptance criterion encoded below: an `Rvalue::BinaryOp` stores its
//! two operands in an **inline `[Operand; 2]`** — the payload is neither a
//! `Box` nor a tuple, construction goes through the
//! [`Rvalue::binary_op`] smart constructor, and no heap allocation is
//! paid per binary op just to hold the pair.

use crate::*;
use glyim_core::def_id::{CrateId, DefId, LocalDefId};
use glyim_core::primitives::{BinOp, IntTy};
use glyim_core::{IndexVec, Mutability};
use glyim_span::Span;
use glyim_test::with_fresh_ty_ctx;
use glyim_type::{Ty, TyCtx, TyCtxMut, TyKind};

use crate::validate::validate_body;

fn int_const(val: i128) -> MirConst {
    MirConst {
        kind: MirConstKind::Int(val),
        ty: Ty::BOOL,
        span: Span::DUMMY,
    }
}

fn copy(local: u32) -> Operand {
    Operand::Copy(Place::new(LocalIdx::from_usize(local as usize)))
}

/// Minimal one-block `return` body with `local_ty` locals, mirroring the
/// `validate.rs` test helper shape.
fn one_block_body(local_ty: Ty) -> Body {
    let mut basic_blocks = IndexVec::new();
    basic_blocks.push(BasicBlockData::new(Terminator {
        kind: TerminatorKind::Return,
        source_info: SourceInfo::new(Span::DUMMY),
    }));
    let mut locals = IndexVec::new();
    locals.push(LocalDecl {
        ty: local_ty,
        mutability: Mutability::Not,
        source_info: SourceInfo::new(Span::DUMMY),
    });
    Body {
        owner: DefId::new(CrateId::from_usize(0), LocalDefId::from_usize(0)),
        basic_blocks,
        locals,
        arg_count: 0,
        return_ty: local_ty,
        span: Span::DUMMY,
        var_debug_info: Vec::new(),
    }
}

#[test]
fn binary_op_stores_operands_inline_via_smart_constructor() {
    let rv = Rvalue::binary_op(BinOp::Add, copy(0), copy(1));
    // The array-literal destructure only compiles against the inline
    // `[Operand; 2]` payload (PF-12): a `Box<(Operand, Operand)>` payload
    // would fail this pattern.
    let Rvalue::BinaryOp(BinOp::Add, [l, r]) = rv else {
        panic!("BinaryOp must carry an inline [Operand; 2] payload");
    };
    assert!(matches!(l, Operand::Copy(p) if p.local.index() == 0));
    assert!(matches!(r, Operand::Copy(p) if p.local.index() == 1));
}

#[test]
fn binary_op_moves_constants_without_boxing() {
    let rv = Rvalue::binary_op(
        BinOp::Mul,
        Operand::Constant(int_const(2)),
        Operand::Constant(int_const(3)),
    );
    let Rvalue::BinaryOp(BinOp::Mul, [Operand::Constant(l), Operand::Constant(r)]) = rv else {
        panic!("expected inline binary-op operands");
    };
    assert!(matches!(l.kind, MirConstKind::Int(2)));
    assert!(matches!(r.kind, MirConstKind::Int(3)));
}

#[test]
fn binary_op_inline_payload_round_trips_through_a_body() {
    let (ctx, body): (TyCtx, Body) = with_fresh_ty_ctx(|c: &mut TyCtxMut| {
        let ty = c.mk_ty(TyKind::Int(IntTy::I32));
        let mut body = one_block_body(ty);
        let assign = Statement {
            kind: StatementKind::Assign(
                Place::new(LocalIdx::from_usize(0)),
                Rvalue::binary_op(
                    BinOp::Add,
                    Operand::Constant(int_const(1)),
                    Operand::Constant(int_const(2)),
                ),
            ),
            source_info: SourceInfo::new(Span::DUMMY),
        };
        body.basic_blocks[BasicBlockIdx::from_usize(0)]
            .statements
            .push(assign);
        body
    });
    // The inline payload is a plain well-formed rvalue: the M5.5 validator
    // accepts a body whose only rvalue is a `BinaryOp`.
    assert!(validate_body(&ctx, &body).is_ok());
    let stmt = &body.basic_blocks[BasicBlockIdx::from_usize(0)].statements[0];
    let StatementKind::Assign(_, Rvalue::BinaryOp(_, ops)) = &stmt.kind else {
        panic!("expected the assign to carry the inline BinaryOp");
    };
    // `ops` is addressable as a slice pair — no indirection, no tuple.
    assert_eq!(ops.len(), 2);
    assert!(matches!(ops[0], Operand::Constant(_)));
    assert!(matches!(ops[1], Operand::Constant(_)));
}
