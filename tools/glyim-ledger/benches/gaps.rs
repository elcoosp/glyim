//! Gaps write-face bench (`gaps witness`, `gaps retire`).
//!
//! Dependency-free (`harness = false`), mirroring the `release` bench:
//! measures the [`glyim_ledger::run_gaps_witness`] engine over a seeded
//! tempdir registry — every run is a full load pass (registry parse +
//! gap lookup + fixture existence check) plus one surgical block edit —
//! witnessing `GAPS` registry gaps in sequence, both formatter faces
//! (the text one-liner and the `glyim.ledger-gaps-witness/1` JSON
//! contract), the in-memory twin of the rewrite seam, and the retire
//! face's whole-block removal path over a fresh registry, so CI logs can
//! track the write-face cost as the gap registry grows. Run with:
//!
//! ```text
//! cargo bench -p glyim-ledger --bench gaps
//! ```

use std::fmt::Write as _;
use std::hint::black_box;
use std::path::Path;
use std::time::Instant;

use glyim_ledger::{
    FileGapRetirer, FileGapWitnessRewriter, GapsRetireOutcome, GapsRetireRequest,
    GapsWitnessOutcome, GapsWitnessRequest, InMemoryGapWitnessRewriter, format_gaps_retire_json,
    format_gaps_retire_text, format_gaps_witness_json, format_gaps_witness_text, run_gaps_retire,
    run_gaps_witness,
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
}
