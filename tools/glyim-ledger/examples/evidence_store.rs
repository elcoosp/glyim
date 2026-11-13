//! §3.4 (evidence-store curation face) consumer walkthrough: curate a
//! harness-emitted JSONL run into the committed evidence store, prove the
//! merge is idempotent, then validate the store with the check face — the
//! same two faces `glyim-ledger evidence record` / `glyim-ledger evidence
//! check` drive.
//!
//! Run from the repository root:
//!
//! ```text
//! cargo run -p glyim-ledger --example evidence_store
//! ```
//!
//! The walkthrough uses a synthetic registry under a temporary directory
//! (two features plus the seed policy), so it never touches the
//! repository's own `ledger/` registry. An optional first argument names
//! a real project root to check instead: with one, the example skips the
//! synthetic curation and prints the committed store's check report.

use std::path::{Path, PathBuf};

use glyim_ledger::{
    FileEvidenceStore, LedgerEvidence, compute::EvidenceOutcome, run_evidence_check,
    run_evidence_record,
};
use ledger_schema::Backend;

/// Seed policy (mirrors `ledger/policy.toml`).
const POLICY: &str = r#"schema = "glyim.ledger-policy/1"

[levels.L1]
rules = [{ mode = "compile-pass", outcome = "passed", count = 1 }]

[levels.L2]
rules = [
    { mode = "compile-pass", outcome = "passed", count = 1 },
    { mode = "compile-fail", outcome = "passed", count = 1 },
]

[levels.L3]
rules = [{ mode = "run-pass", outcome = "passed", backend = "gam", count = 1 }]

[levels.L4]
rules = [{ mode = "run-pass", outcome = "passed", backend = "llvm", count = 1 }]

[gap_caps]
enabled = true

[hardening]
fuzz_cpu_hours = 24
mutation_kill_rate_pct = 75
"#;

/// One `glyim.evidence/1` record, the ledger-side mirror of what
/// `glyim-test` emits when `GLYIM_TEST_EVIDENCE` is set.
fn record(name: &str, mode: &str, outcome: EvidenceOutcome) -> LedgerEvidence {
    let seed = name.bytes().fold(0u64, |acc, byte| {
        acc.wrapping_mul(31).wrapping_add(u64::from(byte))
    });
    LedgerEvidence {
        schema: "glyim.evidence/1".to_string(),
        name: name.to_string(),
        fixture: format!("examples/{name}.g"),
        fixture_sha256: format!("{seed:064x}"),
        revision: String::new(),
        mode: mode.to_string(),
        outcome,
        reason: None,
        backend: Backend::Gam,
        opt: 0,
        features: vec!["LANG-EVIDENCE-A".to_string()],
        declared_backends: vec!["gam".to_string()],
        instructions: None,
        rss_kb: None,
    }
}

/// Writes the synthetic two-feature registry the walkthrough curates into.
fn write_registry(root: &Path) {
    let features = root.join("ledger/features");
    std::fs::create_dir_all(&features).expect("mkdir features");
    std::fs::create_dir_all(root.join("docs/specs")).expect("mkdir specs");
    std::fs::write(root.join("docs/specs/v0.1.0.md"), "# spec fixture\n").expect("write spec");
    std::fs::write(root.join("ledger/policy.toml"), POLICY).expect("write policy");
    std::fs::write(
        features.join("evidence.toml"),
        "[[feature]]\n\
         id = \"LANG-EVIDENCE-A\"\n\
         title = \"evidence walkthrough feature\"\n\
         area = \"lang.control-flow\"\n\
         tier = \"core\"\n\
         spec = \"docs/specs/v0.1.0.md\"\n\
         target_level = \"L5\"\n\
         backends = [\"gam\"]\n\
         owner_lane = \"M\"\n",
    )
    .expect("write features");
}

fn main() {
    let mut args = std::env::args().skip(1);
    let root = match args.next() {
        Some(path) => PathBuf::from(path),
        None => {
            let dir = tempfile::tempdir().expect("tmp");
            walkthrough(dir.path());
            if !run_evidence_check(dir.path()).is_ok() {
                std::process::exit(1);
            }
            return;
        }
    };
    // Real project root: the check face over the committed store.
    let report = run_evidence_check(&root);
    print!("{}", glyim_ledger::format_evidence_check_text(&report));
    if !report.is_ok() {
        std::process::exit(1);
    }
}

fn walkthrough(root: &Path) {
    write_registry(root);

    // A harness run (what `GLYIM_TEST_EVIDENCE` produces) lands in
    // target/: two faces for one fixture, one of them a duplicate of a
    // record already committed by an earlier run.
    let run = root.join("harness-run.jsonl");
    let records = [
        record("alpha", "compile-pass", EvidenceOutcome::Passed),
        record("bravo", "run-pass", EvidenceOutcome::Passed),
    ];
    std::fs::write(
        &run,
        records
            .iter()
            .map(|record| glyim_ledger::canonical_line(record) + "\n")
            .collect::<String>(),
    )
    .expect("write run");

    let store = root.join("ledger/evidence/recorded.jsonl");
    let outcome = run_evidence_record(root, std::slice::from_ref(&run), &store, &FileEvidenceStore)
        .expect("record run");
    print!("{}", glyim_ledger::format_evidence_record_text(&outcome));

    // Idempotence (plan C1/M1.13): re-recording the same run changes
    // nothing — the store is a set of records in canonical order.
    let committed = std::fs::read_to_string(&store).expect("read store");
    let rerun =
        run_evidence_record(root, &[run], &store, &FileEvidenceStore).expect("re-record run");
    assert_eq!(rerun.stats.merged, outcome.stats.merged, "idempotent merge");
    assert_eq!(
        std::fs::read_to_string(&store).expect("reread store"),
        committed,
        "re-record leaves the store byte-identical"
    );

    // The committed store feeds the engine with no explicit override:
    // `ledger compute` discovers `ledger/evidence/*.jsonl` by itself.
    let compute = glyim_ledger::run_compute(root, None);
    println!(
        "the store now drives compute: {} record(s) observed",
        compute.records
    );
}
