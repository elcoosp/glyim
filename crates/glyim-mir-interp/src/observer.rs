//! Per-step observation seam for the MIR interpreter (M1).
//!
//! Mirrors the bytecode VM's `*Sink` observability convention (see
//! `glyim-bytecode-vm/src/lib.rs` §"Observability seams"): an infallible,
//! `'static` trait the host holds behind `Option<Box<dyn …>>`, so the
//! zero-observer path is a single `Option::is_some` branch and a plugged
//! observer pays only a vtable call per dispatch event. The interpreter
//! had no such seam — consumers (`glyip test`, `glyim-test`'s
//! `InterpRunner`, the `--emit=run` CLI) received only the final
//! `RunOutcome`/`InterpError` — so differential tracing and per-call
//! debugging had to reach into the interpreter's internals.
//!
//! The interpreter fires hooks at three points in `run_current_function`:
//!   * [`InterpObserver::on_bb_enter`] — once per basic block entered (top
//!     of the dispatch loop, after the step-limit guard);
//!   * [`InterpObserver::on_call_enter`] — when a `Call`/`Drop` terminator
//!     pushes a callee frame;
//!   * [`InterpObserver::on_call_return`] — when a `Return` terminator pops
//!     the current frame (or the entry body returns).
//!
//! Implementations must not alter interpretation semantics. [`NoopObserver`]
//! is the empty default; [`TraceCollector`] records a `Vec<`[`TraceEvent`]`>`
//! for differential tracing and debugging.

use glyim_core::DefId;
use glyim_mir::BasicBlockIdx;

/// One recorded interpreter dispatch event.
///
/// Events arrive in dispatch order; a `BbEnter` for the entry body's `bb0`
/// is always first and a `CallReturn` for the entry body is always last.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TraceEvent {
    /// A basic block was entered.
    BbEnter {
        /// Owner of the body whose block was entered.
        owner: DefId,
        /// The basic block index.
        bb: BasicBlockIdx,
    },
    /// A `Call`/`Drop` terminator pushed a callee frame.
    CallEnter {
        /// Owner of the callee body being entered.
        callee: DefId,
    },
    /// A `Return` terminator is about to pop the current frame.
    CallReturn {
        /// Owner of the body returning.
        callee: DefId,
    },
}

/// Infallible per-step observation hook for [`crate::Interpreter`].
///
/// Object-safe and `'static` so the interpreter can hold it behind
/// `Option<Box<dyn InterpObserver>>` without a type parameter (the
/// interpreter already carries a `'tcx` lifetime; a second generic would
/// ripple through every method). [`InterpObserver::as_any`] /
/// [`InterpObserver::as_any_mut`] are the standard downcast escape hatch
/// that lets [`crate::Interpreter::take_trace`] recover the typed
/// [`TraceCollector`] a consumer installed via
/// [`crate::Interpreter::with_trace_collector`], and
/// [`crate::Interpreter::take_observer`] hand a custom observer back to its
/// concrete type.
///
/// # Conventions
///
/// * **Infallible.** Observation must never change a run's result; an
///   observer that could fail should swallow its error internally (the
///   bytecode VM's sinks follow the same rule).
/// * **No spans.** The interpreter is below source-level; the
///   `owner: DefId` + `bb: BasicBlockIdx` pair is the unit a caller maps
///   back to source via the pipeline's `def_id` → span tables.
pub trait InterpObserver: 'static {
    /// Fired once per basic block entered.
    fn on_bb_enter(&mut self, owner: DefId, bb: BasicBlockIdx) {
        let _ = (owner, bb);
    }
    /// Fired when a `Call`/`Drop` terminator pushes a callee frame.
    fn on_call_enter(&mut self, callee: DefId) {
        let _ = callee;
    }
    /// Fired when a `Return` terminator pops the current frame.
    fn on_call_return(&mut self, callee: DefId) {
        let _ = callee;
    }
    /// Immutable downcast escape hatch — lets a `&dyn InterpObserver` be
    /// recovered as its concrete type (e.g. `&TraceCollector`). Required
    /// because `Box<dyn InterpObserver>` has no built-in `downcast`.
    fn as_any(&self) -> &dyn std::any::Any;
    /// Mutable downcast escape hatch — lets a `&mut dyn InterpObserver` be
    /// recovered as its concrete type so an installed collector can be
    /// drained in place (see [`crate::Interpreter::take_trace`]).
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any;
}

/// Empty observer — the typed equivalent of "no observer installed".
///
/// Useful when a consumer wants the dispatch-via-`dyn` path exercised
/// without recording anything (e.g. a benchmark isolating vtable overhead
/// from collection cost). Install with
/// [`crate::Interpreter::with_observer`].
#[derive(Debug, Default, Clone, Copy)]
pub struct NoopObserver;

impl InterpObserver for NoopObserver {
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

/// Collects every dispatch event into a `Vec<`[`TraceEvent`]`>`.
///
/// Install with [`crate::Interpreter::with_trace_collector`], run the
/// program, then drain with [`crate::Interpreter::take_trace`]. The
/// collector is also `pub` so a consumer can construct one, install it via
/// [`crate::Interpreter::with_observer`], and read [`TraceCollector::events`]
/// without downcasting if preferred.
#[derive(Debug, Default)]
pub struct TraceCollector {
    events: Vec<TraceEvent>,
}

impl TraceCollector {
    /// Construct an empty collector.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The recorded events, in dispatch order.
    #[must_use]
    pub fn events(&self) -> &[TraceEvent] {
        &self.events
    }

    /// Drain the recorded events, leaving the collector empty.
    pub fn take_events(&mut self) -> Vec<TraceEvent> {
        std::mem::take(&mut self.events)
    }
}

impl InterpObserver for TraceCollector {
    fn on_bb_enter(&mut self, owner: DefId, bb: BasicBlockIdx) {
        self.events.push(TraceEvent::BbEnter { owner, bb });
    }
    fn on_call_enter(&mut self, callee: DefId) {
        self.events.push(TraceEvent::CallEnter { callee });
    }
    fn on_call_return(&mut self, callee: DefId) {
        self.events.push(TraceEvent::CallReturn { callee });
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}
