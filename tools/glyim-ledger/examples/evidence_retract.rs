//! Evidence-store consumer walkthrough: retract one fixture's records —
//! the exact validation and canonical-rewrite path `glyim-ledger evidence
//! retract [ROOT] --fixture <PATH>` runs — then inspect the rewritten
//! store, the reloaded records, and the typed rejections along the way.
//!
//! Run from the repository root:
//!
//! ```text
//! cargo run -p glyim-ledger --example evidence_retract
//! ```
//!
//! A real retract rewrites the repository's committed evidence store, so
//! the walkthrough writes into a throwaway store instead of the live one.
//! It seeds a store with two fixtures' records (byte-derived digests, the
//! same spelling the harness emitter writes), shows the typed rejections
//! that write nothing (an unusable fixture path, a fixture with nothing
//! committed), retracts one fixture (its records are gone; the surviving
//! record is byte-identical in canonical form), and re-runs the check
//! face after the rewrite. The audit face is the finding (`Drift` when a
//! fixture's bytes change); this face is the remedy. Try the CLI on your
//! own store deliberately:
//!
//! ```text
//! cargo run -p glyim-ledger -- evidence audit .              # the finding
//! cargo run -p glyim-ledger -- evidence retract . --fixture crates/glyim-test/tests/run-pass/x.g
//! ```

use std::path::PathBuf;

use glyim_ledger::{
    EvidenceRetractRequest, EvidenceStore, FileEvidenceStore, FileFixtureFingerprint,
    FixtureFingerprint, LedgerEvidence, format_evidence_check_text, format_evidence_retract_text,
    run_evidence_check, run_evidence_retract,
};
use ledger_schema::Backend;

fn main() {
    // The retract face always rewrites, so the walkthrough uses a
    // throwaway store instead of the repository's live evidence files.
    let dir = tempfile::tempdir().expect("walkthrough tmp");
    let root = PathBuf::from(dir.path());
    let out = root.join("ledger/evidence/recorded.jsonl");
    std::fs::create_dir_all(out.parent().expect("mkdir")).expect("mkdir");
    std::fs::create_dir_all(root.join("tests")).expect("mkdir tests");
    // The check face validates the store against the registry, so the
    // walkthrough seeds the minimal project the record references.
    for (rel, contents) in [
        ("docs/specs/v0.1.0.md", "# spec fixture\n\n## evidence\n"),
        (
            "ledger/features/lang/a.toml",
            "[[feature]]\n\
             id = \"LANG-EVIDENCE-A\"\n\
             title = \"t LANG-EVIDENCE-A\"\n\
             area = \"lang.control-flow\"\n\
             tier = \"core\"\n\
             spec = \"docs/specs/v0.1.0.md\"\n\
             target_level = \"L5\"\n\
             backends = [\"gam\"]\n\
             owner_lane = \"M\"\n",
        ),
        (
            "ledger/policy.toml",
            "schema = \"glyim.ledger-policy/1\"\n\n\
             [levels.L1]\n\
             rules = [{ mode = \"compile-pass\", outcome = \"passed\", count = 1 }]\n\n\
             [gap_caps]\n\
             enabled = true\n",
        ),
    ] {
        let path = root.join(rel);
        std::fs::create_dir_all(path.parent().expect("mkdir")).expect("mkdir");
        std::fs::write(path, contents).expect("write seed");
    }

    // Seed two fixtures and one committed record each, with byte-derived
    // digests (the canonical `fixture_sha256` spelling).
    let mut records = Vec::new();
    for (name, body) in [("alpha.g", "// alpha\n"), ("bravo.g", "// bravo\n")] {
        let path = root.join("tests").join(name);
        std::fs::write(&path, body).expect("write fixture");
        let digest = FileFixtureFingerprint
            .sha256_hex(&path)
            .expect("a readable fixture digests");
        records.push(LedgerEvidence {
            schema: "glyim.evidence/1".to_string(),
            name: name.trim_end_matches(".g").to_string(),
            fixture: format!("tests/{name}"),
            fixture_sha256: digest,
            revision: String::new(),
            mode: "compile-pass".to_string(),
            outcome: glyim_ledger::EvidenceOutcome::Passed,
            reason: None,
            backend: Backend::Gam,
            opt: 0,
            features: vec!["LANG-EVIDENCE-A".to_string()],
            declared_backends: vec!["gam".to_string()],
            instructions: None,
            rss_kb: None,
        });
    }
    FileEvidenceStore
        .save(&out, &records)
        .expect("the seed store lands");
    println!(
        "seeded store: {} record(s) at {}",
        records.len(),
        out.display()
    );
    println!();

    // The typed rejections write nothing — the usable-path rule fires
    // before any digest is computed, and a fixture with nothing committed
    // is a rejection naming the digest, not a silent no-op.
    let charlie = root.join("tests/charlie.g");
    std::fs::write(&charlie, "// charlie\n").expect("write record-less fixture");
    for (label, request) in [
        (
            "unusable fixture path",
            EvidenceRetractRequest {
                fixture: "../escape.g".to_string(),
                out: out.clone(),
            },
        ),
        (
            "fixture with nothing committed",
            EvidenceRetractRequest {
                fixture: "tests/charlie.g".to_string(),
                out: out.clone(),
            },
        ),
    ] {
        if let Err(err) =
            run_evidence_retract(&root, &request, &FileEvidenceStore, &FileFixtureFingerprint)
        {
            println!("{label}:");
            println!("  {err}");
            if let Some(help) = miette::Diagnostic::help(&err) {
                println!("  help: {help}");
            }
            println!();
        }
    }

    // The happy path: retract tests/alpha.g. The engine recomputes the
    // digest from the fixture's bytes, partitions the store, and persists
    // the canonical remainder — bravo's record passes through untouched.
    let request = EvidenceRetractRequest {
        fixture: "tests/alpha.g".to_string(),
        out: out.clone(),
    };
    let outcome =
        run_evidence_retract(&root, &request, &FileEvidenceStore, &FileFixtureFingerprint)
            .expect("the retraction lands");
    print!("{}", format_evidence_retract_text(&outcome));
    println!();

    // The store is the source of truth: the reloaded store carries
    // exactly the surviving record, and the check face still passes.
    let reloaded = FileEvidenceStore.load(&out);
    println!(
        "reloaded store: {} record(s), alpha present: {}",
        reloaded.records.len(),
        reloaded
            .records
            .iter()
            .any(|record| record.fixture == "tests/alpha.g"),
    );
    let report = run_evidence_check(&root);
    print!("{}", format_evidence_check_text(&report));
}
