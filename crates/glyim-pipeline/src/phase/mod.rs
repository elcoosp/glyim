//! Typed compilation phases (M5.1 Phase 1 + 2 + 3: trait extraction, core
//! pipeline extraction, driver final extraction).
//!
//! Introduces the [`CompilerPhase`] trait that encapsulates a single
//! compilation stage. Phase 1 extracted the front-end stages (lex+parse,
//! expand+def-map); Phase 2 extracts the core MIR pipeline slice
//! ([`lower::LowerPhase`], [`mono::MonoPhase`]); Phase 3 extracts the
//! remaining inline slices ([`hir::HirPhase`], [`typeck::TypeckPhase`],
//! [`emit::EmitPhase`]) so the driver is a ≤60-line phase chain. Each phase
//! threads shared state through [`PhaseCtx`] and returns a typed output, so
//! the data flow is explicit rather than buried in one ~464-line function.

pub mod def_map;
pub mod emit;
pub mod hir;
pub mod lex_parse;
pub mod lower;
pub mod mono;
pub mod typeck;

use crate::phase_prof::PhaseRecorder;
use glyim_db::Database;
use glyim_diag::{DiagSink, GlyimDiagnostic};
use std::cell::RefCell;

/// Shared, borrowed driver state every phase reads from / writes to: the
/// query database, the phase-profiling recorder, and the diagnostic sink.
pub struct PhaseCtx<'a> {
    /// The compiler's query database (source map, interner, ty-ctx slot).
    pub db: &'a mut Database,
    /// The wall-clock phase profiler (`--time-passes` / Chrome trace).
    pub recorder: &'a PhaseRecorder,
    /// The accumulating diagnostic sink (errors + warnings).
    pub sink: &'a RefCell<DiagSink>,
}

/// A single typed compilation stage (M5.1). Each phase carries its inputs as
/// fields (set at construction) and threads shared state through [`PhaseCtx`];
/// `run` returns the stage's typed output or a diagnostic vector. The
/// monolithic `compile_file_with_artifacts` driver chains the phases in order.
pub trait CompilerPhase {
    /// What the phase produces (the successor's input).
    type Output;
    /// Human-readable phase name for diagnostics/profiling.
    fn name(&self) -> &'static str;
    /// Run the stage: use `ctx` for shared db/recorder/sink and return the
    /// typed output, or `Err` with the accumulated diagnostics.
    fn run(&self, ctx: &mut PhaseCtx) -> glyim_diag::CompResult<Self::Output>;
}

/// Extract the accumulated diagnostics from a [`PhaseCtx`]'s sink as the
/// pipeline's `CompResult` error arm, preserving the original monolith's
/// `into_diagnostics` dedup/stabilization. The sink is swapped for a fresh
/// one in place, so the caller's `RefCell` survives for later warnings.
pub(crate) fn extract_diagnostics(ctx: &PhaseCtx<'_>) -> Vec<GlyimDiagnostic> {
    std::mem::replace(&mut *ctx.sink.borrow_mut(), DiagSink::new()).into_diagnostics()
}
