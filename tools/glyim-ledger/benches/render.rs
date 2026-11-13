//! Known-gaps renderer bench (plan M1.9 remainder).
//!
//! Dependency-free (`harness = false`), mirroring the `gates` bench:
//! measures both [`glyim_ledger::KnownGapsRenderer`] faces — the Markdown
//! `KNOWN_GAPS.md` document and the `glyim.known-gaps/1` JSON contract —
//! over a synthetic 500-gap registry, so CI logs can track the render
//! cost as the registry grows. Run with:
//!
//! ```text
//! cargo bench -p glyim-ledger --bench render
//! ```

use std::hint::black_box;
use std::path::PathBuf;
use std::time::Instant;

use ledger_schema::{GapClass, GapEntry, GapExpectation, GapStatus, Level, Sourced};

use glyim_ledger::{GapOutcome, JsonGapRenderer, KnownGapsRenderer, MarkdownGapRenderer};

/// Builds a synthetic 500-gap outcome: a witnessed class-a gap per even
/// index (with expectation + witness fixture) and an unwitnessed class-b
/// gap per odd index, sorted by ID (the order `run_gaps` establishes).
fn synthetic_outcome(count: usize) -> GapOutcome {
    let mut gaps: Vec<(String, GapEntry)> = Vec::with_capacity(count);
    for i in 0..count {
        let witnessed = i % 2 == 0;
        let entry = GapEntry {
            id: format!("GAP-{i:04}"),
            class: if witnessed { GapClass::A } else { GapClass::B },
            title: format!("bench gap {i}: synthetic drift subject"),
            affects: vec![format!("LANG-BENCH-{i}")],
            caps: Level::L2,
            witness: witnessed.then(|| format!("crates/glyim-test/tests/compile-fail/bench-{i}.g")),
            expect: witnessed.then(|| GapExpectation {
                diagnostic: "T0060".to_string(),
            }),
            opened_in: "v0.3.0".to_string(),
            target_milestone: None,
            prevention: Vec::new(),
            status: if witnessed {
                GapStatus::Witnessed
            } else {
                GapStatus::Unwitnessed
            },
        };
        gaps.push((entry.id.clone(), entry));
    }
    gaps.sort_by(|a, b| a.0.cmp(&b.0));
    let unwitnessed = gaps
        .iter()
        .filter(|(_, e)| matches!(e.status, GapStatus::Unwitnessed))
        .map(|(id, _)| id.clone())
        .collect();
    GapOutcome {
        diagnostics: Vec::new(),
        gaps: gaps
            .into_iter()
            .map(|(_, entry)| Sourced {
                path: PathBuf::from("ledger/gaps/known/bench.toml"),
                entry,
            })
            .collect(),
        unwitnessed,
    }
}

fn main() {
    let outcome = synthetic_outcome(500);
    assert_eq!(outcome.gaps.len(), 500);
    assert_eq!(outcome.unwitnessed.len(), 250);

    let started = Instant::now();
    let markdown = black_box(MarkdownGapRenderer.render(black_box(&outcome)));
    let markdown_elapsed = started.elapsed();
    assert!(markdown.contains("## GAP-0499"));
    assert!(markdown.contains(glyim_ledger::KNOWN_GAPS_MD_START));
    assert!(
        markdown
            .trim_end()
            .ends_with(glyim_ledger::KNOWN_GAPS_MD_END)
    );

    let started = Instant::now();
    let json = black_box(JsonGapRenderer.render(black_box(&outcome)));
    let json_elapsed = started.elapsed();
    assert!(json.contains("\"glyim.known-gaps/1\""));

    println!(
        "render bench: 500-gap registry (250 witnessed, 250 debt)\n  markdown : {markdown_elapsed:.1?} ({} bytes)\n  json     : {json_elapsed:.1?} ({} bytes)",
        markdown.len(),
        json.len(),
    );
}
