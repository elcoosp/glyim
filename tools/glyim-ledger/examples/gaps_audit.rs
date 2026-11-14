//! Gap-registry consumer walkthrough: audit every registered gap's
//! witness fixture against the tree — the exact load, resolve, and
//! metadata-probe path `glyim-ledger gaps audit [ROOT]` runs — then
//! inspect the verdict matrix and the typed gate outcome along the way.
//!
//! Run from the repository root:
//!
//! ```text
//! cargo run -p glyim-ledger --example gaps_audit
//! ```
//!
//! The walkthrough writes into a throwaway registry instead of the live
//! one: it seeds one witnessed gap whose fixture exists, one honest
//! witness debt, and one witnessed gap whose fixture the tree no longer
//! carries — so the audit reports the full verdict matrix (`witnessed`,
//! `debt`, `missing`) and fails the gate exactly the way the CI face
//! would. Try the CLI on your own registry deliberately:
//!
//! ```text
//! cargo run -p glyim-ledger -- gaps audit                # the text face
//! cargo run -p glyim-ledger -- gaps audit --format json  # the contract
//! ```

use std::path::PathBuf;

use glyim_ledger::{
    GapAuditVerdict, format_gaps_audit_json, format_gaps_audit_text, run_gaps_audit,
};

fn main() {
    // The audit face only reads, but the walkthrough still uses a
    // throwaway registry: seeding a missing witness means writing a
    // registry that claims a fixture the tree does not carry.
    let dir = tempfile::tempdir().expect("walkthrough tmp");
    let root = PathBuf::from(dir.path());
    for (rel, contents) in [
        ("docs/specs/v0.1.0.md", "# spec fixture\n"),
        (
            "ledger/features/lang/a.toml",
            "[[feature]]\n\
             id = \"LANG-DEMO-AWAIT\"\n\
             title = \"t LANG-DEMO-AWAIT\"\n\
             area = \"lang.control-flow\"\n\
             tier = \"core\"\n\
             spec = \"docs/specs/v0.1.0.md\"\n\
             target_level = \"L5\"\n\
             backends = [\"gam\", \"llvm\"]\n\
             owner_lane = \"M\"\n",
        ),
        (
            "ledger/gaps/known/0001-seed.toml",
            "# Curated header prose the audit face never rewrites.\n\
             \n\
             [[gap]]\n\
             id        = \"GAP-0001\"\n\
             class     = \"a\"\n\
             title     = \".await inside a loop body rejected (T0060)\"\n\
             affects   = [\"LANG-DEMO-AWAIT\"]\n\
             caps      = \"L2\"\n\
             opened_in = \"v0.3.0\"\n\
             status    = \"witnessed\"\n\
             witness   = \"fixtures/await-loop-known-gap.g\"\n\
             \n\
             [[gap]]\n\
             id        = \"GAP-0002\"\n\
             class     = \"b\"\n\
             title     = \"Windows SEH cleanup edges are unreachable\"\n\
             affects   = [\"LANG-DEMO-AWAIT\"]\n\
             caps      = \"L2\"\n\
             opened_in = \"v0.3.0\"\n\
             status    = \"unwitnessed\"\n\
             \n\
             [[gap]]\n\
             id        = \"GAP-0003\"\n\
             class     = \"c\"\n\
             title     = \"needs_drop is conservative for dyn Trait\"\n\
             affects   = [\"LANG-DEMO-AWAIT\"]\n\
             caps      = \"L2\"\n\
             opened_in = \"v0.3.0\"\n\
             status    = \"witnessed\"\n\
             witness   = \"fixtures/renamed-away-known-gap.g\"\n",
        ),
        (
            "fixtures/await-loop-known-gap.g",
            "//@ known-gap: GAP-0001\nfn main() {}\n",
        ),
    ] {
        let path = root.join(rel);
        std::fs::create_dir_all(path.parent().expect("mkdir")).expect("mkdir");
        std::fs::write(path, contents).expect("write seed");
    }

    // The audit pass: one load, one metadata probe per gap, rows sorted
    // by ID. GAP-0001's fixture exists (`witnessed`), GAP-0002 is honest
    // witness debt (`debt` — displayed, never failed), and GAP-0003
    // claims a fixture the tree no longer carries (`missing` — silent
    // debt surfaced, the gate fails).
    let outcome = run_gaps_audit(&root);
    print!("{}", format_gaps_audit_text(&outcome));
    println!();

    // The row shape: every verdict is inspectable programmatically, with
    // the detail line naming the vanished path.
    for row in &outcome.rows {
        println!(
            "{} [{:>10}] {}",
            row.id,
            row.verdict.as_str(),
            row.detail.as_deref().unwrap_or(row.title.as_str()),
        );
    }
    println!();
    let missing = outcome
        .rows
        .iter()
        .find(|row| row.verdict == GapAuditVerdict::Missing)
        .expect("the seeded missing row is present");
    println!(
        "gate: passed={} (errors={}, missing={}, unreadable={})",
        outcome.is_ok(),
        outcome.errors(),
        outcome.missing,
        outcome.unreadable,
    );
    println!(
        "remedy: `glyim-ledger gaps unwitness --id {}` detaches the stale \
         witness (restores honest debt), then re-witness the moved fixture",
        missing.id,
    );
    println!();

    // The JSON face: the `glyim.ledger-gaps-audit/1` contract a CI gate
    // or an agent consumer parses back.
    print!("{}", format_gaps_audit_json(&outcome));
}
