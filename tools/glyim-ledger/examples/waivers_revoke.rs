//! C9 consumer walkthrough: retire one waiver — the exact validation and
//! rewrite path `glyim-ledger waivers revoke [ROOT] --id <WVR-ID>` runs —
//! then inspect the committed document through the health face.
//!
//! Run from the repository root:
//!
//! ```text
//! cargo run -p glyim-ledger --example waivers_revoke
//! ```
//!
//! A real revoke mutates the repository's `ledger/waivers.toml`, so the
//! walkthrough writes into a throwaway registry instead of the live one.
//! It seeds one valid and one expired waiver, revokes them by exact ID
//! (rejecting an unknown ID with nothing written along the way), and
//! re-runs the health face after each removal. Try the CLI on your own
//! registry deliberately:
//!
//! ```text
//! cargo run -p glyim-ledger -- waivers              # list the live IDs
//! cargo run -p glyim-ledger -- waivers revoke --id WVR-20261113-lang-demo
//! ```

use std::path::PathBuf;

use glyim_ledger::{
    FileWaiverRewriter, FileWaiverSource, JsonWaiversRenderer, WaiverEntry, WaiversRenderer,
    WaiversRevokeRequest, format_waivers_revoke_text, run_waivers, run_waivers_revoke,
};

fn main() {
    // The revoke face always rewrites, so the walkthrough uses a
    // throwaway registry instead of the repository's live
    // `ledger/waivers.toml`.
    let dir = tempfile::tempdir().expect("walkthrough tmp");
    let root = PathBuf::from(dir.path());
    std::fs::create_dir_all(root.join("ledger")).expect("mkdir ledger");

    let valid = WaiverEntry {
        id: "WVR-20261113-lang-demo".to_string(),
        feature: "LANG-DEMO".to_string(),
        reason: "regressions covered while the fix lands".to_string(),
        issue: "https://github.com/elcoosp/glyim/issues/42".to_string(),
        expires: "2026-11-20".to_string(),
    };
    let expired = WaiverEntry {
        id: "WVR-20261105-lang-old".to_string(),
        feature: "LANG-OLD".to_string(),
        reason: "superseded by the landed fix".to_string(),
        issue: "https://github.com/elcoosp/glyim/issues/41".to_string(),
        expires: "2026-11-01".to_string(),
    };
    let seed = format!(
        "[[waiver]]\n\
         id = \"{}\"\n\
         feature = \"{}\"\n\
         reason = \"{}\"\n\
         issue = \"{}\"\n\
         expires = \"{}\"\n\n\
         [[waiver]]\n\
         id = \"{}\"\n\
         feature = \"{}\"\n\
         reason = \"{}\"\n\
         issue = \"{}\"\n\
         expires = \"{}\"\n",
        valid.id,
        valid.feature,
        valid.reason,
        valid.issue,
        valid.expires,
        expired.id,
        expired.feature,
        expired.reason,
        expired.issue,
        expired.expires,
    );
    std::fs::write(root.join("ledger/waivers.toml"), &seed).expect("seed waivers");

    // Face 1: the health face — an expired waiver fails per C9 even when
    // no regression needs covering; the retirement path is how the
    // operator clears it.
    let before = run_waivers(&root, &FileWaiverSource);
    assert!(
        !before.is_ok(),
        "the seeded expired waiver fails the C9 gate"
    );
    println!(
        "before: {} valid, {} expired (C9 FAILED)",
        before.valid, before.expired
    );

    // Face 2: an unknown ID is a typed rejection — nothing is rewritten.
    let absent = WaiversRevokeRequest {
        id: "WVR-20261201-absent".to_string(),
    };
    let rejected = run_waivers_revoke(&root, &absent, &FileWaiverSource, &FileWaiverRewriter);
    assert!(rejected.is_err(), "an unknown ID must not rewrite anything");
    println!("rejected: WVR-20261201-absent is not in the document (nothing written)");

    // Face 3: revoke the expired waiver by its exact ID — the canonical
    // rewrite keeps the remaining entry byte-identical.
    let revoked = run_waivers_revoke(
        &root,
        &WaiversRevokeRequest {
            id: expired.id.clone(),
        },
        &FileWaiverSource,
        &FileWaiverRewriter,
    )
    .expect("revoke of a present ID succeeds");
    print!("{}", format_waivers_revoke_text(&revoked));
    assert_eq!(revoked.remaining, 1);

    // Face 4: revoke the valid waiver, leaving an empty document — the
    // health face now passes.
    let last = run_waivers_revoke(
        &root,
        &WaiversRevokeRequest {
            id: valid.id.clone(),
        },
        &FileWaiverSource,
        &FileWaiverRewriter,
    )
    .expect("revoke of the last ID succeeds");
    assert_eq!(last.remaining, 0);
    let after = run_waivers(&root, &FileWaiverSource);
    assert!(
        after.is_ok(),
        "an empty waiver inventory passes the C9 gate"
    );
    println!(
        "after: {} valid, {} expired (C9 OK)",
        after.valid, after.expired
    );

    // The JSON contract rides on the same outcome (the serialization
    // face CI dashboards consume).
    let json = JsonWaiversRenderer.render(&before);
    assert!(json.contains("\"glyim.ledger-waivers/1\""));
}
