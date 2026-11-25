//! Built-in [`ReproCheck`] drivers — the seam between the reducer and the
//! compiler pipeline (plan M4.5, "wired to the driver").
//!
//! Both checks drive the *same in-process seam* the differential gate and
//! the M4.7 metamorphic suite drive (`compile_file_to_mir` →
//! `Pipeline::entry_main_local_id` → `glyim_mir_interp::run_program`), so
//! reduction runs without an LLVM toolchain and without subprocesses.
//! A future driver — M4.4's generated-program differential, a native
//! backend, a crash-signature bucket — plugs in by implementing
//! [`ReproCheck`]; the reducer never needs to know how the failure is
//! observed.

use glyim_db::{CrateConfig, Database};
use glyim_mir_interp::run_program;
use glyim_pipeline::{Pipeline, compile_file_to_mir};
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use tempfile::TempDir;

/// The driver seam: "does `source` still reproduce the pinned failure?"
///
/// Implementors must be pure functions of `source` (no clock, no
/// randomness — the plan's determinism constitution C6 applies to the
/// harness too), and must not mutate the input.
pub trait ReproCheck {
    /// Returns `true` when `source` reproduces the pinned failure.
    fn still_reproduces(&self, source: &str) -> bool;
}

/// Shared scratch-file plumbing for the two built-in checks: a temp
/// workspace plus a monotonic counter, mirroring the
/// `MetamorphicRunner` seam's file-per-case convention. The [`TempDir`]
/// guard is held for the check's lifetime — dropping it removes the
/// workspace.
struct Scratch {
    _guard: TempDir,
    workdir: PathBuf,
    counter: AtomicUsize,
}

impl Scratch {
    fn new() -> Self {
        let dir = TempDir::new().expect("create reducer scratch tempdir");
        let workdir = dir.path().to_path_buf();
        Self {
            _guard: dir,
            workdir,
            counter: AtomicUsize::new(0),
        }
    }

    /// Writes `source` to a fresh scratch file and returns its path plus
    /// the case index (used to keep crate names unique per case).
    fn write_case(&self, source: &str, tag: &str) -> (usize, PathBuf) {
        let index = self.counter.fetch_add(1, Ordering::SeqCst);
        let path = self.workdir.join(format!("reduce_case_{index}_{tag}.g"));
        fs::write(&path, source).expect("write reducer scratch program");
        (index, path)
    }
}

/// Compiles `source` at `opt_level` and reports the observed outcome: the
/// interpreter exit code, or the rendered diagnostics that rejected the
/// program.
///
/// Returns `Err(None)`-shaped ambiguity away: `Ok(None)` means "compiled
/// but has no `fn main` entry point" — a state a reduction step can reach
/// and that no exit-pinned repro can reproduce.
fn compile_and_run(
    scratch: &Scratch,
    source: &str,
    opt_level: u8,
) -> Result<Option<i32>, Vec<String>> {
    let (index, path) = scratch.write_case(source, "run");
    let mut db = Database::new(CrateConfig {
        name: format!("glyim_reduce_case_{index}"),
        target_triple: format!(
            "{}-unknown-{}",
            std::env::consts::ARCH,
            std::env::consts::OS
        ),
        opt_level,
    });
    let compilation = match compile_file_to_mir(&mut db, &path, opt_level) {
        Ok(compilation) => compilation,
        Err(diagnostics) => {
            return Err(diagnostics.iter().map(ToString::to_string).collect());
        }
    };
    let Some(entry_main) = Pipeline::entry_main_local_id(&mut db, &path) else {
        return Ok(None);
    };
    let bodies: Vec<_> = compilation.bodies.values().cloned().collect();
    let outcome = run_program(&bodies, compilation.ty_ctx.as_ref(), Some(entry_main));
    Ok(Some(outcome.exit_code))
}

/// Pins an interpreter exit code at a pinned optimization level.
///
/// This is the check for *runtime* repros — including the silent-fold
/// class the differential gate hunts: a repro "reproduces" exactly when
/// the program still compiles at `opt_level`, still has an entry point,
/// and still runs to `expected_exit`.
pub struct ExitRepro {
    opt_level: u8,
    expected_exit: i32,
    scratch: Scratch,
}

impl ExitRepro {
    /// Pins `expected_exit` observed at `opt_level`.
    #[must_use]
    pub fn new(opt_level: u8, expected_exit: i32) -> Self {
        Self {
            opt_level,
            expected_exit,
            scratch: Scratch::new(),
        }
    }
}

impl ReproCheck for ExitRepro {
    fn still_reproduces(&self, source: &str) -> bool {
        matches!(
            compile_and_run(&self.scratch, source, self.opt_level),
            Ok(Some(exit)) if exit == self.expected_exit
        )
    }
}

/// Pins a compile-time diagnostic: the repro reproduces exactly when the
/// program still *fails* to compile and the rendered diagnostics contain
/// the pinned needle(s).
///
/// This is the check for *compile-fail* repros (the GAP-0008 shape: the
/// compiler must keep rejecting the program with the pinned message).
/// Two pinning strengths exist because the generic messages parse
/// recovery emits ("expected pattern") are shared across many defects:
///
/// - [`DiagnosticRepro::new`] pins one needle matched as a plain
///   substring anywhere in the rendered diagnostics — use it only when
///   the needle names the defect uniquely;
/// - [`DiagnosticRepro::cascade`] pins an **ordered cascade** of
///   needles that the rendered diagnostic list must reproduce exactly:
///   one diagnostic per pinned needle, position-aligned, nothing extra —
///   the same contract the compile-fail corpus' `//~` annotations pin
///   ("every produced diagnostic must stay consumed, and nothing else
///   may appear"). A reduction step that mangles the surrounding
///   structure changes the cascade and stops reproducing, so the fixed
///   point stays fixture-grade: it can replace the witness fixture with
///   the same annotations.
pub struct DiagnosticRepro {
    needles: Vec<String>,
    scratch: Scratch,
}

impl DiagnosticRepro {
    /// Pins one diagnostic substring (`needle` is matched as a plain
    /// substring of the rendered diagnostics).
    #[must_use]
    pub fn new(needle: impl Into<String>) -> Self {
        Self {
            needles: vec![needle.into()],
            scratch: Scratch::new(),
        }
    }

    /// Pins an ordered cascade of diagnostic substrings (see the type's
    /// doc): every needle must appear, in order, in the rendered
    /// diagnostic lines.
    #[must_use]
    pub fn cascade(needles: &[&str]) -> Self {
        Self {
            needles: needles.iter().map(ToString::to_string).collect(),
            scratch: Scratch::new(),
        }
    }
}

impl ReproCheck for DiagnosticRepro {
    fn still_reproduces(&self, source: &str) -> bool {
        let (index, path) = self.scratch.write_case(source, "diag");
        let mut db = Database::new(CrateConfig {
            name: format!("glyim_reduce_case_{index}"),
            target_triple: format!(
                "{}-unknown-{}",
                std::env::consts::ARCH,
                std::env::consts::OS
            ),
            opt_level: 0,
        });
        match compile_file_to_mir(&mut db, &path, 0) {
            Err(diagnostics) => {
                let rendered: Vec<String> = diagnostics.iter().map(ToString::to_string).collect();
                cascade_matches(&rendered, &self.needles)
            }
            Ok(_) => false,
        }
    }
}

/// Position-aligned cascade: the rendered diagnostic list must have
/// exactly one line per pinned needle, in order, each line carrying its
/// needle — the compile-fail corpus' "every diagnostic stays consumed,
/// nothing else may appear" contract in checker form.
fn cascade_matches(rendered: &[String], needles: &[String]) -> bool {
    rendered.len() == needles.len()
        && rendered
            .iter()
            .zip(needles.iter())
            .all(|(line, needle)| line.contains(needle))
}
