//! M2 consumer walkthrough: grant one C9 waiver — the exact validation
//! and write path `glyim-ledger waivers grant [ROOT]` runs — then inspect
//! the committed document through the health face.
//!
//! Run from the repository root:
//!
//! ```text
//! cargo run -p glyim-ledger --example waivers_grant
//! ```
//!
//! A real grant mutates the repository's `ledger/waivers.toml`, so the
//! walkthrough writes into a throwaway registry instead of the live one.
//! It first rejects a horizon violation (nothing is written), then grants
//! a valid waiver, then re-runs the health face over the committed
//! document. Try the CLI on your own registry deliberately:
//!
//! ```text
//! cargo run -p glyim-ledger -- waivers grant --feature LANG-X \
//!   --reason "fix in flight" \
//!   --issue https://github.com/elcoosp/glyim/issues/123
//! ```

use std::path::PathBuf;

use glyim_ledger::{
    FileWaiverSink, FileWaiverSource, JsonWaiversRenderer, WaiverEntry, WaiverSet,
    WaiversGrantRequest, WaiversRenderer, assess_waivers, format_waivers_grant_text, run_waivers,
    run_waivers_grant,
};

fn main() {
    // The grant face always writes, so the walkthrough uses a throwaway
    // registry instead of the repository's live `ledger/waivers.toml`.
    let dir = tempfile::tempdir().expect("walkthrough tmp");
    let root = PathBuf::from(dir.path());
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
         id = \"LANG-GRANT-DEMO\"\n\
         title = \"grant walkthrough feature\"\n\
         area = \"lang.control-flow\"\n\
         tier = \"core\"\n\
         spec = \"docs/specs/v0.1.0.md\"\n\
         target_level = \"L5\"\n\
         backends = [\"gam\"]\n\
         owner_lane = \"M\"\n",
    )
    .expect("write feature");

    // Face 1: the C9 horizon is enforced at grant time — 15 days exceeds
    // the 14-day cap and nothing is written.
    let too_long = WaiversGrantRequest {
        feature: "LANG-GRANT-DEMO".to_string(),
        reason: "a horizon violation must not land".to_string(),
        issue: "https://github.com/elcoosp/glyim/issues/42".to_string(),
        days: 15,
    };
    let rejected = run_waivers_grant(&root, &too_long, &FileWaiverSource, &FileWaiverSink);
    assert!(rejected.is_err(), "15 days exceed the C9 horizon");
    println!("rejected: 15 days exceed the 14-day C9 horizon (nothing written)");

    // Face 2: a valid grant — deterministic `WVR-<date>-<slug>` ID,
    // canonical `[[waiver]]` block appended to ledger/waivers.toml.
    let request = WaiversGrantRequest {
        feature: "LANG-GRANT-DEMO".to_string(),
        reason: "regressions covered while the fix lands".to_string(),
        issue: "https://github.com/elcoosp/glyim/issues/42".to_string(),
        days: 7,
    };
    let granted = run_waivers_grant(&root, &request, &FileWaiverSource, &FileWaiverSink)
        .expect("the valid grant must land");
    print!("{}", format_waivers_grant_text(&granted));

    // The same document, read back through the health face: the granted
    // entry is live and the C9 gate passes. (The in-memory set shows the
    // pure-function face of the same engine for comparison.)
    let outcome = run_waivers(&root, &FileWaiverSource);
    println!();
    println!(
        "health face: {} waiver(s), {} expired, gate passed: {}",
        outcome.waivers,
        outcome.expired,
        outcome.is_ok(),
    );
    let in_memory = WaiverSet {
        waivers: vec![WaiverEntry {
            id: granted.id.clone(),
            feature: granted.feature.clone(),
            reason: "the same grant, served through the in-memory seam".to_string(),
            issue: "https://github.com/elcoosp/glyim/issues/42".to_string(),
            expires: granted.expires.clone(),
        }],
        diagnostics: Vec::new(),
    };
    let assessed = assess_waivers(&in_memory, outcome.today);
    println!();
    println!("-- glyim.ledger-waivers/1 (first 6 lines) --");
    for line in JsonWaiversRenderer.render(&assessed).lines().take(6) {
        println!("{line}");
    }
}
