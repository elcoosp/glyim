//! Typed compilation phases (M5.1 Phase 1: trait extraction).
//!
//! Introduces the [`CompilerPhase`] trait that encapsulates a single
//! compilation stage, and extracts the front-end stages (lex+parse,
//! expand+def-map) out of the `prepare_compilation` monolith into phase
//! structs that implement it. The driver chains the phases in order; each
//! phase threads shared state through [`PhaseCtx`] and returns a typed
//! output, so the data flow is explicit rather than buried in one
//! ~640-line function.
//!
//! Phase 1 extracts the two cleanest, least-coupled front-end stages. The
//! typeck/lower/borrowck/opt/mono stages remain inline pending the later
//! M5.1 sub-items (typed `PhaseContext` hardening, `≤60`-line driver, no
//! `Option`-filled-later state).

pub mod def_map;
pub mod lex_parse;

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
