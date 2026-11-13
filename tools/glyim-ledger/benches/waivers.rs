//! Waiver bench (constitution C9 faces).
//!
//! Dependency-free (`harness = false`), mirroring the `render` bench:
//! measures the [`glyim_ledger::assess_waivers`] engine, both
//! [`glyim_ledger::WaiversRenderer`] faces — the text report and the
//! `glyim.ledger-waivers/1` JSON contract — the `waivers grant`
//! validation path and the `waivers revoke` retirement path, each over a
//! synthetic 500-waiver set, so CI logs can track the C9 face costs as
//! the exception inventory grows. Run with:
//!
//! ```text
//! cargo bench -p glyim-ledger --bench waivers
//! ```

use std::cell::RefCell;
use std::fmt::Write as _;
use std::hint::black_box;
use std::time::Instant;

use glyim_ledger::{
    FileWaiverSource, InMemoryWaiverRewriter, InMemoryWaiverSink, InMemoryWaiverSource,
    JsonWaiversRenderer, NaiveDate, TextWaiversRenderer, WaiverEntry, WaiverSet,
    WaiversGrantRequest, WaiversRenderer, WaiversRevokeRequest, assess_waivers, run_waivers,
    run_waivers_grant, run_waivers_revoke,
};

/// Builds a synthetic 500-waiver set: half inside the horizon (10 days
/// left), half expired by 1 day (the C9 failure half), IDs sorted (the
/// order `load_waivers` establishes).
fn synthetic_set(count: usize) -> WaiverSet {
    let today = NaiveDate::new_unchecked(2026, 11, 13);
    let mut waivers: Vec<WaiverEntry> = (0..count)
        .map(|i| {
            let days = if i % 2 == 0 { 10 } else { -1 };
            WaiverEntry {
                id: format!("WVR-{i:04}"),
                feature: format!("LANG-BENCH-{i}"),
                reason: format!("bench waiver {i}: synthetic exception"),
                issue: "https://github.com/elcoosp/glyim/issues/42".to_string(),
                expires: today.plus_days(days).to_iso(),
            }
        })
        .collect();
    waivers.sort_by(|a, b| a.id.cmp(&b.id));
    WaiverSet {
        waivers,
        diagnostics: Vec::new(),
    }
}

fn main() {
    let today = NaiveDate::new_unchecked(2026, 11, 13);
    let set = synthetic_set(500);
    assert_eq!(set.waivers.len(), 500);

    let started = Instant::now();
    let outcome = black_box(assess_waivers(black_box(&set), black_box(today)));
    let assess_elapsed = started.elapsed();
    assert_eq!(outcome.waivers, 500);
    assert_eq!(outcome.valid, 250);
    assert_eq!(outcome.expired, 250);
    assert!(!outcome.is_ok(), "half the bench set is expired (C9)");

    let started = Instant::now();
    let text = black_box(TextWaiversRenderer.render(black_box(&outcome)));
    let text_elapsed = started.elapsed();
    assert!(text.contains("WVR-0499"));
    assert!(text.contains("expired"));

    let started = Instant::now();
    let json = black_box(JsonWaiversRenderer.render(black_box(&outcome)));
    let json_elapsed = started.elapsed();
    assert!(json.contains("\"glyim.ledger-waivers/1\""));

    // The source seam adds one clone per load; measure it so the run
    // pipeline (source + assess + render) stays visible in CI logs.
    let source = InMemoryWaiverSource { set: set.clone() };
    let started = Instant::now();
    let outcome = black_box(run_waivers(
        black_box(std::path::Path::new(".")),
        black_box(&source),
    ));
    let run_elapsed = started.elapsed();
    assert_eq!(outcome.waivers, 500);

    // The grant face: the full validation path (registry load, C9 horizon
    // and duplicate checks over the loaded 500-entry set, deterministic
    // ID, canonical fragment) with the append captured by the in-memory
    // sink, so the measured cost is the validation work, not the fs write.
    let dir = tempfile::tempdir().expect("bench tmp");
    let root = dir.path();
    std::fs::create_dir_all(root.join("ledger/features")).expect("mkdir features");
    std::fs::create_dir_all(root.join("docs/specs")).expect("mkdir specs");
    std::fs::write(
        root.join("docs/specs/v0.1.0.md"),
        "# spec fixture\n\n## evidence\n",
    )
    .expect("write spec");
    std::fs::write(
        root.join("ledger/policy.toml"),
        "[levels]\nL1 = { min_pass = 1, require_evidence = false }\n[gap_caps]\nenabled = true\n[hardening]\nfuzz_cpu_hours = 24\nmutation_kill_rate_pct = 75\n",
    )
    .expect("write policy");
    std::fs::write(
        root.join("ledger/features/grant.toml"),
        "[[feature]]\n\
         id = \"LANG-BENCH-1\"\n\
         title = \"grant bench feature\"\n\
         area = \"lang.control-flow\"\n\
         tier = \"core\"\n\
         spec = \"docs/specs/v0.1.0.md\"\n\
         target_level = \"L5\"\n\
         backends = [\"gam\"]\n\
         owner_lane = \"M\"\n",
    )
    .expect("write feature");
    // Seed the committed document with the synthetic set so the duplicate
    // scan runs over 500 entries (the worst case the health face sees).
    let seeded = synthetic_set(500);
    let mut seed_text = String::new();
    for waiver in &seeded.waivers {
        let _ = write!(
            seed_text,
            "[[waiver]]\n\
             id = \"{}\"\n\
             feature = \"{}\"\n\
             reason = \"{}\"\n\
             issue = \"{}\"\n\
             expires = \"{}\"\n\n",
            waiver.id, waiver.feature, waiver.reason, waiver.issue, waiver.expires,
        );
    }
    std::fs::create_dir_all(root.join("ledger")).expect("mkdir ledger");
    std::fs::write(root.join("ledger/waivers.toml"), &seed_text).expect("seed waivers");

    let grant = WaiversGrantRequest {
        feature: "LANG-BENCH-1".to_string(),
        reason: "bench grant: validation-path measurement".to_string(),
        issue: "https://github.com/elcoosp/glyim/issues/42".to_string(),
        days: 7,
    };
    let sink = InMemoryWaiverSink::default();
    let started = Instant::now();
    let granted = black_box(run_waivers_grant(
        black_box(root),
        black_box(&grant),
        black_box(&FileWaiverSource),
        black_box(&sink),
    ))
    .expect("the bench grant must pass");
    let grant_elapsed = started.elapsed();
    assert!(granted.id.starts_with("WVR-"));
    assert_eq!(sink.written.borrow().len(), 1);

    // The revoke face: the full retirement path (document load with the
    // duplicate scan over the seeded 500-entry set, exact-ID match,
    // canonical rewrite of the remaining entries) with the rewrite
    // captured by the in-memory rewriter, so the measured cost is the
    // validation work, not the fs write.
    let rewriter = InMemoryWaiverRewriter {
        doc: RefCell::new(set.waivers.clone()),
        removed: RefCell::new(Vec::new()),
    };
    let revoke = WaiversRevokeRequest {
        id: "WVR-0000".to_string(),
    };
    let started = Instant::now();
    let revoked = black_box(run_waivers_revoke(
        black_box(root),
        black_box(&revoke),
        black_box(&FileWaiverSource),
        black_box(&rewriter),
    ))
    .expect("the bench revoke must pass");
    let revoke_elapsed = started.elapsed();
    assert_eq!(revoked.id, "WVR-0000");
    // The committed document carries the 500 seeded entries (the grant
    // face above appended to the in-memory sink); the rewriter removes
    // exactly one.
    assert_eq!(revoked.remaining, 499);
    assert_eq!(rewriter.removed.borrow().len(), 1);

    println!(
        "waivers bench: 500-waiver set (250 valid, 250 expired)\n  assess    : {assess_elapsed:.1?}\n  text face : {text_elapsed:.1?} ({} bytes)\n  json face : {json_elapsed:.1?} ({} bytes)\n  run (clone+assess+debug span): {run_elapsed:.1?}\n  grant (registry+horizon+dup scan, in-memory sink): {grant_elapsed:.1?}\n  revoke (load+exact-ID scan, in-memory rewrite): {revoke_elapsed:.1?}",
        text.len(),
        json.len(),
    );
}
