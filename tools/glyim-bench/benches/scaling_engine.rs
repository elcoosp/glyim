//! Scaling-fit bench (plan M2.4).
//!
//! Dependency-free (`harness = false`), mirroring the `diff_engine` bench:
//! measures the pure log-log fit plus both renderers over a synthetic
//! 64-point series (exact power law, exponent 1.5), so CI logs can track
//! the analyzer cost as series grow. Run with:
//!
//! ```text
//! cargo bench -p glyim-bench --bench scaling_engine
//! ```

use std::hint::black_box;
use std::time::Instant;

use glyim_bench::scaling::{
    DEFAULT_MAX_SCALING_EXPONENT, ScalingSample, fit_scaling, substitute_size,
};

/// The synthetic series: an exact power law (exponent 1.5, coefficient
/// 2.0) over 64 geometric-ish sizes.
fn series() -> Vec<ScalingSample> {
    (1_u64..=64)
        .map(|i| {
            let size = i * 100;
            ScalingSample {
                size,
                mean_ns: 2.0 * f64::powf(size as f64, 1.5),
            }
        })
        .collect()
}

fn main() {
    let samples = series();

    let started = Instant::now();
    let fit = black_box(fit_scaling(black_box(&samples)).expect("the synthetic series fits"));
    let fit_elapsed = started.elapsed();

    let started = Instant::now();
    let argv = black_box(
        substitute_size(
            &[
                "./glyim".to_string(),
                "--emit=exec".to_string(),
                "--size".to_string(),
                "{size}".to_string(),
            ],
            42,
        )
        .expect("the argv carries the placeholder"),
    );
    let subst_elapsed = started.elapsed();

    let outcome = glyim_bench::scaling::ScalingOutcome::new(
        "synthetic",
        samples.clone(),
        fit,
        glyim_bench::scaling::ScalingThresholds::new(DEFAULT_MAX_SCALING_EXPONENT),
    );
    let started = Instant::now();
    let text = black_box(outcome.render_text());
    let text_elapsed = started.elapsed();
    let started = Instant::now();
    let json = black_box(outcome.to_json_pretty().expect("serializes"));
    let json_elapsed = started.elapsed();

    // Contract asserts: the exact power law is recovered, and renders are
    // deterministic (plan C6).
    assert!(
        (fit.exponent - 1.5).abs() < 1e-6,
        "exponent {}",
        fit.exponent
    );
    assert_eq!(fit.n_points, 64);
    assert!(
        !outcome.is_ok(),
        "1.5 > 1.0 must fail — pin the superlinear gate"
    );
    assert_eq!(
        outcome.verdict,
        glyim_bench::scaling::ScalingVerdict::Superlinear
    );
    assert_eq!(argv.last().expect("argv"), "42");
    assert_eq!(text, outcome.render_text());
    assert_eq!(outcome.to_json_pretty().unwrap(), json);

    println!("scaling bench: 64-point exact power law (exponent 1.5)");
    println!("  fit:    {fit_elapsed:?}");
    println!("  subst:  {subst_elapsed:?}");
    println!("  text:   {text_elapsed:?}");
    println!("  json:   {json_elapsed:?}");
}
