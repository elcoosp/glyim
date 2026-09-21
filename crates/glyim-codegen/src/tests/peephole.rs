//! Tests for the bytecode peephole optimization pass (§2.4).
//!
//! At `OptLevel::O0` the backend emits lowering output verbatim. At `O1`+ it
//! runs the peephole pass: integer constant folding (`LOAD_CONST a; LOAD_CONST
//! b; BINOP` -> `LOAD_CONST (a OP b)`) and double-negation cancellation
//! (`OP_NEG; OP_NEG` -> nothing). Both rules are semantics-preserving.

use super::super::*;
use glyim_core::def_id::{CrateId, DefId, LocalDefId};
use glyim_core::primitives::{BinOp, Mutability};
use glyim_span::Span;
use glyim_type::Ty;
use std::sync::Arc;

fn ty_ctx() -> Arc<glyim_type::TyCtx> {
    Arc::new(glyim_type::TyCtxMut::new(glyim_core::Interner::default()).freeze())
}

/// A body with a single block: `_1 = <const a> <binop> <const b>; trap`.
fn fold_body(op: BinOp, a: i128, b: i128) -> Body {
    let mut body = Body::dummy(DefId::new(CrateId::from_raw(0), LocalDefId::from_raw(0)));
    body.locals.push(LocalDecl {
        ty: Ty::ERROR,
        mutability: Mutability::Not,
        source_info: SourceInfo::new(Span::DUMMY),
    });
    let stmt = Statement {
        kind: StatementKind::Assign(
            Place::new(LocalIdx::from_raw(1)),
            Rvalue::BinaryOp(
                op,
                Box::new((
                    Operand::Constant(MirConst {
                        kind: MirConstKind::Int(a),
                        ty: Ty::ERROR,
                        span: Span::DUMMY,
                    }),
                    Operand::Constant(MirConst {
                        kind: MirConstKind::Int(b),
                        ty: Ty::ERROR,
                        span: Span::DUMMY,
                    }),
                )),
            ),
        ),
        source_info: SourceInfo::new(Span::DUMMY),
    };
    body.basic_blocks[BasicBlockIdx::from_raw(0)]
        .statements
        .push(stmt);
    body
}

#[test]
fn o0_keeps_constants_unfolded() {
    let body = fold_body(BinOp::Add, 3, 5);
    let backend = BytecodeBackend::with_ty_ctx(ty_ctx(), glyim_core::TargetInfo::default())
        .with_opt_level(OptLevel::O0);
    let bc = backend.generate_function(&Arc::new(body)).unwrap();
    // Expect two LOAD_CONST (3, 5) and OP_ADD, i.e. NOT folded to a single const.
    assert_eq!(bc[0], OP_LOAD_CONST);
    let three = i64::from_le_bytes(bc[1..9].try_into().unwrap());
    assert_eq!(three, 3);
    assert_eq!(bc[9], OP_LOAD_CONST);
    let five = i64::from_le_bytes(bc[10..18].try_into().unwrap());
    assert_eq!(five, 5);
    assert_eq!(bc[18], OP_ADD);
}

#[test]
fn o1_folds_integer_constants() {
    let body = fold_body(BinOp::Add, 3, 5);
    let backend = BytecodeBackend::with_ty_ctx(ty_ctx(), glyim_core::TargetInfo::default())
        .with_opt_level(OptLevel::O1);
    let bc = backend.generate_function(&Arc::new(body)).unwrap();
    // Expect a single folded LOAD_CONST (8) followed by STORE_LOCAL, no OP_ADD.
    assert_eq!(bc[0], OP_LOAD_CONST);
    let folded = i64::from_le_bytes(bc[1..9].try_into().unwrap());
    assert_eq!(folded, 8);
    assert_eq!(
        bc[9], OP_STORE_LOCAL,
        "no OP_ADD should remain after folding"
    );
}

/// At O1 the peephole pass runs but must preserve semantics: the executable
/// result of an O1-compiled function equals that of the O0 (verbatim) output.
/// This is the real contract of §2.4 — the pass may not change meaning.
#[test]
fn o1_preserves_semantics_vs_o0() {
    use glyim_bytecode_vm::{Function, Module, Value, Vm};

    let body = fold_body(BinOp::Add, 3, 5);
    let ctx = ty_ctx();
    let ti = glyim_core::TargetInfo::default();
    let o0 = BytecodeBackend::with_ty_ctx(ctx.clone(), ti.clone())
        .with_opt_level(OptLevel::O0)
        .generate_function(&Arc::new(body.clone()))
        .unwrap();
    let o1 = BytecodeBackend::with_ty_ctx(ctx, ti)
        .with_opt_level(OptLevel::O1)
        .generate_function(&Arc::new(body))
        .unwrap();

    // Both must yield 8 (3 + 5) in local 1 after execution.
    let run = |bytes: Vec<u8>| -> Option<Value> {
        let module = Module::new(vec![Function::new(bytes, 2, 0)], 0);
        let mut vm = Vm::new();
        let _ = vm.run_module(&module);
        vm.local(1)
    };
    assert_eq!(run(o0), Some(Value::Int(8)), "O0 must compute 3+5=8");
    assert_eq!(run(o1), Some(Value::Int(8)), "O1 fold must preserve 3+5=8");
}
