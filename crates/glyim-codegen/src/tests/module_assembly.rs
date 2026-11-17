//! Multi-function module-image assembly tests (`generate_module_image`).
//!
//! The bytecode backend's `generate` writes a flat concatenation of every
//! body's instruction stream — a file no loader can execute (no function
//! table, no entry point). `generate_module_image` is the assembly seam the
//! differential test's scope note calls for: it emits every body, resolves
//! the emitter's fn-table index space, and produces a loadable
//! `glyim.module-image/1` artifact whose functions, entry, and block tables
//! are exactly what the VM's decoder validates and runs.

use super::super::*;
use glyim_bytecode_vm::{IMAGE_MAGIC, IMAGE_VERSION, decode_module};
use glyim_core::def_id::{CrateId, DefId, FnDefId, LocalDefId};
use glyim_core::primitives::Mutability;
use glyim_mir::Terminator;
use glyim_span::Span;
use glyim_type::{Substitution, Ty};
use std::sync::Arc;

/// The owner `DefId` for local function `n`.
fn owner(n: u32) -> DefId {
    DefId::new(CrateId::from_raw(0), LocalDefId::from_raw(n))
}

/// A `LocalDecl` with the ERROR type (the unit tests' convention: emission
/// reads layouts only through projections these bodies never touch).
fn local_decl() -> LocalDecl {
    LocalDecl {
        ty: Ty::ERROR,
        mutability: Mutability::Not,
        source_info: SourceInfo::new(Span::DUMMY),
    }
}

/// A body whose single block returns (no statements; the emitter pushes
/// local 0 then `OP_RETURN`).
fn return_body(owner_id: DefId, n_locals: usize) -> Arc<Body> {
    let mut body = Body::dummy(owner_id);
    for _ in 0..n_locals {
        body.locals.push(local_decl());
    }
    body.basic_blocks[BasicBlockIdx::from_raw(0)].terminator = Terminator {
        kind: TerminatorKind::Return,
        source_info: SourceInfo::new(Span::DUMMY),
    };
    Arc::new(body)
}

/// A body that calls `callee` (by fn-const) into local `dest`, resuming at
/// `target`, then falls through to the given extra blocks.
fn call_body(owner_id: DefId, callee: FnDefId, dest: u32, target: u32) -> Arc<Body> {
    let mut body = Body::dummy(owner_id);
    body.locals.push(local_decl());
    body.locals.push(local_decl());
    body.basic_blocks[BasicBlockIdx::from_raw(0)].terminator = Terminator {
        kind: TerminatorKind::Call {
            func: Operand::Constant(MirConst {
                kind: MirConstKind::Fn(callee, Substitution::empty()),
                ty: Ty::ERROR,
                span: Span::DUMMY,
            }),
            args: vec![],
            destination: Place::new(LocalIdx::from_raw(dest)),
            target: Some(BasicBlockIdx::from_raw(target)),
            cleanup: None,
        },
        source_info: SourceInfo::new(Span::DUMMY),
    };
    Arc::new(body)
}

fn backend() -> BytecodeBackend {
    BytecodeBackend::with_ty_ctx(
        std::sync::Arc::new(glyim_type::TyCtxMut::new(glyim_core::Interner::default()).freeze()),
        glyim_core::TargetInfo::default(),
    )
}

#[test]
fn module_image_assembles_multi_function_modules() {
    // main calls helper, whose fn-const is interned at slot 0 during main's
    // emission; helper's own body claims that slot, main is appended after
    // the fn table as the unclaimed body.
    let helper = return_body(owner(1), 1);
    let main = call_body(owner(0), FnDefId::from_raw(1), 0, 1);
    let bodies = vec![("main".to_string(), main), ("helper".to_string(), helper)];

    let image = backend()
        .generate_module_image(&bodies)
        .expect("assembly must succeed");

    assert_eq!(
        &image[0..4],
        &IMAGE_MAGIC,
        "assembled image starts with the container magic"
    );
    let module = decode_module(&image).expect("assembled image must decode");
    assert_eq!(module.functions.len(), 2, "one slot per emitted body");
    // Slot 0 is the interned callee: helper (Body::dummy's pre-seeded
    // return local + one pushed local). The entry is the body named `main`
    // (pre-seeded local + two pushed).
    assert_eq!(module.functions[0].n_locals, 2, "slot 0 is helper");
    assert_eq!(module.functions[0].arg_count, 0);
    assert_eq!(module.entry, 1, "entry is the body named main");
    let entry_fn = &module.functions[module.entry];
    assert_eq!(
        entry_fn.n_locals, 3,
        "main's local count travels in the image"
    );
    // The emitted call must reference the index helper was assembled at:
    // LOAD_CONST <idx=0> immediately before OP_CALL.
    let load_const_call: Vec<u8> = std::iter::once(OP_LOAD_CONST)
        .chain(0i64.to_le_bytes())
        .chain(std::iter::once(OP_CALL))
        .collect();
    let windows: Vec<&[u8]> = entry_fn
        .code
        .windows(load_const_call.len())
        .filter(|w| *w == load_const_call.as_slice())
        .collect();
    assert_eq!(
        windows.len(),
        1,
        "main's call must push helper's assembled index 0: {entry_fn:?}"
    );
}

#[test]
fn module_image_entry_prefers_the_main_symbol() {
    // Bodies in an order where "main" is not first: entry must still land
    // on it, not on the first body.
    let first = return_body(owner(2), 1);
    let entry = return_body(owner(0), 1);
    let bodies = vec![("first".to_string(), first), ("main".to_string(), entry)];
    let image = backend()
        .generate_module_image(&bodies)
        .expect("assembly must succeed");
    let module = decode_module(&image).expect("decode");
    assert_eq!(module.entry, 1, "entry is the body named main");
}

#[test]
fn module_image_carries_per_function_block_tables() {
    // main: block 0 calls helper (dest local 0, resume block 1); block 1
    // returns. The assembled function must carry both blocks' byte offsets
    // so the VM resolves the call's resume target by block index.
    let helper = return_body(owner(1), 1);
    let mut main_body = Body::dummy(owner(0));
    main_body.locals.push(local_decl());
    main_body.locals.push(local_decl());
    main_body.basic_blocks[BasicBlockIdx::from_raw(0)].terminator = Terminator {
        kind: TerminatorKind::Call {
            func: Operand::Constant(MirConst {
                kind: MirConstKind::Fn(FnDefId::from_raw(1), Substitution::empty()),
                ty: Ty::ERROR,
                span: Span::DUMMY,
            }),
            args: vec![],
            destination: Place::new(LocalIdx::from_raw(0)),
            target: Some(BasicBlockIdx::from_raw(1)),
            cleanup: None,
        },
        source_info: SourceInfo::new(Span::DUMMY),
    };
    main_body.basic_blocks.push(BasicBlockData {
        statements: Vec::new(),
        terminator: Terminator {
            kind: TerminatorKind::Return,
            source_info: SourceInfo::new(Span::DUMMY),
        },
        is_cleanup: false,
    });
    let bodies = vec![
        ("main".to_string(), Arc::new(main_body)),
        ("helper".to_string(), helper),
    ];
    let image = backend()
        .generate_module_image(&bodies)
        .expect("assembly must succeed");
    let module = decode_module(&image).expect("decode");
    let entry_fn = &module.functions[module.entry];
    assert_eq!(
        entry_fn.block_offsets.len(),
        2,
        "both blocks' byte offsets travel in the image"
    );
}

#[test]
fn module_image_refuses_optimized_block_tables() {
    // The block-offset tables are recorded before the peephole pass, so
    // assembly at O1+ would ship wrong jump targets. The seam must refuse
    // loudly instead of silently assembling a broken image.
    let body = return_body(owner(0), 1);
    let bodies = vec![("main".to_string(), body)];
    let err = backend()
        .with_opt_level(crate::OptLevel::O1)
        .generate_module_image(&bodies)
        .expect_err("O1 assembly must be refused");
    assert!(
        err.iter().any(|d| d.to_string().contains("OptLevel::O0")),
        "error must name the O0 requirement: {err:?}"
    );
}

#[test]
fn module_image_refuses_empty_body_sets() {
    let err = backend()
        .generate_module_image(&[])
        .expect_err("an image with no functions cannot run");
    assert!(
        err.iter()
            .any(|d| d.to_string().contains("at least one body")),
        "error must explain the empty-set refusal: {err:?}"
    );
}

#[test]
fn module_image_version_is_the_supported_one() {
    let body = return_body(owner(0), 1);
    let image = backend()
        .generate_module_image(&[("main".to_string(), body)])
        .expect("assembly must succeed");
    assert_eq!(
        u16::from_le_bytes([image[4], image[5]]),
        IMAGE_VERSION,
        "assembled image declares the format version this crate decodes"
    );
}
