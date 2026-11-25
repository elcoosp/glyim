//! `glyim-reduce` — CST delta-debugging with token fallback, wired to the
//! in-process driver seam (plan M4.5:
//! `docs/roadmaps/glyim-master-plan.md` §6 M4.5).
//!
//! The plan's work item: *"glyim-reduce (CST delta-debugging, token
//! fallback) wired to the driver: minimal repro + witness fixture + gap
//! stub per new bug"*. This crate is the mechanism half of that item:
//!
//! - [`ReproCheck`] is the **driver seam**: any component that can answer
//!   "does this source still reproduce the pinned failure?" drives the
//!   reduction. The built-ins ([`ExitRepro`], [`DiagnosticRepro`]) run
//!   the same in-process pipeline seam the differential gate and the
//!   M4.7 metamorphic suite drive (parse → typeck → lower → borrowck →
//!   optimize → `glyim-mir-interp`), so reduction needs no LLVM toolchain
//!   and no subprocess; a future driver (M4.4's generated-program
//!   differential, native backends) plugs in by implementing the trait.
//! - [`Reducer`] is the **CST delta-debugger**: it deletes source chunks
//!   through the real parse tree (top-level items first, then statements
//!   inside bodies), falls back to token-level deletion when the CST
//!   ladder bottoms out, and commits a deletion only while the driver
//!   seam still reports the pinned failure. The fixed point is a minimal
//!   repro whose defect expression survives.
//!
//! The consumer contract (plan M4.5's "minimal repro + witness fixture +
//! gap stub per new bug"): a driver finds a failing program, hands it to
//! [`Reducer`], and the returned [`ReducedRepro`] is what becomes the
//! witnessed gap's fixture. The crate ships no corpus of its own — the
//! `examples/reduce_witness.rs` walk-through reduces a real witnessed
//! gap fixture end-to-end.
//!
//! # Example
//!
//! ```
//! use glyim_reduce::{DiagnosticRepro, Reducer};
//!
//! let bloated = "fn inert(x: i32) -> i32 { x }\nfn main() -> i32 {\n    let r = match 2 {\n        | 1 => 10\n        | 2 => 20\n        _ => 30\n    };\n    return r;\n}\n";
//! // Compile-fail repros pin the full ordered cascade (the same shape
//! // the compile-fail corpus' `//~` annotations pin), so a reduction
//! // step that mangles the surrounding structure stops "reproducing"
//! // and the fixed point stays fixture-grade.
//! let check = DiagnosticRepro::cascade(&[
//!     "expected pattern, found Or",
//!     "expected FatArrow, found IntLit",
//!     "expected pattern, found FatArrow",
//!     "expected FatArrow, found IntLit",
//!     "expected pattern, found FatArrow",
//!     "expected FatArrow, found IntLit",
//! ]);
//! let reduced = Reducer::new(&check).reduce(bloated);
//! assert!(reduced.source().contains("| 1"));
//! assert!(reduced.source().contains("| 2"));
//! ```

use std::fmt;

pub mod checks;
pub mod reducer;

pub use checks::{DiagnosticRepro, ExitRepro, ReproCheck};
pub use reducer::{ReducedRepro, Reducer, ReductionPass};

/// Human-readable one-line summary of a reduction outcome, used by the
/// example and by driver logs (never a status file — the ledger owns
/// status, plan §7.2 rule 7).
impl fmt::Display for ReducedRepro {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "reduced {} -> {} lines in {} check call(s), {} pass(es)",
            self.original_line_count(),
            self.line_count(),
            self.check_calls(),
            self.passes().len()
        )
    }
}
