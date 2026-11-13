//! C9 consumer walkthrough: re-pin one waiver's expiry — the exact
//! validation and rewrite path `glyim-ledger waivers extend [ROOT] --id
//! <WVR-ID> [--days N]` runs — then inspect the committed document
//! through the health face.
//!
//! Run from the repository root:
//!
//! ```text
//! cargo run -p glyim-ledger --example waivers_extend
//! ```
//!
//! A real extend mutates the repository's `ledger/waivers.toml`, so the
//! walkthrough writes into a throwaway registry instead of the live one.
//! It seeds one waiver about to expire, re-pins its expiry (the
//! sanctioned counterpart of a duplicate re-grant), rejects an unknown
//! ID and an out-of-horizon request with nothing written along the way,
//! and re-runs the health face after each mutation. Try the CLI on your
//! own registry deliberately:
//!
//! ```text
//! cargo run -p glyim-ledger -- waivers               # list the live IDs
//! cargo run -p glyim-ledger -- waivers extend --id WVR-20261113-lang-demo --days 7
//! ```

use std::path::PathBuf;

use glyim_ledger::{
    FileWaiverExtender, FileWaiverSource, JsonWaiversRenderer, WaiverEntry, WaiversExtendRequest,
    WaiversRenderer, WaiversRevokeRequest, format_waivers_extend_text, run_waivers,
    run_waivers_extend, run_waivers_revoke,
};

fn main() {
    // The extend face always rewrites, so the walkthrough uses a
    // throwaway registry instead of the repository's live
    // `ledger/waivers.toml`.
    let dir = tempfile::tempdir().expect("walkthrough tmp");
    let root = PathBuf::from(dir.path());
    std::fs::create_dir_all(root.join("ledger")).expect("mkdir ledger");

    let expiring = WaiverEntry {
        id: "WVR-20261113-lang-demo".to_string(),
        feature: "LANG-DEMO".to_string(),
        reason: "regressions covered while the fix lands".to_string(),
        issue: "https://github.com/elcoosp/glyim/issues/42".to_string(),
        expires: "2026-11-14".to_string(),
    };
    let seed = format!(
        "[[waiver]]\n\
         id = \"{}\"\n\
         feature = \"{}\"\n\
         reason = \"{}\"\n\
         issue = \"{}\"\n\
         expires = \"{}\"\n",
        expiring.id, expiring.feature, expiring.reason, expiring.issue, expiring.expires,
    );
    std::fs::write(root.join("ledger/waivers.toml"), &seed).expect("seed waivers");

    // Face 1: the health face — the seeded waiver still passes today (one
    // day left), but the remaining-days column is the operator's
    // reminder that a re-pin is due.
    let before = run_waivers(&root, &FileWaiverSource);
    assert!(before.is_ok(), "one valid waiver passes the C9 gate");
    println!(
        "before: {} valid, {} expired (C9 OK, {} day(s) left)",
        before.valid, before.expired, before.rows[0].remaining_days
    );

    // Face 2: an unknown ID is a typed rejection — nothing is rewritten.
    let absent = WaiversExtendRequest {
        id: "WVR-20261201-absent".to_string(),
        days: 7,
    };
    let rejected = run_waivers_extend(&root, &absent, &FileWaiverSource, &FileWaiverExtender);
    assert!(rejected.is_err(), "an unknown ID must not rewrite anything");
    println!("rejected: WVR-20261201-absent is not in the document (nothing written)");

    // Face 3: an out-of-horizon request is a typed rejection too — C9
    // caps the re-pin at 14 days from today, exactly like a grant.
    let horizon = WaiversExtendRequest {
        id: expiring.id.clone(),
        days: 15,
    };
    let rejected = run_waivers_extend(&root, &horizon, &FileWaiverSource, &FileWaiverExtender);
    assert!(
        rejected.is_err(),
        "a 15-day re-pin exceeds the C9 horizon and must not rewrite"
    );
    println!("rejected: --days 15 exceeds the 14-day C9 horizon (nothing written)");

    // Face 4: extend the waiver by its exact ID — the identity, reason,
    // and issue survive; only the expiry moves.
    let extended = run_waivers_extend(
        &root,
        &WaiversExtendRequest {
            id: expiring.id.clone(),
            days: 7,
        },
        &FileWaiverSource,
        &FileWaiverExtender,
    )
    .expect("extend of a present ID succeeds");
    print!("{}", format_waivers_extend_text(&extended));
    assert_eq!(extended.id, expiring.id);
    assert_eq!(extended.previous_expires, expiring.expires);
    assert_eq!(extended.remaining, 1);

    // Face 5: the health face now reports the fresh horizon, and the
    // JSON contract rides on the same outcome (the serialization face
    // CI dashboards consume).
    let after = run_waivers(&root, &FileWaiverSource);
    assert!(
        after.is_ok() && after.rows[0].remaining_days == 7,
        "the re-pinned waiver carries the full 7-day horizon"
    );
    println!(
        "after: {} valid, {} expired (C9 OK, {} day(s) left)",
        after.valid, after.expired, after.rows[0].remaining_days
    );
    let json = JsonWaiversRenderer.render(&after);
    assert!(json.contains("\"glyim.ledger-waivers/1\""));

    // Face 6: retirement stays the way out — extend covers a fix that
    // needs more time; revoke retires it once the fix lands.
    let revoked = run_waivers_revoke(
        &root,
        &WaiversRevokeRequest {
            id: expiring.id.clone(),
        },
        &FileWaiverSource,
        &glyim_ledger::FileWaiverRewriter,
    )
    .expect("revoke of the extended ID succeeds");
    assert_eq!(revoked.remaining, 0);
    println!(
        "retired: {} (the full grant → extend → revoke loop)",
        revoked.id
    );
}
