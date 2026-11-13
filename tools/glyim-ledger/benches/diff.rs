//! Readiness-diff bench (plan M1.10 first slice).
//!
//! Dependency-free (`harness = false`), mirroring the `status` bench:
//! measures the pure diff engine plus both renderers over two synthetic
//! 500-feature snapshots (a third of the features move down, a third up,
//! a third stay), so CI logs can track the gate cost as the registry
//! grows. Run with:
//!
//! ```text
//! cargo bench -p glyim-ledger --bench diff
//! ```

use std::hint::black_box;
use std::time::Instant;

use glyim_ledger::{
    ComputeSnapshot, LevelVerdict, NaiveDate, SnapshotEntry, WaiverEntry, WaiverSet, diff_levels,
    format_diff_json, format_diff_text,
};
use ledger_schema::{Level, Tier};

/// Builds a synthetic snapshot with `count` features at `level`.
fn snapshot(count: usize, schema: &str, level: Level) -> ComputeSnapshot {
    ComputeSnapshot {
        schema: schema.to_string(),
        entries: (0..count)
            .map(|i| SnapshotEntry {
                id: format!("LANG-BENCH-{i}"),
                tier: Tier::Core,
                target_level: Level::L5,
                computed_level: level,
                capped_by: Vec::new(),
            })
            .collect(),
    }
}

fn main() {
    let count = 500;
    let baseline = snapshot(count, glyim_ledger::SNAPSHOT_SCHEMA, Level::L3);
    // Head: the first third drops one level (regressions), the second
    // third rises one (improvements), the rest are unchanged.
    let mut head = snapshot(count, glyim_ledger::SNAPSHOT_SCHEMA, Level::L3);
    for (index, entry) in head.entries.iter_mut().enumerate() {
        if index < count / 3 {
            entry.computed_level = Level::L2;
        } else if index < 2 * count / 3 {
            entry.computed_level = Level::L4;
        }
    }

    let today = NaiveDate::new_unchecked(2026, 11, 12);
    // One waiver per regressed feature (all unexpired) so the waiver-match
    // path runs at the same scale as the diff itself.
    let waivers = WaiverSet {
        waivers: head.entries[..count / 3]
            .iter()
            .map(|entry| WaiverEntry {
                id: format!("W-{}", entry.id),
                feature: entry.id.clone(),
                reason: "bench waiver".to_string(),
                issue: "https://example.com/issue/1".to_string(),
                expires: today.plus_days(7).to_iso(),
            })
            .collect(),
        diagnostics: Vec::new(),
    };

    let started = Instant::now();
    let outcome = black_box(diff_levels(
        black_box(&baseline),
        black_box(&head),
        &waivers,
        today,
    ));
    let diff_elapsed = started.elapsed();

    let started = Instant::now();
    let text = black_box(format_diff_text(black_box(&outcome)));
    let text_elapsed = started.elapsed();

    let started = Instant::now();
    let json = black_box(format_diff_json(black_box(&outcome)));
    let json_elapsed = started.elapsed();

    assert_eq!(outcome.diffs.len(), count);
    // Every regression is waived by its bench waiver, so the gate passes.
    assert!(
        outcome.is_ok(),
        "bench waivers must cover the seeded regressions"
    );
    assert_eq!(
        outcome
            .diffs
            .iter()
            .filter(|d| d.verdict == LevelVerdict::Regressed)
            .count(),
        count / 3
    );
    assert!(text.contains("gate: PASSED"), "{text}");
    assert!(
        json.contains("\"schema\": \"glyim.ledger-diff/1\""),
        "{json}"
    );
    // Determinism: two renders are byte-identical (plan C1/M1.13).
    assert_eq!(text, format_diff_text(&outcome));
    assert_eq!(json, format_diff_json(&outcome));

    println!(
        "ledger diff bench: {count} features, {} regressed, {} waived",
        count / 3,
        count / 3
    );
    println!("  diff: {diff_elapsed:?}");
    println!("  text: {text_elapsed:?}");
    println!("  json: {json_elapsed:?}");
}
