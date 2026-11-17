//! Module-image assembly timing harness (`generate_module_image`).
//!
//! Dependency-free (`harness = false`), mirroring the `glyim-mir-interp`
//! `run_program` bench: only `std::time::Instant` + `std::hint::black_box`
//! (the `glyim-test` dev-dependency provides the blessed `test_ty_ctx`
//! helper, same as the crate's unit tests). Every multi-function artifact
//! pays the assembly path once per `--emit` run, so its per-call cost must
//! stay small against a real compile. This bench prints the per-op cost so
//! CI logs can track headroom. Run with:
//!
//! ```text
//! cargo bench -p glyim-codegen --bench module_assembly
//! ```

use glyim_bytecode_vm::decode_module;
use glyim_codegen::{BytecodeBackend, OptLevel};
use glyim_core::{FnDefId, TargetInfo};
use glyim_mir::*;
use glyim_span::Span;
use glyim_test::test_ty_ctx;
use glyim_type::Substitution;
use std::hint::black_box;
use std::sync::Arc;
use std::time::Instant;

/// A leaf body: single block, bare `Return` (the emitter pushes local 0 for
/// the return place).
fn leaf_body(owner: glyim_core::def_id::DefId) -> Arc<Body> {
    let mut body = Body::dummy(owner);
    body.basic_blocks[BasicBlockIdx::from_raw(0)].terminator = Terminator {
        kind: TerminatorKind::Return,
        source_info: SourceInfo::new(Span::DUMMY),
    };
    Arc::new(body)
}

/// A `main` body calling `callee` by fn-const into local 1, resuming at its
// second (empty) block.
fn main_body(callee: FnDefId) -> Arc<Body> {
    let mut body = Body::dummy(glyim_core::def_id::DefId::new(
        glyim_core::CrateId::from_raw(0),
        glyim_core::def_id::LocalDefId::from_raw(0),
    ));
    body.basic_blocks[BasicBlockIdx::from_raw(0)].terminator = Terminator {
        kind: TerminatorKind::Call {
            func: Operand::Constant(MirConst {
                kind: MirConstKind::Fn(callee, Substitution::empty()),
                ty: glyim_type::Ty::ERROR,
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

/// The measured corpus: `count` leaves (owners 1..=count) plus a `main`
/// (owner 0) calling the first leaf — the multi-function shape the assembly
/// seam exists for.
fn measured_bodies(count: u32) -> Vec<(String, Arc<Body>)> {
    let mut bodies = vec![("main".to_string(), main_body(FnDefId::from_raw(1)))];
    for i in 1..=count {
        bodies.push((
            format!("leaf_{i}"),
            leaf_body(glyim_core::def_id::DefId::new(
                glyim_core::CrateId::from_raw(0),
                glyim_core::def_id::LocalDefId::from_raw(i),
            )),
        ));
    }
    bodies
}

fn main() {
    let backend =
        BytecodeBackend::with_ty_ctx(Arc::new(test_ty_ctx().freeze()), TargetInfo::default())
            .with_opt_level(OptLevel::O0);
    let bodies = measured_bodies(32);

    const ITERATIONS: u32 = 2_000;
    let runs = 20;
    let mut best_assemble = u64::MAX;
    let mut best_assemble_load = u64::MAX;
    for _ in 0..runs {
        let start = Instant::now();
        for _ in 0..ITERATIONS {
            black_box(
                backend
                    .generate_module_image(black_box(&bodies))
                    .expect("corpus must assemble"),
            );
        }
        best_assemble =
            best_assemble.min(start.elapsed().as_nanos() as u64 / u64::from(ITERATIONS));

        let start = Instant::now();
        for _ in 0..ITERATIONS {
            let image = backend
                .generate_module_image(black_box(&bodies))
                .expect("corpus must assemble");
            black_box(decode_module(black_box(&image)).expect("corpus must load"));
        }
        best_assemble_load =
            best_assemble_load.min(start.elapsed().as_nanos() as u64 / u64::from(ITERATIONS));
    }

    println!("corpus: {} bodies", bodies.len());
    println!("best assemble: {best_assemble} ns/op");
    println!("best assemble+load: {best_assemble_load} ns/op");
}
