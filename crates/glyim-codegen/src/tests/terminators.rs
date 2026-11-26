use super::super::*;
use glyim_core::def_id::{CrateId, DefId, LocalDefId};
use glyim_core::primitives::Mutability;
use glyim_span::Span;
use glyim_type::Ty;
use std::sync::Arc;

fn make_body_with_terminator(term: TerminatorKind) -> Body {
    let mut body = Body::dummy(DefId::new(
        CrateId::from_usize(0),
        LocalDefId::from_usize(0),
    ));
    body.basic_blocks[BasicBlockIdx::from_usize(0)].terminator = Terminator {
        kind: term,
        source_info: SourceInfo::new(Span::DUMMY),
    };
    body
}

#[test]
fn emit_call() {
    let mut body = Body::dummy(DefId::new(
        CrateId::from_usize(0),
        LocalDefId::from_usize(0),
    ));
    let dest = LocalIdx::from_usize(1);
    body.locals.push(LocalDecl {
        ty: Ty::ERROR,
        mutability: Mutability::Not,
        source_info: SourceInfo::new(Span::DUMMY),
    });
    body.basic_blocks[BasicBlockIdx::from_usize(0)].terminator = Terminator {
        kind: TerminatorKind::Call {
            func: Operand::Constant(MirConst {
                kind: MirConstKind::Int(0xdead),
                ty: Ty::ERROR,
                span: Span::DUMMY,
            }),
            args: vec![Operand::Constant(MirConst {
                kind: MirConstKind::Int(7),
                ty: Ty::ERROR,
                span: Span::DUMMY,
            })],
            destination: Place::new(dest),
            target: Some(BasicBlockIdx::from_usize(2)),
            cleanup: None,
        },
        source_info: SourceInfo::new(Span::DUMMY),
    };
    let backend = BytecodeBackend::with_ty_ctx(
        std::sync::Arc::new(glyim_type::TyCtxMut::new(glyim_core::Interner::default()).freeze()),
        glyim_core::TargetInfo::default(),
    );
    let result = backend.generate_function(&Arc::new(body)).unwrap();
    assert!(result.contains(&OP_CALL));
}

#[test]
fn emit_switch_int() {
    let mut body = Body::dummy(DefId::new(
        CrateId::from_usize(0),
        LocalDefId::from_usize(0),
    ));
    let targets = SwitchTargets::new(
        Box::new([
            (1u128, BasicBlockIdx::from_usize(3)),
            (2u128, BasicBlockIdx::from_usize(4)),
        ]),
        BasicBlockIdx::from_usize(5),
    );
    body.basic_blocks[BasicBlockIdx::from_usize(0)].terminator = Terminator {
        kind: TerminatorKind::SwitchInt {
            discr: Operand::Constant(MirConst {
                kind: MirConstKind::Int(1),
                ty: Ty::ERROR,
                span: Span::DUMMY,
            }),
            switch_ty: Ty::ERROR,
            targets,
        },
        source_info: SourceInfo::new(Span::DUMMY),
    };
    let backend = BytecodeBackend::with_ty_ctx(
        std::sync::Arc::new(glyim_type::TyCtxMut::new(glyim_core::Interner::default()).freeze()),
        glyim_core::TargetInfo::default(),
    );
    let result = backend.generate_function(&Arc::new(body)).unwrap();
    assert!(result.contains(&OP_SWITCH_INT));
}

#[test]
fn emit_assert() {
    let body = make_body_with_terminator(TerminatorKind::Assert {
        cond: Operand::Constant(MirConst {
            kind: MirConstKind::Bool(true),
            ty: Ty::BOOL,
            span: Span::DUMMY,
        }),
        expected: true,
        target: BasicBlockIdx::from_usize(2),
        cleanup: None,
        msg: glyim_mir::AssertMessage::Overflow(glyim_core::primitives::BinOp::Add),
    });
    let backend = BytecodeBackend::with_ty_ctx(
        std::sync::Arc::new(glyim_type::TyCtxMut::new(glyim_core::Interner::default()).freeze()),
        glyim_core::TargetInfo::default(),
    );
    let result = backend.generate_function(&Arc::new(body)).unwrap();
    assert!(result.contains(&OP_ASSERT));
}

#[test]
fn emit_drop_is_nop() {
    let body = make_body_with_terminator(TerminatorKind::Drop {
        place: Place::new(LocalIdx::from_usize(0)),
        target: BasicBlockIdx::from_usize(1),
        cleanup: None,
    });
    let backend = BytecodeBackend::with_ty_ctx(
        std::sync::Arc::new(glyim_type::TyCtxMut::new(glyim_core::Interner::default()).freeze()),
        glyim_core::TargetInfo::default(),
    );
    let result = backend.generate_function(&Arc::new(body));
    // Should succeed (nop), no error
    assert!(result.is_ok());
}
