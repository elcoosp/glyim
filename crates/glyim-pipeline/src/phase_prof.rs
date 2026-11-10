//! Phase-granularity instrumentation for the compilation pipeline (M2.1).
//!
//! Every front-half phase (load → parse → expand → defmap → hir → typeck →
//! lower → borrowck → opt → mono) plus codegen is bracketed by a
//! [`PhaseRecorder::enter`] guard that (a) opens a `tracing` span so
//! `RUST_LOG`-style subscribers see phase events as they happen and
//! (b) accumulates wall-clock microseconds into a [`PhaseProfile`].
//!
//! The profile is the machine-checked backbone of three CLI surfaces:
//!
//! * `--time-passes` prints the profile as a fixed-width table on stderr;
//! * `--self-profile[=FILE]` writes it as a Chrome Trace event list
//!   (openable in Perfetto / `chrome://tracing`);
//! * `--emit=stats` writes a schema-versioned JSON summary of the front
//!   half plus mono/warning counters ([`StatsReport`]).
//!
//! All three renderers are byte-deterministic for a given profile: the
//! phase set and its order are pinned by [`Phase::ALL`], no clock or PRNG
//! data reaches the output, and map iteration is avoided entirely. The
//! *durations* of course vary between runs — determinism covers schema,
//! order, and formatting, which is what downstream parsers rely on.
//!
//! Cost model: one `Instant::now()` pair plus two `Cell` writes per
//! phase entry, and a no-op span when no subscriber is interested —
//! negligible against any real compile, so recording is unconditional
//! and the CLI decides what to emit.

use std::cell::Cell;
use std::time::Instant;

/// Number of phases in [`Phase::ALL`]; the profile's fixed slot count.
const PHASE_COUNT: usize = 11;

/// The compilation phases the pipeline instruments, in execution order.
///
/// The order and spelling are part of every renderer's output schema:
/// `--time-passes` rows, `--self-profile` trace events, and `--emit=stats`
/// entries all follow [`Phase::ALL`]. Adding a phase appends a row;
/// reordering or renaming is a breaking change for downstream parsers.
/// The discriminant doubles as the profile's slot index, so `ALL` and the
/// discriminant order must stay in lockstep (pinned by
/// `phase_prof_tests::phase_order_is_stable`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Phase {
    /// Crate source loading (`load_crate_source`, module splicing).
    Load = 0,
    /// Lexing + parsing (`glyim_frontend::parse_to_syntax`).
    Parse = 1,
    /// Builtin + procedural macro expansion (`glyim_meta::Expander`).
    Expand = 2,
    /// Name resolution / definition map (`glyim_def_map::build_def_map`).
    DefMap = 3,
    /// HIR lowering (`lower_crate_for_pipeline_with_file_id`).
    Hir = 4,
    /// Trait/alias registration and type checking (`typeck_crate`).
    Typeck = 5,
    /// THIR → MIR body lowering (`glyim_lower::lower_body`, once per body).
    Lower = 6,
    /// Borrow checking (`glyim_borrowck::check_borrows`, once per body).
    Borrowck = 7,
    /// MIR optimization (`glyim_opt::optimize`, once per body).
    Opt = 8,
    /// Monomorphization: root discovery, collection, polymorphization
    /// dedup, post-mono checks, and CGU partition.
    Mono = 9,
    /// Backend code generation (`CodegenBackend::generate`) — only ever
    /// non-zero on object-producing paths (`--emit=obj|exec|cdylib` and
    /// the LTO variants); analysis-only emits keep this slot at zero.
    Codegen = 10,
}

impl Phase {
    /// Every phase in pipeline execution order (the output-schema order).
    pub const ALL: [Phase; PHASE_COUNT] = [
        Phase::Load,
        Phase::Parse,
        Phase::Expand,
        Phase::DefMap,
        Phase::Hir,
        Phase::Typeck,
        Phase::Lower,
        Phase::Borrowck,
        Phase::Opt,
        Phase::Mono,
        Phase::Codegen,
    ];

    /// The phase's stable wire name, as printed by every renderer.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Phase::Load => "load",
            Phase::Parse => "parse",
            Phase::Expand => "expand",
            Phase::DefMap => "defmap",
            Phase::Hir => "hir",
            Phase::Typeck => "typeck",
            Phase::Lower => "lower",
            Phase::Borrowck => "borrowck",
            Phase::Opt => "opt",
            Phase::Mono => "mono",
            Phase::Codegen => "codegen",
        }
    }

    /// Slot index in the profile's fixed arrays — the discriminant, so
    /// `ALL` order and discriminant order must stay in lockstep.
    fn index(self) -> usize {
        self as usize
    }
}

/// Accumulated wall-clock time per phase, in fixed [`Phase::ALL`] order.
///
/// Cloned out of the pipeline's recorder; treat it as an immutable
/// snapshot. Zeros mean "phase did not run" (e.g. [`Phase::Codegen`] on
/// analysis-only emit modes).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PhaseProfile {
    /// Accumulated microseconds per phase slot (0 = phase never ran).
    micros: [u64; PHASE_COUNT],
    /// How many times each phase was entered (lower/borrowck/opt run
    /// once per body, so their hit counts equal the body count).
    hits: [u32; PHASE_COUNT],
}

impl PhaseProfile {
    /// A profile with every phase at zero — the shape before any phase
    /// has run, and the base for the renderer tests' synthetic builds.
    pub const EMPTY: PhaseProfile = PhaseProfile {
        micros: [0; PHASE_COUNT],
        hits: [0; PHASE_COUNT],
    };

    /// Accumulated microseconds recorded for `phase` (0 if never run).
    #[must_use]
    pub fn micros(&self, phase: Phase) -> u64 {
        self.micros[phase.index()]
    }

    /// How many times `phase` was entered (0 if never run).
    #[must_use]
    pub fn hits(&self, phase: Phase) -> u32 {
        self.hits[phase.index()]
    }

    /// Sum of every phase's microseconds — the measured compile time
    /// inside the instrumented region.
    #[must_use]
    pub fn total_micros(&self) -> u64 {
        self.micros.iter().sum()
    }

    /// Seed builder: returns a copy with `micros` recorded for `phase`
    /// (and one hit). Public so tests, benches, and examples can build
    /// synthetic profiles with exact, deterministic durations; the
    /// recorder is the only *production* writer.
    #[must_use]
    pub fn with_micros(mut self, phase: Phase, micros: u64) -> Self {
        self.micros[phase.index()] = micros;
        self.hits[phase.index()] += 1;
        self
    }

    /// Fixed-width text table for `--time-passes` (stderr).
    ///
    /// Byte-deterministic: rows in [`Phase::ALL`] order, fixed column
    /// widths, one-decimal shares. When the total is 0 every share is
    /// `0.0%` (no NaN, no division).
    #[must_use]
    pub fn render_text(&self) -> String {
        use std::fmt::Write as _;
        let total = self.total_micros();
        let mut out = String::with_capacity(PHASE_COUNT * 48);
        let _ = writeln!(out, "Phase         Micros       Share");
        let _ = writeln!(out, "------------  -----------  --------");
        for phase in Phase::ALL {
            let micros = self.micros[phase.index()];
            let share = share_percent(micros, total);
            let _ = writeln!(
                out,
                "{:<10}  {:>11}  {:>7.1}%",
                phase.as_str(),
                micros,
                share
            );
        }
        let total_share = if total > 0 { 100.0 } else { 0.0 };
        let _ = writeln!(out, "{:<10}  {:>11}  {:>7.1}%", "Total", total, total_share);
        out
    }

    /// Chrome Trace JSON for `--self-profile` (Perfetto / trace-viewer).
    ///
    /// One complete (`ph: "X"`) event per phase in [`Phase::ALL`] order,
    /// timestamps starting at 0 and strictly cumulative (`ts` of event N
    /// equals the end of event N-1), so the event list is deterministic
    /// given the recorded durations. `args.hits` carries the entry count.
    #[must_use]
    pub fn render_chrome_trace(&self) -> String {
        let mut events: Vec<serde_json::Value> = Vec::with_capacity(PHASE_COUNT + 2);
        events.push(serde_json::json!({
            "ph": "M", "name": "process_name", "pid": 1, "tid": 1,
            "args": { "name": "glyim-pipeline" },
        }));
        events.push(serde_json::json!({
            "ph": "M", "name": "thread_name", "pid": 1, "tid": 1,
            "args": { "name": "compile" },
        }));
        let mut ts = 0u64;
        for phase in Phase::ALL {
            let micros = self.micros[phase.index()];
            events.push(serde_json::json!({
                "ph": "X", "name": phase.as_str(), "cat": "phase",
                "pid": 1, "tid": 1,
                "ts": ts, "dur": micros,
                "args": { "hits": self.hits[phase.index()] },
            }));
            ts += micros;
        }
        serde_json::to_string(&serde_json::json!({
            "traceEvents": events,
            "displayTimeUnit": "ms",
        }))
        .unwrap_or_else(|_| "{\"traceEvents\":[]}".to_string())
    }
}

/// Share of `micros` in `total`, in percent, rounded to one decimal.
/// Returns 0.0 (never NaN) when `total` is 0.
fn share_percent(micros: u64, total: u64) -> f64 {
    if total == 0 {
        0.0
    } else {
        micros as f64 / total as f64 * 100.0
    }
}

/// The recorder lives inside `prepare_compilation` and accumulates phase
/// durations; [`PhaseRecorder::profile`] snapshots it into a
/// [`PhaseProfile`] when the compile finishes.
pub(crate) struct PhaseRecorder {
    micros: [Cell<u64>; PHASE_COUNT],
    hits: [Cell<u32>; PHASE_COUNT],
}

impl PhaseRecorder {
    /// A fresh, all-zero recorder.
    pub(crate) fn new() -> Self {
        Self {
            micros: std::array::from_fn(|_| Cell::new(0)),
            hits: std::array::from_fn(|_| Cell::new(0)),
        }
    }

    /// Open a phase bracket: enters a `tracing` span and, on drop,
    /// accumulates the elapsed microseconds into `phase`'s slot.
    ///
    /// The guard must be dropped (or scoped) when the phase ends — bind
    /// it inside a block, not as a function-level `let _g = …`, or every
    /// later phase nests inside it.
    pub(crate) fn enter(&self, phase: Phase) -> PhaseGuard<'_> {
        let span = tracing::info_span!("pipeline_phase", phase = phase.as_str());
        PhaseGuard {
            recorder: self,
            phase,
            started: Instant::now(),
            _span: span.entered(),
        }
    }

    /// Snapshot the accumulated durations into an immutable profile.
    pub(crate) fn profile(&self) -> PhaseProfile {
        PhaseProfile {
            micros: std::array::from_fn(|i| self.micros[i].get()),
            hits: std::array::from_fn(|i| self.hits[i].get()),
        }
    }
}

/// RAII bracket for one phase execution; records on drop.
pub(crate) struct PhaseGuard<'r> {
    recorder: &'r PhaseRecorder,
    phase: Phase,
    started: Instant,
    _span: tracing::span::EnteredSpan,
}

impl Drop for PhaseGuard<'_> {
    fn drop(&mut self) {
        let micros = self.started.elapsed().as_micros() as u64;
        let idx = self.phase.index();
        let prev = self.recorder.micros[idx].get();
        self.recorder.micros[idx].set(prev + micros);
        let hits = self.recorder.hits[idx].get() + 1;
        self.recorder.hits[idx].set(hits);
        tracing::debug!(
            phase = self.phase.as_str(),
            micros,
            hits,
            "pipeline phase complete"
        );
    }
}

/// The front-half measurement `--emit=stats` writes out: the phase
/// profile plus the two counters the profile alone cannot express (how
/// many monomorphized bodies the compile produced, how many advisory
/// diagnostics accompanied it).
#[derive(Clone, Debug)]
pub struct StatsReport {
    /// Per-phase wall-clock profile of the front half. [`Phase::Codegen`]
    /// is always 0 here — `--emit=stats` is analysis-only and never runs
    /// a backend.
    pub profile: PhaseProfile,
    /// Number of monomorphized bodies the compile produced.
    pub mono_bodies: usize,
    /// Number of advisory (non-error) diagnostics the compile produced.
    pub warnings: usize,
}

impl StatsReport {
    /// Schema-versioned JSON summary for `--emit=stats`.
    ///
    /// `version` is pinned at 1; bump it (and document the change) on any
    /// incompatible shape change so downstream parsers can branch.
    #[must_use]
    pub fn render_json(&self) -> String {
        let phases: Vec<serde_json::Value> = Phase::ALL
            .iter()
            .map(|phase| {
                serde_json::json!({
                    "phase": phase.as_str(),
                    "micros": self.profile.micros(*phase),
                    "hits": self.profile.hits(*phase),
                })
            })
            .collect();
        serde_json::to_string(&serde_json::json!({
            "version": 1,
            "phases": phases,
            "total_micros": self.profile.total_micros(),
            "mono_bodies": self.mono_bodies,
            "warnings": self.warnings,
        }))
        .unwrap_or_else(|_| "{\"version\":1}".to_string())
    }
}
