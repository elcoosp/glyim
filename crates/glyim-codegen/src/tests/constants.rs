//! Tests for constant emission (S08-T02, S08-T03)

use crate::{BytecodeBackend, CodegenBackend, OP_LOAD_CONST_STR};
use glyim_core::primitives::Mutability;
use glyim_core::{CrateId, FnDefId, Interner, LocalDefId};
use glyim_mir::{
    BasicBlockIdx, Body, LocalDecl, LocalIdx, MirConst, MirConstKind, Operand, Place, Rvalue,
    SourceInfo, Statement, StatementKind, Terminator, TerminatorKind,
};
use glyim_type::{Substitution, TyCtxMut};
use std::sync::Arc;

#[test]
fn string_constant_emitted_to_string_table() {
    let interner = Interner::default();
    let ctx_mut = TyCtxMut::new(interner.clone());

    let mut body = Body::dummy(glyim_core::DefId::new(
        CrateId::from_usize(0),
        LocalDefId::from_usize(0),
    ));

    body.locals.push(LocalDecl {
        ty: ctx_mut.unit_ty(),
        mutability: Mutability::Not,
        source_info: SourceInfo::new(glyim_span::Span::DUMMY),
    });

    let name = interner.intern("test_string");

    let mir_const = MirConst {
        kind: MirConstKind::String(name),
        ty: ctx_mut.unit_ty(),
        span: glyim_span::Span::DUMMY,
    };

    let stmt = Statement {
        kind: StatementKind::Assign(
            Place::new(LocalIdx::from_usize(0)),
            Rvalue::Use(Operand::Constant(mir_const)),
        ),
        source_info: SourceInfo::new(glyim_span::Span::DUMMY),
    };

    body.basic_blocks[BasicBlockIdx::from_usize(0)]
        .statements
        .push(stmt);
    body.basic_blocks[BasicBlockIdx::from_usize(0)].terminator = Terminator {
        kind: TerminatorKind::Return,
        source_info: SourceInfo::new(glyim_span::Span::DUMMY),
    };

    let ctx = ctx_mut.freeze();
    let backend = BytecodeBackend::with_ty_ctx(Arc::new(ctx), glyim_core::TargetInfo::default());
    let result = backend.generate_function(&Arc::new(body));

    assert!(result.is_ok());
    let bc = result.unwrap();
    assert!(!bc.is_empty());
    // A string constant is a *pool reference*: it travels under
    // `OP_LOAD_CONST_STR` + a u32 pool index (never under the integer
    // `OP_LOAD_CONST`), and `generate_module_image` ships the pool the
    // index resolves into.
    assert_eq!(bc[0], OP_LOAD_CONST_STR);
    let idx = u32::from_le_bytes(bc[1..5].try_into().unwrap());
    assert_eq!(idx, 0, "first interned string lands at pool index 0");
}

#[test]
fn function_constant_emitted_to_fn_table() {
    let interner = Interner::default();
    let ctx_mut = TyCtxMut::new(interner.clone());

    let mut body = Body::dummy(glyim_core::DefId::new(
        CrateId::from_usize(0),
        LocalDefId::from_usize(0),
    ));

    body.locals.push(LocalDecl {
        ty: ctx_mut.unit_ty(),
        mutability: Mutability::Not,
        source_info: SourceInfo::new(glyim_span::Span::DUMMY),
    });

    let def_id = FnDefId::from_usize(42);
    let substs = Substitution::empty();

    let mir_const = MirConst {
        kind: MirConstKind::Fn(def_id, substs),
        ty: ctx_mut.unit_ty(),
        span: glyim_span::Span::DUMMY,
    };

    let stmt = Statement {
        kind: StatementKind::Assign(
            Place::new(LocalIdx::from_usize(0)),
            Rvalue::Use(Operand::Constant(mir_const)),
        ),
        source_info: SourceInfo::new(glyim_span::Span::DUMMY),
    };

    body.basic_blocks[BasicBlockIdx::from_usize(0)]
        .statements
        .push(stmt);
    body.basic_blocks[BasicBlockIdx::from_usize(0)].terminator = Terminator {
        kind: TerminatorKind::Return,
        source_info: SourceInfo::new(glyim_span::Span::DUMMY),
    };

    let ctx = ctx_mut.freeze();
    let backend = BytecodeBackend::with_ty_ctx(Arc::new(ctx), glyim_core::TargetInfo::default());
    let result = backend.generate_function(&Arc::new(body));

    assert!(result.is_ok());
    let bc = result.unwrap();
    assert!(!bc.is_empty());
    assert_eq!(bc[0], 0x01);
}
