//! Module-assembly walkthrough: emit MIR bodies into a loadable module image.
//!
//! `BytecodeBackend::generate` writes a flat, unloadable instruction stream;
//! `generate_module_image` is the multi-function seam — it resolves the
//! emitter's fn-table index space and encodes the `glyim.module-image/1`
//! container the VM crate loads. This walkthrough assembles a two-body
//! module (`main` calling `leaf` through the fn-table slot the call
//! interned), loads it back through `decode_module`, and executes it on the
//! bytecode VM — the same path the `vm_interp_diff` differential tests
//! verify against the MIR interpreter. Run with:
//!
//! ```text
//! cargo run -p glyim-codegen --example module_assembly_walkthrough
//! ```

use glyim_bytecode_vm::decode_module;
use glyim_codegen::{BytecodeBackend, OptLevel};
use glyim_core::TargetInfo;
use glyim_core::def_id::{CrateId, DefId, FnDefId, LocalDefId};
use glyim_mir::{
    BasicBlockData, BasicBlockIdx, Body, LocalIdx, MirConst, MirConstKind, Operand, Place,
    SourceInfo, Terminator, TerminatorKind,
};
use glyim_span::Span;
use glyim_test::test_ty_ctx;
use glyim_type::{Substitution, Ty};
use std::sync::Arc;

/// `leaf` (local def id 1): a body whose single block returns.
fn leaf_body() -> Arc<Body> {
    let owner = DefId::new(CrateId::from_raw(0), LocalDefId::from_raw(1));
    let mut body = Body::dummy(owner);
    body.basic_blocks[BasicBlockIdx::from_raw(0)].terminator = Terminator {
        kind: TerminatorKind::Return,
        source_info: SourceInfo::new(Span::DUMMY),
    };
    Arc::new(body)
}

/// `main` (local def id 0): calls the fn-table slot the call interned for
/// `leaf` (fn-const with local id 1), stores into local 1, resumes at block
/// 1, and returns that local.
fn main_body() -> Arc<Body> {
    let owner = DefId::new(CrateId::from_raw(0), LocalDefId::from_raw(0));
    let mut body = Body::dummy(owner);
    body.basic_blocks[BasicBlockIdx::from_raw(0)].terminator = Terminator {
        kind: TerminatorKind::Call {
            func: Operand::Constant(MirConst {
                kind: MirConstKind::Fn(FnDefId::from_raw(1), Substitution::empty()),
                ty: Ty::ERROR,
                span: Span::DUMMY,
            }),
            args: vec![],
            destination: Place::new(LocalIdx::from_raw(1)),
            target: Some(BasicBlockIdx::from_raw(1)),
            cleanup: None,
        },
        source_info: SourceInfo::new(Span::DUMMY),
    };
    body.basic_blocks.push(BasicBlockData {
        statements: Vec::new(),
        terminator: Terminator {
            kind: TerminatorKind::Return,
            source_info: SourceInfo::new(Span::DUMMY),
        },
        is_cleanup: false,
    });
    Arc::new(body)
}

fn main() {
    let backend =
        BytecodeBackend::with_ty_ctx(Arc::new(test_ty_ctx().freeze()), TargetInfo::default())
            .with_opt_level(OptLevel::O0);
    let bodies = vec![
        ("main".to_string(), main_body()),
        ("leaf".to_string(), leaf_body()),
    ];

    // Assemble: every body emitted, fn-table slots resolved, entry picked.
    let image = backend
        .generate_module_image(&bodies)
        .expect("two-body module must assemble");
    println!(
        "assembled {} body(ies) into {} image bytes",
        bodies.len(),
        image.len()
    );

    // Load and run: the consumer's path over the emitted artifact.
    let module = decode_module(&image).expect("assembled image must decode");
    println!(
        "module: {} function(s), entry index {}",
        module.functions.len(),
        module.entry
    );
    let result = module.run().expect("decoded module must execute");
    println!("entry returned {result:?} (leaf's unit return surfaces as Int(0))");

    // The O0 contract: assembled jump targets are only accurate at O0
    // (block tables are recorded before the peephole pass), so higher
    // levels are refused explicitly instead of shipping wrong jumps.
    let refused =
        BytecodeBackend::with_ty_ctx(Arc::new(test_ty_ctx().freeze()), TargetInfo::default())
            .with_opt_level(OptLevel::O1)
            .generate_module_image(&bodies);
    match refused {
        Err(diagnostics) => println!("O1 assembly refused: {}", diagnostics[0]),
        Ok(_) => panic!("O1 assembly must be refused"),
    }
}
