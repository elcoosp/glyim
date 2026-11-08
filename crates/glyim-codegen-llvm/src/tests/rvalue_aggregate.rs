use glyim_codegen::CodegenBackend;
use glyim_core::arena::IndexVec;
use glyim_core::def_id::{CrateId, DefId, LocalDefId};
use glyim_core::primitives::*;
use glyim_mir::*;
use glyim_type::Ty;
use inkwell::targets::{InitializationConfig, Target};
use std::sync::Arc;

use crate::LlvmBackend;

fn make_simple_body_with_rvalue(rvalue: Rvalue, result_ty: Ty) -> Body {
    let mut locals = IndexVec::<LocalIdx, LocalDecl>::new();
    locals.push(LocalDecl {
        ty: Ty::UNIT,
        mutability: Mutability::Not,
        source_info: SourceInfo::new(glyim_span::Span::DUMMY),
    });
    locals.push(LocalDecl {
        ty: result_ty,
        mutability: Mutability::Mut,
        source_info: SourceInfo::new(glyim_span::Span::DUMMY),
    });

    let stmt = Statement {
        kind: StatementKind::Assign(Place::new(LocalIdx::from_raw(1)), rvalue),
        source_info: SourceInfo::new(glyim_span::Span::DUMMY),
    };

    let term = Terminator {
        kind: TerminatorKind::Return,
        source_info: SourceInfo::new(glyim_span::Span::DUMMY),
    };

    let bb0 = BasicBlockData {
        statements: vec![stmt],
        terminator: term,
        is_cleanup: false,
    };

    let mut bbs = IndexVec::<BasicBlockIdx, BasicBlockData>::new();
    bbs.push(bb0);

    Body {
        owner: DefId::new(CrateId::from_raw(0), LocalDefId::from_raw(0)),
        basic_blocks: bbs,
        locals,
        arg_count: 0,
        return_ty: Ty::UNIT,
        span: glyim_span::Span::DUMMY,
        var_debug_info: vec![],
    }
}

fn make_ref_body() -> Body {
    let mut locals = IndexVec::<LocalIdx, LocalDecl>::new();
    locals.push(LocalDecl {
        ty: Ty::UNIT,
        mutability: Mutability::Not,
        source_info: SourceInfo::new(glyim_span::Span::DUMMY),
    });
    locals.push(LocalDecl {
        ty: Ty::BOOL,
        mutability: Mutability::Not,
        source_info: SourceInfo::new(glyim_span::Span::DUMMY),
    });
    locals.push(LocalDecl {
        ty: Ty::BOOL,
        mutability: Mutability::Mut,
        source_info: SourceInfo::new(glyim_span::Span::DUMMY),
    });

    let rvalue = Rvalue::Ref(Place::new(LocalIdx::from_raw(1)), BorrowKind::Shared);

    let stmt = Statement {
        kind: StatementKind::Assign(Place::new(LocalIdx::from_raw(2)), rvalue),
        source_info: SourceInfo::new(glyim_span::Span::DUMMY),
    };

    let term = Terminator {
        kind: TerminatorKind::Return,
        source_info: SourceInfo::new(glyim_span::Span::DUMMY),
    };

    let bb0 = BasicBlockData {
        statements: vec![stmt],
        terminator: term,
        is_cleanup: false,
    };

    let mut bbs = IndexVec::<BasicBlockIdx, BasicBlockData>::new();
    bbs.push(bb0);

    Body {
        owner: DefId::new(CrateId::from_raw(0), LocalDefId::from_raw(0)),
        basic_blocks: bbs,
        locals,
        arg_count: 0,
        return_ty: Ty::UNIT,
        span: glyim_span::Span::DUMMY,
        var_debug_info: vec![],
    }
}

#[test]
fn v13_t01_struct_literal_aggregate() {
    Target::initialize_all(&InitializationConfig::default());
    let backend = LlvmBackend::new();
    let operands = vec![
        Operand::Constant(MirConst {
            kind: MirConstKind::Int(1),
            ty: Ty::BOOL,
            span: glyim_span::Span::DUMMY,
        }),
        Operand::Constant(MirConst {
            kind: MirConstKind::Int(2),
            ty: Ty::BOOL,
            span: glyim_span::Span::DUMMY,
        }),
    ];
    let rvalue = Rvalue::Aggregate(AggregateKind::Tuple, operands);
    let tuple_ty = Ty::UNIT;
    let body = make_simple_body_with_rvalue(rvalue, tuple_ty);
    let result = CodegenBackend::generate_function(&backend, &Arc::new(body));
    assert!(
        result.is_ok(),
        "struct/tuple aggregate should lower successfully: {result:?}"
    );
}

#[test]
fn v13_t02_array_literal_aggregate() {
    Target::initialize_all(&InitializationConfig::default());
    let backend = LlvmBackend::new();
    let operands = vec![
        Operand::Constant(MirConst {
            kind: MirConstKind::Int(10),
            ty: Ty::BOOL,
            span: glyim_span::Span::DUMMY,
        }),
        Operand::Constant(MirConst {
            kind: MirConstKind::Int(20),
            ty: Ty::BOOL,
            span: glyim_span::Span::DUMMY,
        }),
        Operand::Constant(MirConst {
            kind: MirConstKind::Int(30),
            ty: Ty::BOOL,
            span: glyim_span::Span::DUMMY,
        }),
    ];
    let rvalue = Rvalue::Aggregate(AggregateKind::Array(Ty::BOOL), operands);
    let array_ty = Ty::UNIT;
    let body = make_simple_body_with_rvalue(rvalue, array_ty);
    let result = CodegenBackend::generate_function(&backend, &Arc::new(body));
    assert!(
        result.is_ok(),
        "array aggregate should lower successfully: {result:?}"
    );
}

#[test]
fn v13_t03_ref_local_alloca() {
    Target::initialize_all(&InitializationConfig::default());
    let backend = LlvmBackend::new();
    let body = make_ref_body();
    let result = CodegenBackend::generate_function(&backend, &Arc::new(body));
    assert!(
        result.is_ok(),
        "ref local should lower successfully: {result:?}"
    );
}

#[test]
fn v13_t04_discriminant_enum() {
    Target::initialize_all(&InitializationConfig::default());
    let backend = LlvmBackend::new();
    let discr_place = Place::new(LocalIdx::from_raw(1));
    let rvalue = Rvalue::Discriminant(discr_place);
    let mut locals = IndexVec::<LocalIdx, LocalDecl>::new();
    locals.push(LocalDecl {
        ty: Ty::UNIT,
        mutability: Mutability::Not,
        source_info: SourceInfo::new(glyim_span::Span::DUMMY),
    });
    locals.push(LocalDecl {
        ty: Ty::BOOL,
        mutability: Mutability::Not,
        source_info: SourceInfo::new(glyim_span::Span::DUMMY),
    });
    locals.push(LocalDecl {
        ty: Ty::BOOL,
        mutability: Mutability::Mut,
        source_info: SourceInfo::new(glyim_span::Span::DUMMY),
    });

    let stmt = Statement {
        kind: StatementKind::Assign(Place::new(LocalIdx::from_raw(2)), rvalue),
        source_info: SourceInfo::new(glyim_span::Span::DUMMY),
    };
    let term = Terminator {
        kind: TerminatorKind::Return,
        source_info: SourceInfo::new(glyim_span::Span::DUMMY),
    };
    let bb0 = BasicBlockData {
        statements: vec![stmt],
        terminator: term,
        is_cleanup: false,
    };
    let mut bbs = IndexVec::<BasicBlockIdx, BasicBlockData>::new();
    bbs.push(bb0);

    let body = Body {
        owner: DefId::new(CrateId::from_raw(0), LocalDefId::from_raw(0)),
        basic_blocks: bbs,
        locals,
        arg_count: 0,
        return_ty: Ty::UNIT,
        span: glyim_span::Span::DUMMY,
        var_debug_info: vec![],
    };

    let result = CodegenBackend::generate_function(&backend, &Arc::new(body));
    assert!(
        result.is_ok(),
        "discriminant should lower successfully: {result:?}"
    );
}

#[test]
fn v13_t05_len_array() {
    Target::initialize_all(&InitializationConfig::default());

    // Import from correct modules
    use glyim_core::UintTy;
    use glyim_test::with_fresh_ty_ctx;
    use glyim_type::{Const, ConstKind}; // ← Added Const imports

    let (ctx, body) = with_fresh_ty_ctx(|ctx_mut| {
        // 1. Create element type: u64
        let elem_ty = ctx_mut.mk_ty(glyim_type::TyKind::Uint(UintTy::U64));

        // 2. Create the length constant: Const { kind: Uint(4), ty: usize }
        // Note: Construct Const directly; no mk_const method exists
        let usize_ty = ctx_mut.mk_ty(glyim_type::TyKind::Uint(UintTy::U64));
        let len_const = Const {
            kind: ConstKind::Uint(4),
            ty: usize_ty,
        };

        // 3. Create Array type: [u64; 4]
        let array_ty = ctx_mut.mk_ty(glyim_type::TyKind::Array(elem_ty, len_const));

        // 4. Result type for Len (usize/u64)
        let result_ty = ctx_mut.mk_ty(glyim_type::TyKind::Uint(UintTy::U64));

        let mut locals = IndexVec::<LocalIdx, LocalDecl>::new();

        // Local 0: Return place
        locals.push(LocalDecl {
            ty: Ty::UNIT,
            mutability: Mutability::Not,
            source_info: SourceInfo::new(glyim_span::Span::DUMMY),
        });

        // Local 1: The array variable [u64; 4]
        locals.push(LocalDecl {
            ty: array_ty,
            mutability: Mutability::Not,
            source_info: SourceInfo::new(glyim_span::Span::DUMMY),
        });

        // Local 2: Result of Len (usize)
        locals.push(LocalDecl {
            ty: result_ty,
            mutability: Mutability::Mut,
            source_info: SourceInfo::new(glyim_span::Span::DUMMY),
        });

        // Rvalue: Len of local 1 (the array)
        let rvalue = Rvalue::Len(Place::new(LocalIdx::from_raw(1)));
        let stmt = Statement {
            kind: StatementKind::Assign(Place::new(LocalIdx::from_raw(2)), rvalue),
            source_info: SourceInfo::new(glyim_span::Span::DUMMY),
        };

        let term = Terminator {
            kind: TerminatorKind::Return,
            source_info: SourceInfo::new(glyim_span::Span::DUMMY),
        };

        let bb0 = BasicBlockData {
            statements: vec![stmt],
            terminator: term,
            is_cleanup: false,
        };

        let mut bbs = IndexVec::<BasicBlockIdx, BasicBlockData>::new();
        bbs.push(bb0);

        Body {
            owner: DefId::new(CrateId::from_raw(0), LocalDefId::from_raw(0)),
            basic_blocks: bbs,
            locals,
            arg_count: 0,
            return_ty: Ty::UNIT,
            span: glyim_span::Span::DUMMY,
            var_debug_info: vec![],
        }
    });

    // Backend must use the context that created our types
    let backend = LlvmBackend::new().with_ty_ctx(ctx);

    let result = CodegenBackend::generate_function(&backend, &Arc::new(body));
    assert!(result.is_ok(), "len should lower successfully: {result:?}");
}
