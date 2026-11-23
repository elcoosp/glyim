//! CT-5 error-code registry overhead bench (plan §6 M3.3).
//!
//! Dependency-free (`harness = false`), mirroring `json_render.rs`: measures
//! [`ErrorCode::from_registered`] (the registry-checked constructor every
//! diagnostic now routes through) and [`ErrorCode::explain`] (the
//! `--explain` lookup) over a 1_000_000-iteration loop, so CI can track the
//! registry seam's cost as the code count grows. The first lookup also pays
//! the one-time `errors/codes.toml` parse; a warm-up loop isolates that.
//!
//! Run with:
//!
//! ```text
//! cargo bench -p glyim-diag --bench registry
//! ```

use std::hint::black_box;
use std::time::Instant;

use glyim_diag::{ErrorCategory, ErrorCode};

fn main() {
    // Warm: force the one-time `codes.toml` parse out of the timed region.
    let _ = black_box(ErrorCode::from_registered(ErrorCategory::Type, 1).explain());

    const ITER: u32 = 1_000_000;

    let start = Instant::now();
    let mut acc: u32 = 0;
    for i in 0..ITER {
        // Rotate through every registered code so the lookup is not a
        // branch-predicted hit on a single entry.
        let code = match i % 12 {
            0 => ErrorCode::from_registered(ErrorCategory::Lex, 1),
            1 => ErrorCode::from_registered(ErrorCategory::Parse, 1),
            2 => ErrorCode::from_registered(ErrorCategory::Type, 0),
            3 => ErrorCode::from_registered(ErrorCategory::Type, 1),
            4 => ErrorCode::from_registered(ErrorCategory::Type, 2),
            5 => ErrorCode::from_registered(ErrorCategory::Type, 50),
            6 => ErrorCode::from_registered(ErrorCategory::Type, 51),
            7 => ErrorCode::from_registered(ErrorCategory::Type, 60),
            8 => ErrorCode::from_registered(ErrorCategory::Type, 61),
            9 => ErrorCode::from_registered(ErrorCategory::Borrow, 1),
            10 => ErrorCode::from_registered(ErrorCategory::Comptime, 1),
            _ => ErrorCode::from_registered(ErrorCategory::Internal, 0),
        };
        acc = acc.wrapping_add(u32::from(code.number));
    }
    let from_registered_elapsed = start.elapsed();
    let _ = black_box(acc);

    let start = Instant::now();
    let mut len: usize = 0;
    for i in 0..ITER {
        let code = ErrorCode::from_registered(ErrorCategory::Type, 60);
        let explanation = black_box(code.explain());
        len = len.wrapping_add(explanation.len());
        let _ = black_box(i);
    }
    let explain_elapsed = start.elapsed();
    let _ = black_box(len);

    println!(
        "from_registered (rotated, 12 codes): {:?} total, {:.3} ns/op over {ITER} ops",
        from_registered_elapsed,
        from_registered_elapsed.as_nanos() as f64 / f64::from(ITER),
    );
    println!(
        "explain (T0060):                     {:?} total, {:.3} ns/op over {ITER} ops",
        explain_elapsed,
        explain_elapsed.as_nanos() as f64 / f64::from(ITER),
    );
}
