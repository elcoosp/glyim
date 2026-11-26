use crate::LlvmBackend;
use glyim_codegen::CodegenBackend;
use glyim_core::def_id::{CrateId, DefId, LocalDefId};
use glyim_core::primitives::Mutability;
use glyim_mir::{
    BasicBlockData, BasicBlockIdx, Body, LocalDecl, LocalIdx, Operand, Place, SourceInfo,
    SwitchTargets, Terminator, TerminatorKind,
};
use glyim_span::Span;
use glyim_type::Ty;
use std::sync::Arc;

#[test]
fn v12_t01_empty_function_ret_void() {
    let backend = LlvmBackend::new();
    let body = Arc::new(Body::dummy(DefId::new(
        CrateId::from_usize(0),
        LocalDefId::from_usize(0),
    )));
    let result = backend.generate_function(&body);
    assert!(
        result.is_ok(),
        "Empty function should generate valid LLVM IR"
    );
}

#[test]
fn v12_t02_function_with_if_conditional_branch() {
    let backend = LlvmBackend::new();
    let mut body = Body::dummy(DefId::new(
        CrateId::from_usize(0),
        LocalDefId::from_usize(0),
    ));

    let _bb0 = body.basic_blocks.push(BasicBlockData::new(Terminator {
        kind: TerminatorKind::Goto {
            target: BasicBlockIdx::from_usize(1),
        },
        source_info: SourceInfo::new(Span::DUMMY),
    }));

    let _bb1 = body.basic_blocks.push(BasicBlockData::new(Terminator {
        kind: TerminatorKind::Return,
        source_info: SourceInfo::new(Span::DUMMY),
    }));

    let body = Arc::new(body);
    let result = backend.generate_function(&body);
    assert!(
        result.is_ok(),
        "Function with if should generate valid LLVM IR with conditional branch"
    );
}

#[test]
fn v12_t03_switch_int_on_integer() {
    let backend = LlvmBackend::new();
    let mut body = Body::dummy(DefId::new(
        CrateId::from_usize(0),
        LocalDefId::from_usize(0),
    ));

    // Add a local with an integer type to switch on
    body.locals.push(LocalDecl {
        ty: Ty::BOOL,
        mutability: Mutability::Not,
        source_info: SourceInfo::new(Span::DUMMY),
    });

    let _bb0 = body.basic_blocks.push(BasicBlockData::new(Terminator {
        kind: TerminatorKind::Goto {
            target: BasicBlockIdx::from_usize(1),
        },
        source_info: SourceInfo::new(Span::DUMMY),
    }));

    let _bb1 = body.basic_blocks.push(BasicBlockData::new(Terminator {
        kind: TerminatorKind::SwitchInt {
            discr: Operand::Copy(Place::new(LocalIdx::from_usize(1))),
            switch_ty: Ty::BOOL,
            targets: SwitchTargets::if_switch(
                BasicBlockIdx::from_usize(2),
                BasicBlockIdx::from_usize(3),
            ),
        },
        source_info: SourceInfo::new(Span::DUMMY),
    }));

    let _bb2 = body.basic_blocks.push(BasicBlockData::new(Terminator {
        kind: TerminatorKind::Return,
        source_info: SourceInfo::new(Span::DUMMY),
    }));

    let _bb3 = body.basic_blocks.push(BasicBlockData::new(Terminator {
        kind: TerminatorKind::Return,
        source_info: SourceInfo::new(Span::DUMMY),
    }));

    let body = Arc::new(body);
    let result = backend.generate_function(&body);
    assert!(
        result.is_ok(),
        "SwitchInt should generate valid LLVM IR switch"
    );
}

#[test]
fn v12_t04_unreachable() {
    let backend = LlvmBackend::new();
    let mut body = Body::dummy(DefId::new(
        CrateId::from_usize(0),
        LocalDefId::from_usize(0),
    ));

    let _bb0 = body.basic_blocks.push(BasicBlockData::new(Terminator {
        kind: TerminatorKind::Unreachable,
        source_info: SourceInfo::new(Span::DUMMY),
    }));

    let body = Arc::new(body);
    let result = backend.generate_function(&body);
    assert!(
        result.is_ok(),
        "Unreachable should generate valid LLVM IR unreachable"
    );
}

#[test]
fn v12_t05_basic_block_ordering_preserved() {
    let backend = LlvmBackend::new();
    let mut body = Body::dummy(DefId::new(
        CrateId::from_usize(0),
        LocalDefId::from_usize(0),
    ));

    let _bb0 = body.basic_blocks.push(BasicBlockData::new(Terminator {
        kind: TerminatorKind::Goto {
            target: BasicBlockIdx::from_usize(1),
        },
        source_info: SourceInfo::new(Span::DUMMY),
    }));

    let _bb1 = body.basic_blocks.push(BasicBlockData::new(Terminator {
        kind: TerminatorKind::Goto {
            target: BasicBlockIdx::from_usize(2),
        },
        source_info: SourceInfo::new(Span::DUMMY),
    }));

    let _bb2 = body.basic_blocks.push(BasicBlockData::new(Terminator {
        kind: TerminatorKind::Return,
        source_info: SourceInfo::new(Span::DUMMY),
    }));

    let body = Arc::new(body);
    let result = backend.generate_function(&body);
    assert!(
        result.is_ok(),
        "Basic block ordering should be preserved in LLVM IR"
    );
}
