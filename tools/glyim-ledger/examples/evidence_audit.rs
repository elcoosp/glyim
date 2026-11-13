//! Evidence-store consumer walkthrough: audit the committed store against
//! the tree — the exact load, triage, and sha256-recompute path
//! `glyim-ledger evidence audit [ROOT]` runs.
//!
//! Run from the repository root:
//!
//! ```text
//! cargo run -p glyim-ledger --example evidence_audit
//! ```
//!
//! The walkthrough writes into a throwaway project: it commits a store
//! describing two real fixtures, then edits one and deletes the other, so
//! the audit reports the full verdict matrix (`ok`, `drift`, `missing`)
//! and fails the gate. Try the CLI on your own registry deliberately:
//!
//! ```text
//! cargo run -p glyim-ledger -- evidence audit
//! ```

use std::path::PathBuf;

use glyim_ledger::{
    EVIDENCE_AUDIT_SCHEMA, EvidenceOutcome, FileEvidenceStore, FixtureVerdict,
    format_evidence_audit_text, run_evidence_audit,
};

fn main() {
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
        root.join("ledger/features/audit.toml"),
        "[[feature]]\n\
         id = \"LANG-AUDIT-DEMO\"\n\
         title = \"audit demo feature\"\n\
         area = \"lang.control-flow\"\n\
         tier = \"core\"\n\
         spec = \"docs/specs/v0.1.0.md\"\n\
         target_level = \"L5\"\n\
         backends = [\"gam\"]\n\
         owner_lane = \"M\"\n",
    )
    .expect("write feature");

    // Two real fixtures, recorded with their true digests (the harness
    // emitter computes `fixture_sha256` over the fixture bytes).
    let fixtures = [
        ("alpha.g", "// alpha fixture: stays in sync\n"),
        ("bravo.g", "// bravo fixture: drifts away\n"),
    ];
    let mut records = Vec::new();
    for (name, body) in fixtures {
        std::fs::create_dir_all(root.join("tests")).expect("mkdir tests");
        std::fs::write(root.join("tests").join(name), body).expect("write fixture");
        records.push(glyim_ledger::LedgerEvidence {
            schema: "glyim.evidence/1".to_string(),
            name: name.trim_end_matches(".g").to_string(),
            fixture: format!("tests/{name}"),
            fixture_sha256: fixture_digest(body),
            revision: String::new(),
            mode: "compile-pass".to_string(),
            outcome: EvidenceOutcome::Passed,
            reason: None,
            backend: ledger_schema::Backend::Gam,
            opt: 0,
            features: vec!["LANG-AUDIT-DEMO".to_string()],
            declared_backends: vec!["gam".to_string()],
            instructions: None,
            rss_kb: None,
        });
    }
    let store = root.join("ledger/evidence/test.jsonl");
    FileEvidenceStore
        .save(&store, &records)
        .expect("commit the store");

    // Face 1: the store matches the tree — every verdict is `ok` and the
    // gate passes.
    let before = run_evidence_audit(
        &root,
        &glyim_ledger::ProjectEvidenceSource::new(&root, None),
        &glyim_ledger::FileFixtureFingerprint,
    );
    assert!(
        before.is_ok(),
        "both fixtures hash to their recorded digests"
    );
    println!(
        "before: {} record(s), all ok (audit passed)",
        before.records
    );

    // Face 2: the tree moves on — one fixture edited, one deleted — and
    // the store now lies about the world.
    std::fs::write(root.join("tests/alpha.g"), "// alpha fixture, edited\n")
        .expect("drift the fixture");
    std::fs::remove_file(root.join("tests/bravo.g")).expect("remove the fixture");
    let after = run_evidence_audit(
        &root,
        &glyim_ledger::ProjectEvidenceSource::new(&root, None),
        &glyim_ledger::FileFixtureFingerprint,
    );
    print!("{}", format_evidence_audit_text(&after));
    assert!(!after.is_ok(), "drift and missing both fail the gate");
    let verdicts: Vec<FixtureVerdict> = after.rows.iter().map(|row| row.verdict).collect();
    assert!(verdicts.contains(&FixtureVerdict::Drift));
    assert!(verdicts.contains(&FixtureVerdict::Missing));

    // The JSON contract rides on the same outcome (the serialization face
    // CI dashboards consume).
    let json = glyim_ledger::format_evidence_audit_json(&after);
    assert!(json.contains(&format!("\"schema\": \"{EVIDENCE_AUDIT_SCHEMA}\"")));
}

/// sha256 over the fixture bytes (the harness emitter's digest spelling).
fn fixture_digest(body: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(body.as_bytes());
    hasher
        .finalize()
        .iter()
        .fold(String::with_capacity(64), |mut out, byte| {
            use std::fmt::Write as _;
            let _ = write!(out, "{byte:02x}");
            out
        })
}
