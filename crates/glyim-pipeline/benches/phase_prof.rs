//! Phase-profile renderer timing harness (M2.1).
//!
//! Dependency-free (`harness = false`), mirroring the `glyim-test`
//! `annotations`/`evidence` benches: the three profile renderers
//! (`--time-passes` table, `--self-profile` Chrome trace, `--emit=stats`
//! JSON) run at most once per compiler invocation, so their cost must be
//! invisible against any real compile. This bench prints the per-render
//! cost so CI logs can track headroom. Run with:
//!
//! ```text
//! cargo bench -p glyim-pipeline --bench phase_prof
//! ```

use std::hint::black_box;
use std::time::Instant;

use glyim_pipeline::phase_prof::{Phase, PhaseProfile, StatsReport};

/// Builds the profile to bench: one shaped like a real compile — every
/// phase populated, lower/borrowck/opt with several hundred hits (one
/// per body of a mid-sized crate).
fn synthetic_profile() -> PhaseProfile {
    let mut profile = PhaseProfile::EMPTY;
    for phase in Phase::ALL {
        let (micros, hits) = match phase {
            Phase::Load => (120, 1),
            Phase::Parse => (4_800, 1),
            Phase::Expand => (310, 1),
            Phase::DefMap => (900, 1),
            Phase::Hir => (6_200, 1),
            Phase::Typeck => (48_000, 1),
            Phase::Lower => (15_000, 410),
            Phase::Borrowck => (6_400, 410),
            Phase::Opt => (9_100, 410),
            Phase::Mono => (21_000, 1),
            Phase::Codegen => (88_000, 1),
        };
        for _ in 0..hits {
            profile = profile.with_micros(phase, micros / hits as u64);
        }
    }
    profile
}

fn main() {
    let profile = synthetic_profile();
    let report = StatsReport {
        profile: profile.clone(),
        mono_bodies: 410,
        warnings: 12,
    };

    // Renderer 1: `--time-passes` fixed-width table.
    let t = Instant::now();
    let mut bytes = 0usize;
    for _ in 0..10_000 {
        let text = black_box(profile.clone()).render_text();
        bytes = text.len();
        black_box(text);
    }
    let text_us = t.elapsed().as_micros() as f64 / 10_000.0;

    // Renderer 2: `--self-profile` Chrome trace.
    let t = Instant::now();
    for _ in 0..10_000 {
        let trace = black_box(profile.clone()).render_chrome_trace();
        black_box(trace);
    }
    let trace_us = t.elapsed().as_micros() as f64 / 10_000.0;

    // Renderer 3: `--emit=stats` JSON.
    let t = Instant::now();
    for _ in 0..10_000 {
        let json = black_box(report.clone()).render_json();
        black_box(json);
    }
    let json_us = t.elapsed().as_micros() as f64 / 10_000.0;

    println!("phase_prof bench (profile of 11 phases, 410-body crate shape)");
    println!("  render_text         {text_us:8.3} µs/call  ({bytes} B table)");
    println!("  render_chrome_trace {trace_us:8.3} µs/call");
    println!("  render_json         {json_us:8.3} µs/call");
    println!();
    println!(
        "Total profiled compile in the synthetic profile: {} µs",
        profile.total_micros()
    );
}
