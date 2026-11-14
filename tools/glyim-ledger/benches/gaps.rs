//! Gaps face bench (`gaps open`, `gaps witness`, `gaps retire`, `gaps audit`).
//!
//! Dependency-free (`harness = false`), mirroring the `release` bench:
//! measures the [`glyim_ledger::run_gaps_witness`] engine over a seeded
//! tempdir registry — every run is a full load pass (registry parse +
//! gap lookup + fixture existence check) plus one surgical block edit —
//! witnessing `GAPS` registry gaps in sequence, both formatter faces
//! (the text one-liner and the `glyim.ledger-gaps-witness/1` JSON
//! contract), the in-memory twin of the rewrite seam, the retire face's
//! whole-block removal path over a fresh registry, and the open face's
//! canonical-append path (load pass + duplicate-ID guard + affects
//! cross-reference + one numbered file minted per gap), the audit
//! face's trust-loop pass over a witnessed registry (once with every
//! fixture present, once with the fixture tree removed), and the
//! unwitness face's reverse surgical edit over a witnessed registry, so
//! CI logs can track the face cost as the gap registry grows. Run with:
//!
//! ```text
//! cargo bench -p glyim-ledger --bench gaps
//! ```

use std::fmt::Write as _;
use std::hint::black_box;
use std::path::Path;
use std::time::Instant;

use glyim_ledger::{
    FileGapOpener, FileGapRetirer, FileGapWitnessDetacher, FileGapWitnessRewriter, GapsOpenOutcome,
    GapsOpenRequest, GapsRetireOutcome, GapsRetireRequest, GapsUnwitnessOutcome,
    GapsUnwitnessRequest, GapsWitnessOutcome, GapsWitnessRequest, InMemoryGapWitnessRewriter,
    format_gaps_audit_json, format_gaps_audit_text, format_gaps_open_json, format_gaps_open_text,
    format_gaps_retire_json, format_gaps_retire_text, format_gaps_unwitness_json,
    format_gaps_unwitness_text, format_gaps_witness_json, format_gaps_witness_text, run_gaps_audit,
    run_gaps_open, run_gaps_retire, run_gaps_unwitness, run_gaps_witness,
};

/// How many registry gaps the bench witnesses (and fixtures it seeds).
const GAPS: usize = 40;

/// A curated registry file in the committed `ledger/gaps/known/` style:
/// header prose the rewriter must preserve, plus `gaps` unwitnessed
/// `[[gap]]` blocks.
fn curated_registry(gaps: usize) -> String {
    let mut text = String::from(
        "# Curated header prose (bench seed) the rewriter must preserve\n\
         # byte-for-byte: witness debt stays honest until a fixture\n\
         # reproduces each gap.\n\n",
    );
    for i in 0..gaps {
        let _ = write!(
            &mut text,
            "[[gap]]\n\
             id        = \"GAP-{i:04}\"\n\
             class     = \"b\"\n\
             title     = \"bench gap {i}: no lowering site sets cleanup\"\n\
             affects   = [\"LANG-BENCH-CLEANUP\"]\n\
             caps      = \"L2\"\n\
             opened_in = \"v0.3.0\"\n\
             status    = \"unwitnessed\"\n\n"
        );
    }
    text
}

/// Seeds a tempdir project: two features, the spec they resolve against,
/// the curated gap registry, and one fixture file per gap (the engine
/// refuses a phantom witness path).
fn seed(root: &Path) {
    std::fs::create_dir_all(root.join("ledger/gaps/known")).expect("mkdir ledger");
    std::fs::create_dir_all(root.join("ledger/features/lang")).expect("mkdir features");
    std::fs::create_dir_all(root.join("docs/specs")).expect("mkdir specs");
    std::fs::create_dir_all(root.join("fixtures")).expect("mkdir fixtures");
    std::fs::write(root.join("docs/specs/v0.1.0.md"), "# spec fixture\n").expect("write spec");
    std::fs::write(
        root.join("ledger/features/lang/a.toml"),
        "[[feature]]\n\
         id = \"LANG-BENCH-CLEANUP\"\n\
         title = \"t LANG-BENCH-CLEANUP\"\n\
         area = \"lang.control-flow\"\n\
         tier = \"core\"\n\
         spec = \"docs/specs/v0.1.0.md\"\n\
         target_level = \"L5\"\n\
         backends = [\"gam\", \"llvm\"]\n\
         owner_lane = \"M\"\n",
    )
    .expect("write features");
    std::fs::write(
        root.join("ledger/gaps/known/0001-bench.toml"),
        curated_registry(GAPS),
    )
    .expect("write gaps");
    for i in 0..GAPS {
        std::fs::write(
            root.join(format!("fixtures/gap-{i:04}.g")),
            format!("//@ known-gap: GAP-{i:04}\nfn main() {{}}\n"),
        )
        .expect("write fixture");
    }
}

/// Witnesses every seeded gap through the full file path.
fn witness_all(root: &Path) -> GapsWitnessOutcome {
    let mut last = None;
    for i in 0..GAPS {
        let request = GapsWitnessRequest {
            id: format!("GAP-{i:04}"),
            witness: format!("fixtures/gap-{i:04}.g"),
        };
        last = Some(
            run_gaps_witness(
                black_box(root),
                black_box(&request),
                &FileGapWitnessRewriter,
            )
            .unwrap_or_else(|err| panic!("bench witness GAP-{i:04} failed: {err}")),
        );
    }
    last.expect("at least one gap witnessed")
}

fn main() {
    // The file path: every run re-loads the registry, resolves the gap,
    // checks the fixture, and surgically edits the committed file.
    let dir = tempfile::tempdir().expect("bench tmp");
    let root = dir.path().to_path_buf();
    seed(&root);
    let started = Instant::now();
    let outcome = witness_all(&root);
    let file_elapsed = started.elapsed();
    assert_eq!(outcome.schema, "glyim.ledger-gaps-witness/1");
    assert_eq!(outcome.class, "b");
    assert_eq!(
        outcome.doc_target,
        root.join("KNOWN_GAPS.md").display().to_string()
    );

    let text = black_box(format_gaps_witness_text(&outcome));
    let json = black_box(format_gaps_witness_json(&outcome));

    // The in-memory twin: same engine, same validation passes, the edit
    // lands in the owned document instead of the file.
    let mem_dir = tempfile::tempdir().expect("bench tmp");
    let mem_root = mem_dir.path().to_path_buf();
    seed(&mem_root);
    let loaded = ledger_schema::load_project_registry(&mem_root);
    let gap_file = loaded
        .registry
        .gaps()
        .first()
        .map(|sourced| sourced.path.clone())
        .expect("the seeded registry loads with gaps");
    let in_memory = InMemoryGapWitnessRewriter::default();
    in_memory.doc.borrow_mut().insert(
        gap_file.clone(),
        std::fs::read_to_string(&gap_file).expect("read seed"),
    );
    let started = Instant::now();
    for i in 0..GAPS {
        let request = GapsWitnessRequest {
            id: format!("GAP-{i:04}"),
            witness: format!("fixtures/gap-{i:04}.g"),
        };
        run_gaps_witness(black_box(&mem_root), black_box(&request), &in_memory)
            .unwrap_or_else(|err| panic!("bench in-memory witness GAP-{i:04} failed: {err}"));
    }
    let in_memory_elapsed = started.elapsed();
    assert_eq!(in_memory.edits.borrow().len(), GAPS);

    println!(
        "gaps witness bench: {GAPS} gaps / curated registry + fixtures (tempdir)\n  \
         file path  : {file_elapsed:.1?} (40 loads + 40 surgical edits)\n  \
         text face  : {} bytes\n  \
         json face  : {} bytes\n  \
         in-memory  : {in_memory_elapsed:.1?}",
        text.len(),
        json.len(),
    );

    // The retire face: a fresh registry, every gap removed in sequence —
    // each run is a load pass plus one whole-block removal.
    let retire_dir = tempfile::tempdir().expect("bench tmp");
    let retire_root = retire_dir.path().to_path_buf();
    seed(&retire_root);
    let started = Instant::now();
    let mut retire_remaining = 0;
    for i in 0..GAPS {
        let request = GapsRetireRequest {
            id: format!("GAP-{i:04}"),
        };
        let outcome = run_gaps_retire(
            black_box(&retire_root),
            black_box(&request),
            &FileGapRetirer,
        )
        .unwrap_or_else(|err| panic!("bench retire GAP-{i:04} failed: {err}"));
        retire_remaining = outcome.remaining;
    }
    let retire_elapsed = started.elapsed();
    assert_eq!(retire_remaining, 0, "every gap retired");
    let retire_text = black_box(format_gaps_retire_text(&GapsRetireOutcome {
        schema: glyim_ledger::GAPS_RETIRE_SCHEMA,
        id: "GAP-0000".to_string(),
        title: "bench gap 0: no lowering site sets cleanup".to_string(),
        target: "ledger/gaps/known/0001-bench.toml".to_string(),
        remaining: 0,
    }));
    let retire_json = black_box(format_gaps_retire_json(&GapsRetireOutcome {
        schema: glyim_ledger::GAPS_RETIRE_SCHEMA,
        id: "GAP-0000".to_string(),
        title: "bench gap 0: no lowering site sets cleanup".to_string(),
        target: "ledger/gaps/known/0001-bench.toml".to_string(),
        remaining: 0,
    }));

    println!(
        "gaps retire bench : {GAPS} gaps / curated registry (tempdir)\n  \
         file path  : {retire_elapsed:.1?} (40 loads + 40 block removals)\n  \
         text face  : {} bytes\n  \
         json face  : {} bytes",
        retire_text.len(),
        retire_json.len(),
    );

    // The open face: a fresh registry, one canonical block appended per
    // run — each run is a load pass (registry parse + duplicate-ID guard
    // + affects cross-reference) plus the next numbered default file
    // resolution and one append. The affects ID is a seeded feature, so
    // the cross-reference pass exercises the loaded registry.
    let open_dir = tempfile::tempdir().expect("bench tmp");
    let open_root = open_dir.path().to_path_buf();
    seed(&open_root);
    let started = Instant::now();
    for i in 0..GAPS {
        let request = GapsOpenRequest {
            id: format!("GAP-1{i:03}"),
            class: ledger_schema::GapClass::B,
            title: format!("bench gap {i}: opened without a witness"),
            affects: vec!["LANG-BENCH-CLEANUP".to_string()],
            caps: ledger_schema::Level::L2,
            opened_in: "v0.4.0".to_string(),
            file: None,
        };
        run_gaps_open(black_box(&open_root), black_box(&request), &FileGapOpener)
            .unwrap_or_else(|err| panic!("bench open GAP-1{i:03} failed: {err}"));
    }
    let open_elapsed = started.elapsed();
    let opened_files = open_root
        .join("ledger/gaps/known")
        .read_dir()
        .expect("known dir")
        .count();
    assert_eq!(opened_files, GAPS + 1, "one numbered file minted per gap");
    let open_outcome = GapsOpenOutcome {
        schema: glyim_ledger::GAPS_OPEN_SCHEMA,
        id: "GAP-1000".to_string(),
        class: "b".to_string(),
        title: "bench gap 0: opened without a witness".to_string(),
        affects: vec!["LANG-BENCH-CLEANUP".to_string()],
        caps: "L2".to_string(),
        opened_in: "v0.4.0".to_string(),
        target: "ledger/gaps/known/0002-gaps-open.toml".to_string(),
        doc_target: "KNOWN_GAPS.md".to_string(),
    };
    let open_text = black_box(format_gaps_open_text(&open_outcome));
    let open_json = black_box(format_gaps_open_json(&open_outcome));

    println!(
        "gaps open bench   : {GAPS} gaps / fresh + curated registry (tempdir)\n  \
         file path  : {open_elapsed:.1?} (40 loads + 40 canonical appends)\n  \
         text face  : {} bytes\n  \
         json face  : {} bytes",
        open_text.len(),
        open_json.len(),
    );

    // The audit face: a fresh registry witnessed end-to-end (every row
    // `Witnessed`), audited once, then re-audited with the fixture tree
    // removed (every row `Missing`) — each run is a full load pass plus
    // one metadata probe per gap, the same shape the CI gate pays.
    let audit_dir = tempfile::tempdir().expect("bench tmp");
    let audit_root = audit_dir.path().to_path_buf();
    seed(&audit_root);
    for i in 0..GAPS {
        let request = GapsWitnessRequest {
            id: format!("GAP-{i:04}"),
            witness: format!("fixtures/gap-{i:04}.g"),
        };
        run_gaps_witness(
            black_box(&audit_root),
            black_box(&request),
            &FileGapWitnessRewriter,
        )
        .unwrap_or_else(|err| panic!("bench witness GAP-{i:04} failed: {err}"));
    }
    let started = Instant::now();
    let audit_outcome = run_gaps_audit(black_box(&audit_root));
    let audit_witnessed_elapsed = started.elapsed();
    assert_eq!(
        audit_outcome.witnessed, GAPS,
        "every seeded gap is witnessed"
    );
    assert!(audit_outcome.is_ok(), "the witnessed state passes the gate");
    std::fs::remove_dir_all(audit_root.join("fixtures")).expect("remove fixtures");
    let started = Instant::now();
    let audit_missing = run_gaps_audit(black_box(&audit_root));
    let audit_missing_elapsed = started.elapsed();
    assert_eq!(audit_missing.missing, GAPS, "every witness is now gone");
    assert!(!audit_missing.is_ok(), "the missing state fails the gate");
    let audit_text = black_box(format_gaps_audit_text(&audit_outcome));
    let audit_json = black_box(format_gaps_audit_json(&audit_missing));

    println!(
        "gaps audit bench  : {GAPS} gaps / witnessed + stripped tree (tempdir)\n  \
         witnessed  : {audit_witnessed_elapsed:.1?} (1 load + {GAPS} probes)\n  \
         missing    : {audit_missing_elapsed:.1?} (1 load + {GAPS} probes)\n  \
         text face  : {} bytes\n  \
         json face  : {} bytes",
        audit_text.len(),
        audit_json.len(),
    );

    // The unwitness face: a fresh registry witnessed end-to-end, every
    // witness detached in sequence — each run is a load pass plus one
    // surgical block edit (the reverse of the witness path).
    let unwitness_dir = tempfile::tempdir().expect("bench tmp");
    let unwitness_root = unwitness_dir.path().to_path_buf();
    seed(&unwitness_root);
    for i in 0..GAPS {
        let request = GapsWitnessRequest {
            id: format!("GAP-{i:04}"),
            witness: format!("fixtures/gap-{i:04}.g"),
        };
        run_gaps_witness(
            black_box(&unwitness_root),
            black_box(&request),
            &FileGapWitnessRewriter,
        )
        .unwrap_or_else(|err| panic!("bench witness GAP-{i:04} failed: {err}"));
    }
    let started = Instant::now();
    for i in 0..GAPS {
        let request = GapsUnwitnessRequest {
            id: format!("GAP-{i:04}"),
        };
        run_gaps_unwitness(
            black_box(&unwitness_root),
            black_box(&request),
            &FileGapWitnessDetacher,
        )
        .unwrap_or_else(|err| panic!("bench unwitness GAP-{i:04} failed: {err}"));
    }
    let unwitness_elapsed = started.elapsed();
    let unwitnessed = run_gaps_audit(&unwitness_root);
    assert_eq!(unwitnessed.debt, GAPS, "every gap is honest debt again");
    let unwitness_outcome = GapsUnwitnessOutcome {
        schema: glyim_ledger::GAPS_UNWITNESS_SCHEMA,
        id: "GAP-0000".to_string(),
        title: "bench gap 0: no lowering site sets cleanup".to_string(),
        class: "b".to_string(),
        witness: Some("fixtures/gap-0000.g".to_string()),
        target: "ledger/gaps/known/0001-bench.toml".to_string(),
        doc_target: unwitness_root.join("KNOWN_GAPS.md").display().to_string(),
    };
    let unwitness_text = black_box(format_gaps_unwitness_text(&unwitness_outcome));
    let unwitness_json = black_box(format_gaps_unwitness_json(&unwitness_outcome));

    println!(
        "gaps unwitness    : {GAPS} gaps / witnessed registry (tempdir)\n  \
         file path  : {unwitness_elapsed:.1?} (40 loads + 40 detach edits)\n  \
         text face  : {} bytes\n  \
         json face  : {} bytes",
        unwitness_text.len(),
        unwitness_json.len(),
    );
}
