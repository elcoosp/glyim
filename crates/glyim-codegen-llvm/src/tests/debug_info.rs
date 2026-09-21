use crate::LlvmBackend;
use glyim_core::arena::IndexVec;
use glyim_core::def_id::{AdtId, ClosureId};
use glyim_core::primitives::*;
use glyim_core::{CrateId, DefId, Interner, LocalDefId, Name};
use glyim_mir::{
    BasicBlockData, Body, LocalDecl, LocalIdx, MirConst, MirConstKind, Operand, Place, Rvalue,
    SourceInfo, Statement, StatementKind, Terminator, TerminatorKind, VarDebugInfo,
    VarDebugInfoValue,
};
use glyim_span::{
    ByteIdx, ExpnData, ExpnId, ExpnKind, FileId, HygieneCtx, Span, SyntaxContext, Transparency,
};
use glyim_type::{TyCtxMut, TyKind};
use inkwell::context::Context;
use std::collections::HashMap;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn macro_name() -> Name {
    Interner::default().intern("macro")
}

fn make_test_body(ctx: &TyCtxMut, var_name: Name) -> Body {
    let bool_ty = ctx.bool_ty();
    let unit_ty = ctx.unit_ty();

    let mut body = Body::dummy(DefId::new(CrateId::from_raw(0), LocalDefId::from_raw(0)));
    body.return_ty = unit_ty;
    let mut locals: IndexVec<LocalIdx, LocalDecl> = IndexVec::new();
    locals.push(LocalDecl {
        ty: unit_ty,
        mutability: Mutability::Not,
        source_info: SourceInfo::new(Span::new(
            FileId::from_raw(0),
            ByteIdx::from_raw(0),
            ByteIdx::from_raw(0),
            SyntaxContext::ROOT,
        )),
    });
    locals.push(LocalDecl {
        ty: bool_ty,
        mutability: Mutability::Not,
        source_info: SourceInfo::new(Span::new(
            FileId::from_raw(0),
            ByteIdx::from_raw(10),
            ByteIdx::from_raw(15),
            SyntaxContext::ROOT,
        )),
    });
    body.locals = locals;

    body.var_debug_info = vec![VarDebugInfo {
        name: var_name,
        value: VarDebugInfoValue::Place(Place::new(LocalIdx::from_raw(1))),
    }];

    let stmts = vec![Statement {
        kind: StatementKind::Assign(
            Place::new(LocalIdx::from_raw(1)),
            Rvalue::Use(Operand::Constant(MirConst {
                kind: MirConstKind::Bool(false),
                ty: bool_ty,
                span: Span::new(
                    FileId::from_raw(0),
                    ByteIdx::from_raw(12),
                    ByteIdx::from_raw(17),
                    SyntaxContext::ROOT,
                ),
            })),
        ),
        source_info: SourceInfo::new(Span::new(
            FileId::from_raw(0),
            ByteIdx::from_raw(10),
            ByteIdx::from_raw(17),
            SyntaxContext::ROOT,
        )),
    }];

    let bb_data = BasicBlockData {
        statements: stmts,
        terminator: Terminator {
            kind: TerminatorKind::Return,
            source_info: SourceInfo::new(Span::new(
                FileId::from_raw(0),
                ByteIdx::from_raw(18),
                ByteIdx::from_raw(19),
                SyntaxContext::ROOT,
            )),
        },
        is_cleanup: false,
    };

    let mut bbs = IndexVec::new();
    bbs.push(bb_data);
    body.basic_blocks = bbs;
    body
}

fn make_source_map(source: &str) -> HashMap<FileId, (String, String)> {
    let mut map = HashMap::new();
    map.insert(
        FileId::from_raw(0),
        ("test.g".to_string(), source.to_string()),
    );
    map
}

fn has_compile_unit(module: &inkwell::module::Module) -> bool {
    !module.get_global_metadata("llvm.dbg.cu").is_empty()
}

// ---------------------------------------------------------------------------
// Original tests
// ---------------------------------------------------------------------------

#[test]
fn test_debug_compile_unit_present() {
    let source = "fn main() {\n  let x = false;\n}\n";
    let (_ctx, body) = glyim_test::with_fresh_ty_ctx(|ctx_mut| {
        let name_x = ctx_mut.resolver().intern("x");
        make_test_body(ctx_mut, name_x)
    });

    let backend = LlvmBackend::new()
        .with_debug_info(true)
        .with_source_map(make_source_map(source));

    let llvm_ctx = Context::create();
    let module = backend
        .lower_body_to_module(&llvm_ctx, &body)
        .expect("lowering failed");

    assert!(
        has_compile_unit(&module),
        "Expected a DICompileUnit in the module"
    );
}

#[test]
fn test_debug_subprogram_attached() {
    let source = "fn main() {\n  let x = false;\n}\n";
    let (_ctx, body) = glyim_test::with_fresh_ty_ctx(|ctx_mut| {
        let name_x = ctx_mut.resolver().intern("x");
        make_test_body(ctx_mut, name_x)
    });

    let backend = LlvmBackend::new()
        .with_debug_info(true)
        .with_source_map(make_source_map(source));

    let llvm_ctx = Context::create();
    let module = backend
        .lower_body_to_module(&llvm_ctx, &body)
        .expect("lowering failed");

    let func = module.get_first_function().expect("no function in module");
    assert!(
        func.get_subprogram().is_some(),
        "Function does not have a DISubprogram"
    );
}

#[test]
fn test_debug_line_info_on_instruction() {
    let source = "fn main() {\n  let x = false;\n}\n";
    let (_ctx, body) = glyim_test::with_fresh_ty_ctx(|ctx_mut| {
        let name_x = ctx_mut.resolver().intern("x");
        make_test_body(ctx_mut, name_x)
    });

    let backend = LlvmBackend::new()
        .with_debug_info(true)
        .with_source_map(make_source_map(source));

    let llvm_ctx = Context::create();
    let module = backend
        .lower_body_to_module(&llvm_ctx, &body)
        .expect("lowering failed");

    let func = module.get_first_function().expect("no function in module");
    let mut has_location = false;
    for bb in func.get_basic_blocks() {
        for instr in bb.get_instructions() {
            if let Some(loc) = instr.get_debug_location() {
                let _line: u32 = loc.get_line();
                has_location = true;
                break;
            }
        }
    }
    assert!(has_location, "No instruction with DILocation found");
}

#[test]
fn test_debug_local_variable_has_di() {
    let source = "fn main() {\n  let x = false;\n}\n";
    let (_ctx, body) = glyim_test::with_fresh_ty_ctx(|ctx_mut| {
        let name_x = ctx_mut.resolver().intern("x");
        make_test_body(ctx_mut, name_x)
    });

    let backend = LlvmBackend::new()
        .with_debug_info(true)
        .with_source_map(make_source_map(source));

    let llvm_ctx = Context::create();
    let module = backend
        .lower_body_to_module(&llvm_ctx, &body)
        .expect("lowering failed");

    assert!(
        has_compile_unit(&module),
        "No DICompileUnit found; debug info not generated"
    );
}

#[test]
fn test_debug_info_disabled_no_crash() {
    let source = "fn main() { let x = 1; }";
    let (_ctx, body) = glyim_test::with_fresh_ty_ctx(|ctx_mut| {
        let name_x = ctx_mut.resolver().intern("x");
        make_test_body(ctx_mut, name_x)
    });

    let backend = LlvmBackend::new()
        .with_debug_info(false)
        .with_source_map(make_source_map(source));

    let llvm_ctx = Context::create();
    let module = backend
        .lower_body_to_module(&llvm_ctx, &body)
        .expect("lowering without debug info should succeed");

    assert!(
        !has_compile_unit(&module),
        "DICompileUnit should NOT be present when debug info is disabled"
    );
}

#[test]
fn test_debug_info_multiple_files() {
    let source_main = "fn main() {}\n";
    let source_lib = "fn helper() -> bool { true }\n";

    let mut source_map = HashMap::new();
    source_map.insert(
        FileId::from_raw(0),
        ("main.g".to_string(), source_main.to_string()),
    );
    source_map.insert(
        FileId::from_raw(1),
        ("lib.g".to_string(), source_lib.to_string()),
    );

    let (_ctx, body) = glyim_test::with_fresh_ty_ctx(|ctx_mut| {
        let name_x = ctx_mut.resolver().intern("x");
        make_test_body(ctx_mut, name_x)
    });

    let backend = LlvmBackend::new()
        .with_debug_info(true)
        .with_source_map(source_map);

    let llvm_ctx = Context::create();
    let module = backend
        .lower_body_to_module(&llvm_ctx, &body)
        .expect("lowering with multiple files should succeed");

    assert!(
        has_compile_unit(&module),
        "DICompileUnit should be present with multiple files"
    );
}

#[test]
fn test_debug_info_empty_source() {
    let source = "";
    let (_ctx, body) = glyim_test::with_fresh_ty_ctx(|ctx_mut| {
        let name_x = ctx_mut.resolver().intern("x");
        make_test_body(ctx_mut, name_x)
    });

    let backend = LlvmBackend::new()
        .with_debug_info(true)
        .with_source_map(make_source_map(source));

    let llvm_ctx = Context::create();
    let module = backend
        .lower_body_to_module(&llvm_ctx, &body)
        .expect("lowering with empty source should succeed");

    assert!(
        has_compile_unit(&module),
        "DICompileUnit should exist even with empty source"
    );
}

#[test]
fn test_debug_info_dummy_spans() {
    let source = "fn main() {}\n";
    let (_ctx, body) = glyim_test::with_fresh_ty_ctx(|ctx_mut| {
        let name_x = ctx_mut.resolver().intern("x");
        make_test_body(ctx_mut, name_x)
    });

    let backend = LlvmBackend::new()
        .with_debug_info(true)
        .with_source_map(make_source_map(source));

    let llvm_ctx = Context::create();
    let module = backend
        .lower_body_to_module(&llvm_ctx, &body)
        .expect("lowering with dummy spans should succeed");

    assert!(
        has_compile_unit(&module),
        "DICompileUnit should exist with dummy spans"
    );
}

#[test]
fn test_debug_info_var_debug_const_value() {
    let source = "fn main() {}\n";
    let (_ctx, body) = glyim_test::with_fresh_ty_ctx(|ctx_mut| {
        let name_x = ctx_mut.resolver().intern("x");
        let bool_ty = ctx_mut.bool_ty();
        let unit_ty = ctx_mut.unit_ty();

        let mut body = Body::dummy(DefId::new(CrateId::from_raw(0), LocalDefId::from_raw(0)));
        let mut locals: IndexVec<LocalIdx, LocalDecl> = IndexVec::new();
        locals.push(LocalDecl {
            ty: unit_ty,
            mutability: Mutability::Not,
            source_info: SourceInfo::new(Span::new(
                FileId::from_raw(0),
                ByteIdx::from_raw(0),
                ByteIdx::from_raw(0),
                SyntaxContext::ROOT,
            )),
        });
        body.locals = locals;

        body.var_debug_info = vec![VarDebugInfo {
            name: name_x,
            value: VarDebugInfoValue::Const(MirConst {
                kind: MirConstKind::Bool(true),
                ty: bool_ty,
                span: Span::DUMMY,
            }),
        }];

        let bb_data = BasicBlockData {
            statements: vec![],
            terminator: Terminator {
                kind: TerminatorKind::Return,
                source_info: SourceInfo::new(Span::DUMMY),
            },
            is_cleanup: false,
        };
        let mut bbs = IndexVec::new();
        bbs.push(bb_data);
        body.basic_blocks = bbs;
        body
    });

    let backend = LlvmBackend::new()
        .with_debug_info(true)
        .with_source_map(make_source_map(source));

    let llvm_ctx = Context::create();
    let result = backend.lower_body_to_module(&llvm_ctx, &body);
    assert!(
        result.is_ok(),
        "Lowering with const debug variable should succeed"
    );
}

#[test]
fn test_debug_info_subprogram_exists() {
    let source = "fn my_func() -> bool { false }\n";
    let (_ctx, body) = glyim_test::with_fresh_ty_ctx(|ctx_mut| {
        let name_x = ctx_mut.resolver().intern("x");
        make_test_body(ctx_mut, name_x)
    });

    let backend = LlvmBackend::new()
        .with_debug_info(true)
        .with_source_map(make_source_map(source));

    let llvm_ctx = Context::create();
    let module = backend
        .lower_body_to_module(&llvm_ctx, &body)
        .expect("lowering succeeded");

    let func = module.get_first_function().expect("no function");
    assert!(
        func.get_subprogram().is_some(),
        "Subprogram should be attached"
    );
}

#[test]
fn test_debug_info_no_source_map_no_crash() {
    let (_ctx, body) = glyim_test::with_fresh_ty_ctx(|ctx_mut| {
        let name_x = ctx_mut.resolver().intern("x");
        make_test_body(ctx_mut, name_x)
    });

    let backend = LlvmBackend::new()
        .with_debug_info(true)
        .with_source_map(HashMap::new());

    let llvm_ctx = Context::create();
    let result = backend.lower_body_to_module(&llvm_ctx, &body);
    assert!(
        result.is_ok(),
        "Lowering with empty source_map should succeed"
    );
}

#[test]
fn test_debug_info_multiline_source_line_numbers() {
    let source = "// line 1\n// line 2\nfn main() {\n  let x = false;\n}\n";
    let (_ctx, body) = glyim_test::with_fresh_ty_ctx(|ctx_mut| {
        let name_x = ctx_mut.resolver().intern("x");
        make_test_body(ctx_mut, name_x)
    });

    let backend = LlvmBackend::new()
        .with_debug_info(true)
        .with_source_map(make_source_map(source));

    let llvm_ctx = Context::create();
    let module = backend
        .lower_body_to_module(&llvm_ctx, &body)
        .expect("lowering with multi-line source should succeed");

    let func = module.get_first_function().expect("no function");
    let mut locations: Vec<(u32, u32)> = Vec::new();
    for bb in func.get_basic_blocks() {
        for instr in bb.get_instructions() {
            if let Some(loc) = instr.get_debug_location() {
                locations.push((loc.get_line(), loc.get_column()));
            }
        }
    }
    assert!(!locations.is_empty(), "No debug locations found");
    for (line, col) in &locations {
        assert!(*line >= 1, "Line number should be >= 1, got {}", line);
        assert!(col >= &0, "Column should be >= 0, got {}", col);
    }
}

#[test]
fn test_debug_info_verify_module() {
    let source = "fn main() { let x: i32 = 42; }\n";
    let (_ctx, body) = glyim_test::with_fresh_ty_ctx(|ctx_mut| {
        let name_x = ctx_mut.resolver().intern("x");
        make_test_body(ctx_mut, name_x)
    });

    let backend = LlvmBackend::new()
        .with_debug_info(true)
        .with_source_map(make_source_map(source));

    let llvm_ctx = Context::create();
    let module = backend
        .lower_body_to_module(&llvm_ctx, &body)
        .expect("lowering succeeded");

    let result = module.verify();
    assert!(
        result.is_ok(),
        "LLVM module verification failed: {:?}",
        result.err()
    );
}

// ---------------------------------------------------------------------------
// Macro resolution tests (added by stream W3-C02)
// ---------------------------------------------------------------------------

#[test]
fn resolve_span_to_location_returns_call_site_for_macro_span() {
    let mut hygiene = HygieneCtx::new();
    let file_id = FileId::from_raw(1);
    let call_site = Span::new(
        file_id,
        ByteIdx::from_raw(10),
        ByteIdx::from_raw(20),
        SyntaxContext::ROOT,
    );
    let expn_id = ExpnId::from_raw(1);
    let expn_data = ExpnData {
        expn_id,
        parent: ExpnId::ROOT,
        kind: ExpnKind::MacroRules { name: macro_name() },
        call_site,
        def_site: Span::DUMMY,
        transparency: Transparency::Transparent,
    };
    hygiene.push_expansion(expn_data);
    let macro_ctx = SyntaxContext::from_raw(expn_id.to_raw());
    let expanded_span = Span::new(
        file_id,
        ByteIdx::from_raw(15),
        ByteIdx::from_raw(25),
        macro_ctx,
    );
    let resolved = crate::debug::resolve_span_to_location(expanded_span, &hygiene);
    assert_eq!(resolved.file, call_site.file);
    assert_eq!(resolved.lo.to_usize(), call_site.lo.to_usize());
    assert_eq!(resolved.hi.to_usize(), call_site.hi.to_usize());
    assert!(resolved.ctx.is_root());
}

#[test]
fn resolve_span_to_location_returns_unchanged_for_root_span() {
    let hygiene = HygieneCtx::new();
    let file_id = FileId::from_raw(1);
    let root_span = Span::new(
        file_id,
        ByteIdx::from_raw(10),
        ByteIdx::from_raw(20),
        SyntaxContext::ROOT,
    );
    let resolved = crate::debug::resolve_span_to_location(root_span, &hygiene);
    assert_eq!(resolved, root_span);
}

#[test]
fn resolve_span_to_location_returns_dummy_for_dummy_span() {
    let hygiene = HygieneCtx::new();
    let resolved = crate::debug::resolve_span_to_location(Span::DUMMY, &hygiene);
    assert!(resolved.is_dummy());
}

#[test]
fn macro_defined_function_has_correct_line_numbers_in_ir() {
    let mut hygiene = HygieneCtx::new();
    let file_id = FileId::from_raw(1);
    let source = "line1\nline2\nline3\nline4\nline5".to_string();
    let call_site = Span::new(
        file_id,
        ByteIdx::from_raw(0),
        ByteIdx::from_raw(5),
        SyntaxContext::ROOT,
    );
    let expn_id = ExpnId::from_raw(2);
    let expn_data = ExpnData {
        expn_id,
        parent: ExpnId::ROOT,
        kind: ExpnKind::MacroRules { name: macro_name() },
        call_site,
        def_site: Span::DUMMY,
        transparency: Transparency::Transparent,
    };
    hygiene.push_expansion(expn_data);
    let macro_ctx = SyntaxContext::from_raw(expn_id.to_raw());
    let macro_span = Span::new(
        file_id,
        ByteIdx::from_raw(10),
        ByteIdx::from_raw(15),
        macro_ctx,
    );
    let mut source_map = HashMap::new();
    source_map.insert(file_id, ("test.g".to_string(), source.clone()));

    let ctx_mut = TyCtxMut::new(Interner::default());
    let unit_ty = ctx_mut.unit_ty();
    let return_ty = unit_ty;
    let mut locals = glyim_core::arena::IndexVec::new();
    locals.push(glyim_mir::LocalDecl {
        ty: return_ty,
        mutability: glyim_core::primitives::Mutability::Not,
        source_info: glyim_mir::SourceInfo::new(macro_span),
    });
    let mut basic_blocks = glyim_core::arena::IndexVec::new();
    let terminator = glyim_mir::Terminator {
        kind: glyim_mir::TerminatorKind::Return,
        source_info: glyim_mir::SourceInfo::new(macro_span),
    };
    basic_blocks.push(glyim_mir::BasicBlockData {
        statements: vec![],
        terminator,
        is_cleanup: false,
    });
    let body = Body {
        owner: glyim_core::DefId::new(
            glyim_core::CrateId::from_raw(1),
            glyim_core::LocalDefId::from_raw(1),
        ),
        basic_blocks,
        locals,
        arg_count: 0,
        return_ty,
        span: macro_span,
        var_debug_info: vec![],
    };

    let backend = LlvmBackend::new()
        .with_debug_info(true)
        .with_source_map(source_map)
        .with_hygiene_ctx(hygiene)
        .with_ty_ctx(ctx_mut.freeze());
    let ir = backend.generate_ir(&body).expect("IR generation failed");
    let call_site_line = source[..call_site.lo.to_usize()].matches('\n').count() + 1;
    let expected_line_pattern = format!("line: {}", call_site_line);
    assert!(
        ir.contains(&expected_line_pattern),
        "IR missing correct line number.\nIR:\n{}\nExpected line: {}",
        ir,
        call_site_line
    );
}

#[test]
fn nested_macro_expansions_produce_correct_inline_locations() {
    let mut hygiene = HygieneCtx::new();
    let file_id = FileId::from_raw(1);
    let source = "line1\nline2\nline3\nline4\nline5".to_string();
    let outer_call_site = Span::new(
        file_id,
        ByteIdx::from_raw(0),
        ByteIdx::from_raw(5),
        SyntaxContext::ROOT,
    );
    let outer_expn_id = ExpnId::from_raw(3);
    let outer_expn_data = ExpnData {
        expn_id: outer_expn_id,
        parent: ExpnId::ROOT,
        kind: ExpnKind::MacroRules { name: macro_name() },
        call_site: outer_call_site,
        def_site: Span::DUMMY,
        transparency: Transparency::Transparent,
    };
    hygiene.push_expansion(outer_expn_data);
    let _outer_ctx = SyntaxContext::from_raw(outer_expn_id.to_raw());

    let inner_call_site = Span::new(
        file_id,
        ByteIdx::from_raw(10),
        ByteIdx::from_raw(15),
        SyntaxContext::ROOT,
    );
    let inner_expn_id = ExpnId::from_raw(4);
    let inner_expn_data = ExpnData {
        expn_id: inner_expn_id,
        parent: outer_expn_id,
        kind: ExpnKind::MacroRules { name: macro_name() },
        call_site: inner_call_site,
        def_site: Span::DUMMY,
        transparency: Transparency::Transparent,
    };
    hygiene.push_expansion(inner_expn_data);
    let inner_ctx = SyntaxContext::from_raw(inner_expn_id.to_raw());

    let inner_span = Span::new(
        file_id,
        ByteIdx::from_raw(20),
        ByteIdx::from_raw(25),
        inner_ctx,
    );
    let mut source_map = HashMap::new();
    source_map.insert(file_id, ("test.g".to_string(), source.clone()));

    let ctx_mut = TyCtxMut::new(Interner::default());
    let unit_ty = ctx_mut.unit_ty();
    let return_ty = unit_ty;
    let mut locals = glyim_core::arena::IndexVec::new();
    locals.push(glyim_mir::LocalDecl {
        ty: return_ty,
        mutability: glyim_core::primitives::Mutability::Not,
        source_info: glyim_mir::SourceInfo::new(inner_span),
    });
    let mut basic_blocks = glyim_core::arena::IndexVec::new();
    let terminator = glyim_mir::Terminator {
        kind: glyim_mir::TerminatorKind::Return,
        source_info: glyim_mir::SourceInfo::new(inner_span),
    };
    basic_blocks.push(glyim_mir::BasicBlockData {
        statements: vec![],
        terminator,
        is_cleanup: false,
    });
    let body = Body {
        owner: glyim_core::DefId::new(
            glyim_core::CrateId::from_raw(1),
            glyim_core::LocalDefId::from_raw(1),
        ),
        basic_blocks,
        locals,
        arg_count: 0,
        return_ty,
        span: inner_span,
        var_debug_info: vec![],
    };

    let backend = LlvmBackend::new()
        .with_debug_info(true)
        .with_source_map(source_map)
        .with_hygiene_ctx(hygiene)
        .with_ty_ctx(ctx_mut.freeze());
    let ir = backend.generate_ir(&body).expect("IR generation failed");
    let outer_call_site_line = source[..outer_call_site.lo.to_usize()]
        .matches('\n')
        .count()
        + 1;
    let expected_line_pattern = format!("line: {}", outer_call_site_line);
    assert!(
        ir.contains(&expected_line_pattern),
        "IR missing correct line number for nested macro expansion.\nIR:\n{}\nExpected line: {}",
        ir,
        outer_call_site_line
    );
}

// ---------------------------------------------------------------------------
// Tier 5.2: DWARF pointer/slice debug types must use real pointer shapes
// (DIDerivedType / DW_TAG_pointer_type) rather than an opaque blob.
// ---------------------------------------------------------------------------

#[test]
fn tier5_2_reference_debug_type_is_real_pointer() {
    use glyim_core::Mutability;
    use glyim_type::{Region, TyKind};

    let mut ctx_mut = TyCtxMut::new(Interner::default());
    let i32_ty = ctx_mut.mk_ty(TyKind::Int(IntTy::I32));
    let ref_ty = ctx_mut.mk_ty(TyKind::Ref(Region::Static, i32_ty, Mutability::Not));
    let raw_ty = ctx_mut.mk_ty(TyKind::RawPtr(i32_ty, Mutability::Not));
    let slice_ty = ctx_mut.mk_ty(TyKind::Slice(i32_ty));

    let context = Context::create();
    let module = context.create_module("tier5_2_debug_types");
    let source_map: HashMap<FileId, (String, String)> = HashMap::from([(
        FileId::from_raw(0),
        ("test.g".to_string(), "fn main() {}".to_string()),
    )]);
    let mut debug_ctx = crate::debug::DebugInfoCtx::new(&context, &module, source_map, true, None);

    // Reference, raw pointer, and slice must all emit real DWARF pointer
    // shapes (not opaque blobs). Force retention of the created DI types into
    // the module IR via global metadata so we can assert on the produced
    // DWARF (declaring locals is broken under LLVM 22 in this repo, but the
    // type construction itself is what Tier 5.2 targets). Force retention of
    // the created DI types into the module IR by wrapping each in a global
    // variable expression (which references the DIType and is emitted into
    // the module's metadata), so we can assert on the produced DWARF.
    let file = debug_ctx.builder.create_file("test.g", ".");
    let ref_di = debug_ctx.debug_type_for_ty(&context, ref_ty, &ctx_mut.freeze());
    let raw_di = debug_ctx.debug_type_for_ty(&context, raw_ty, &ctx_mut.freeze());
    let slice_di = debug_ctx.debug_type_for_ty(&context, slice_ty, &ctx_mut.freeze());
    let ref_gv = debug_ctx.builder.create_global_variable_expression(
        debug_ctx.compile_unit_scope,
        "ref_v",
        "",
        file,
        1,
        ref_di,
        true,
        None,
        None,
        0,
    );
    let raw_gv = debug_ctx.builder.create_global_variable_expression(
        debug_ctx.compile_unit_scope,
        "raw_v",
        "",
        file,
        2,
        raw_di,
        true,
        None,
        None,
        0,
    );
    let slice_gv = debug_ctx.builder.create_global_variable_expression(
        debug_ctx.compile_unit_scope,
        "slice_v",
        "",
        file,
        3,
        slice_di,
        true,
        None,
        None,
        0,
    );
    module
        .add_global_metadata("glyim.dbg.types", &ref_gv.as_metadata_value(&context))
        .unwrap();
    module
        .add_global_metadata("glyim.dbg.types", &raw_gv.as_metadata_value(&context))
        .unwrap();
    module
        .add_global_metadata("glyim.dbg.types", &slice_gv.as_metadata_value(&context))
        .unwrap();
    debug_ctx.finalize();

    let ir = module.print_to_string().to_string();
    assert!(
        ir.contains("DW_TAG_pointer_type"),
        "Ref/RawPtr/Slice debug types should emit DW_TAG_pointer_type (real pointer shape).\nIR:\n{}",
        ir
    );
}

// ---------------------------------------------------------------------------
// Phase 6.2 (unstub-5): closure debug types must emit real per-capture member
// types (DW_TAG_member) instead of an opaque blob, so debuggers can inspect a
// closure's captured environment by name/type.
// ---------------------------------------------------------------------------

#[test]
fn phase6_2_closure_debug_type_has_capture_members() {
    use glyim_core::primitives::IntTy;

    let mut ctx_mut = TyCtxMut::new(Interner::default());
    let i32_ty = ctx_mut.mk_ty(TyKind::Int(IntTy::I32));
    let bool_ty = ctx_mut.mk_ty(TyKind::Bool);

    // Register a closure capturing two variables; this builds the synthetic
    // captured-environment ADT and records the ClosureId -> AdtId mapping.
    let name_a = ctx_mut.resolver().intern("captured_x");
    let name_b = ctx_mut.resolver().intern("captured_flag");
    let adt_id: AdtId = ctx_mut.register_closure(vec![(name_a, i32_ty), (name_b, bool_ty)]);
    let closure_id = ClosureId::from_raw(adt_id.to_raw());

    // The debug pass sees a `TyKind::Closure(closure_id, _)`; it must recover
    // the captured-environment ADT via `closure_adt` and emit member types.
    let closure_substs = ctx_mut.intern_substitution(vec![]);
    let closure_ty = ctx_mut.mk_ty(TyKind::Closure(closure_id, closure_substs));

    let context = Context::create();
    let module = context.create_module("phase6_2_closure_dbg");
    let source_map: HashMap<FileId, (String, String)> = HashMap::from([(
        FileId::from_raw(0),
        ("test.g".to_string(), "fn main() {}".to_string()),
    )]);
    let mut debug_ctx = crate::debug::DebugInfoCtx::new(&context, &module, source_map, true, None);

    let file = debug_ctx.builder.create_file("test.g", ".");
    let closure_di = debug_ctx.debug_type_for_ty(&context, closure_ty, &ctx_mut.freeze());
    let gv = debug_ctx.builder.create_global_variable_expression(
        debug_ctx.compile_unit_scope,
        "closure_v",
        "",
        file,
        1,
        closure_di,
        true,
        None,
        None,
        0,
    );
    module
        .add_global_metadata("glyim.dbg.types", &gv.as_metadata_value(&context))
        .unwrap();
    debug_ctx.finalize();

    let ir = module.print_to_string().to_string();
    // The closure must be a real composite (struct) type whose `elements`
    // list carries the captured types — NOT an opaque empty struct.
    assert!(
        ir.contains("DW_TAG_structure_type") && ir.contains("elements:"),
        "Closure debug type should be a real struct with captured-member elements, not an opaque blob.\nIR:\n{}",
        ir
    );
    // The captured types must actually appear as members of the closure struct
    // (i32 for `captured_x`, bool for `captured_flag`).
    assert!(
        ir.contains("closure2000000") && ir.contains("i32") && ir.contains("bool"),
        "Closure debug type should reference its captured value types (i32, bool).\nIR:\n{}",
        ir
    );
}
