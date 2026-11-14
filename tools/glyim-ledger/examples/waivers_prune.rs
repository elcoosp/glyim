//! C9 consumer walkthrough: retire every expired waiver at once — the
//! exact validation and batch-rewrite path `glyim-ledger waivers prune
//! [ROOT]` runs — then inspect the committed document through the health
//! face. The batch counterpart of the `waivers_revoke` walkthrough: one
//! call clears the whole expired-inventory debt instead of one ID per
//! call.
//!
//! Run from the repository root:
//!
//! ```text
//! cargo run -p glyim-ledger --example waivers_prune
//! ```
//!
//! A real prune mutates the repository's `ledger/waivers.toml`, so the
//! walkthrough writes into a throwaway registry instead of the live one.
//! It seeds two valid and two expired waivers, refuses a non-clean
//! document (nothing written), prunes the expired half (a clean document
//! is never rewritten), and re-runs the health face after the prune. Try
//! the CLI on your own registry deliberately:
//!
//! ```text
//! cargo run -p glyim-ledger -- waivers        # list the live inventory
//! cargo run -p glyim-ledger -- waivers prune  # retire every expired block
//! ```

use std::path::PathBuf;

use glyim_ledger::{
    FileWaiverPruner, FileWaiverSource, WaiverEntry, format_waivers_prune_text, run_waivers,
    run_waivers_prune,
};

fn main() {
    // The prune face rewrites on success, so the walkthrough uses a
    // throwaway registry instead of the repository's live
    // `ledger/waivers.toml`.
    let dir = tempfile::tempdir().expect("walkthrough tmp");
    let root = PathBuf::from(dir.path());
    std::fs::create_dir_all(root.join("ledger")).expect("mkdir ledger");

    let entry = |id: &str, feature: &str, expires: &str| WaiverEntry {
        id: id.to_string(),
        feature: feature.to_string(),
        reason: "walkthrough exception".to_string(),
        issue: "https://github.com/elcoosp/glyim/issues/42".to_string(),
        expires: expires.to_string(),
    };
    let valid_a = entry("WVR-20261107-alpha", "LANG-ALPHA", "2026-11-20");
    let expired_a = entry("WVR-20261110-beta", "LANG-BETA", "2026-11-11");
    let valid_b = entry("WVR-20261112-delta", "LANG-DELTA", "2026-11-25");
    let expired_b = entry("WVR-20261105-gamma", "LANG-GAMMA", "2026-11-05");
    let mut seed = String::new();
    for waiver in [&valid_a, &expired_a, &valid_b, &expired_b] {
        use std::fmt::Write as _;
        let _ = write!(
            seed,
            "[[waiver]]\n\
             id = \"{}\"\n\
             feature = \"{}\"\n\
             reason = \"{}\"\n\
             issue = \"{}\"\n\
             expires = \"{}\"\n\n",
            waiver.id, waiver.feature, waiver.reason, waiver.issue, waiver.expires,
        );
    }
    let path = root.join("ledger/waivers.toml");
    std::fs::write(&path, &seed).expect("seed waivers");

    // Face 1: the health face — two expired waivers fail per C9 even when
    // no regression needs covering; the batch retirement path is how the
    // operator clears the whole debt at once.
    let before = run_waivers(&root, &FileWaiverSource);
    assert!(
        !before.is_ok(),
        "the seeded expired waivers fail the C9 gate"
    );
    println!(
        "before: {} valid, {} expired (C9 FAILED)",
        before.valid, before.expired
    );

    // Face 2: a non-clean document is refused — nothing is rewritten
    // (the refusal path is the typed `WaiversPruneLoad` error the CLI
    // reports with its help line).
    let broken = "{ not a waiver document";
    std::fs::write(&path, broken).expect("break the document");
    let refused = run_waivers_prune(&root, &FileWaiverSource, &FileWaiverPruner);
    assert!(
        refused.is_err(),
        "a non-clean document must not be rewritten"
    );
    println!("refused: the document does not load cleanly (nothing written)");
    std::fs::write(&path, &seed).expect("restore the document");

    // Face 3: prune the expired half — the canonical rewrite keeps the
    // valid entries, and the outcome rows report ID order.
    let pruned = run_waivers_prune(&root, &FileWaiverSource, &FileWaiverPruner)
        .expect("prune of a clean document succeeds");
    print!("{}", format_waivers_prune_text(&pruned));
    assert_eq!(pruned.removed.len(), 2, "both expired entries pruned");
    assert_eq!(
        pruned
            .removed
            .iter()
            .map(|row| row.id.as_str())
            .collect::<Vec<_>>(),
        ["WVR-20261105-gamma", "WVR-20261110-beta"],
        "outcome rows report ID order"
    );
    assert_eq!(pruned.remaining, 2);

    // Face 4: re-running the prune on the now-clean document is a no-op —
    // the seam is not invoked and the file bytes stay untouched.
    let bytes_before = std::fs::read_to_string(&path).expect("read back");
    let clean = run_waivers_prune(&root, &FileWaiverSource, &FileWaiverPruner)
        .expect("clean prune succeeds");
    assert!(clean.removed.is_empty(), "nothing expired: nothing pruned");
    assert_eq!(clean.remaining, 2);
    assert_eq!(
        std::fs::read_to_string(&path).expect("read back"),
        bytes_before,
        "a clean document is never rewritten"
    );

    // The health face now passes: the C9 debt is cleared.
    let after = run_waivers(&root, &FileWaiverSource);
    assert!(after.is_ok(), "no expired waiver: the C9 gate passes");
    println!(
        "after: {} valid, {} expired (C9 OK)",
        after.valid, after.expired
    );
}
