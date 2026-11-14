//! Gap-lifecycle consumer walkthrough: witness one gap — the exact
//! validation and rewrite path `glyim-ledger gaps witness [ROOT] --id
//! <GAP-ID> --witness <PATH>` runs — then inspect the rewritten registry
//! and the typed rejections along the way.
//!
//! Run from the repository root:
//!
//! ```text
//! cargo run -p glyim-ledger --example gaps_witness
//! ```
//!
//! A real witness mutates the repository's `ledger/gaps/known/*.toml`,
//! so the walkthrough writes into a throwaway registry instead of the
//! live one. It seeds a curated registry (header prose, one witnessed
//! entry, one unwitnessed), witnesses the open gap (the curated prose
//! survives byte-for-byte; only the matched block's `status` flips and
//! the aligned `witness` line is appended), and shows the typed
//! rejections — unknown ID, already-witnessed, phantom fixture — that
//! write nothing. Try the CLI on your own registry deliberately:
//!
//! ```text
//! cargo run -p glyim-ledger -- gaps                     # the witness report
//! cargo run -p glyim-ledger -- gaps witness --id GAP-0002 --witness crates/glyim-test/tests/compile-fail/your-fixture.g
//! ```

use std::path::PathBuf;

use glyim_ledger::{
    FileGapWitnessRewriter, GapWitnessRewriter, GapsWitnessRequest, InMemoryGapWitnessRewriter,
    format_gaps_witness_text, run_gaps_witness,
};

/// The curated seed registry, in the committed `ledger/gaps/known/`
/// style: header prose the rewriter must preserve, one witnessed entry,
/// one unwitnessed one.
const CURATED: &str = "# Curated header prose the rewriter must preserve byte-for-byte:\n\
     # witness debt stays honest — GAP-0002 is unwitnessed until a\n\
     # fixture reproduces it.\n\
     \n\
     [[gap]]\n\
     id        = \"GAP-0001\"\n\
     class     = \"a\"\n\
     title     = \".await inside a loop body is diagnosed (T0060)\"\n\
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
     affects   = [\"LANG-DEMO-CLEANUP\"]\n\
     caps      = \"L2\"\n\
     opened_in = \"v0.3.0\"\n\
     status    = \"unwitnessed\"\n";

fn main() {
    // The witness face always rewrites, so the walkthrough uses a
    // throwaway registry instead of the repository's live gap files.
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
            "ledger/features/lang/b.toml",
            "[[feature]]\n\
             id = \"LANG-DEMO-CLEANUP\"\n\
             title = \"t LANG-DEMO-CLEANUP\"\n\
             area = \"lang.control-flow\"\n\
             tier = \"core\"\n\
             spec = \"docs/specs/v0.1.0.md\"\n\
             target_level = \"L5\"\n\
             backends = [\"gam\", \"llvm\"]\n\
             owner_lane = \"M\"\n",
        ),
        ("ledger/gaps/known/0001-seed.toml", CURATED),
        (
            "fixtures/await-loop-known-gap.g",
            "//@ known-gap: GAP-0001\nfn main() {}\n",
        ),
        (
            "fixtures/cleanup-known-gap.g",
            "//@ known-gap: GAP-0002\nfn main() {}\n",
        ),
    ] {
        let path = root.join(rel);
        std::fs::create_dir_all(path.parent().expect("mkdir")).expect("mkdir");
        std::fs::write(path, contents).expect("write seed");
    }

    // The rejections write nothing — each one is a typed CLI error with a
    // help line, printed here through the same shape `main` uses, and all
    // exercised while GAP-0002 is still open so every check is reachable.
    for (label, request) in [
        (
            "unknown ID",
            GapsWitnessRequest {
                id: "GAP-9999".to_string(),
                witness: "fixtures/cleanup-known-gap.g".to_string(),
            },
        ),
        (
            "phantom fixture",
            GapsWitnessRequest {
                id: "GAP-0002".to_string(),
                witness: "fixtures/does-not-exist.g".to_string(),
            },
        ),
    ] {
        if let Err(err) = run_gaps_witness(&root, &request, &FileGapWitnessRewriter) {
            println!("{label}:");
            println!("  {err}");
            if let Some(help) = miette::Diagnostic::help(&err) {
                println!("  help: {help}");
            }
        } else {
            println!("{label}: (unexpectedly accepted)");
        }
    }
    println!();

    // The happy path: witness GAP-0002 with the committed fixture. The
    // engine loads the registry, validates the request, and edits exactly
    // the matched block — the rewriter returns whether the edit landed.
    let request = GapsWitnessRequest {
        id: "GAP-0002".to_string(),
        witness: "fixtures/cleanup-known-gap.g".to_string(),
    };
    let outcome =
        run_gaps_witness(&root, &request, &FileGapWitnessRewriter).expect("the witness lands");
    print!("{}", format_gaps_witness_text(&outcome));
    println!();

    // A second witness for the same gap is a typed rejection (never a
    // silent overwrite), again writing nothing.
    if let Err(err) = run_gaps_witness(&root, &request, &FileGapWitnessRewriter) {
        println!("already witnessed:");
        println!("  {err}");
        if let Some(help) = miette::Diagnostic::help(&err) {
            println!("  help: {help}");
        }
    }
    println!();

    // The registry is the source of truth: the rewritten file re-loads
    // cleanly and reports GAP-0002 as witnessed, and the curated header
    // prose is preserved byte-for-byte.
    let loaded = ledger_schema::load_project_registry(&root);
    let gap = loaded
        .registry
        .gaps()
        .iter()
        .map(|sourced| &sourced.entry)
        .find(|gap| gap.id == "GAP-0002")
        .expect("GAP-0002 still present");
    println!(
        "re-loaded registry: GAP-0002 status={}, witness={:?}",
        gap.status.as_str(),
        gap.witness,
    );
    let on_disk = std::fs::read_to_string(root.join("ledger/gaps/known/0001-seed.toml"))
        .expect("read the rewritten registry");
    let kept = on_disk.lines().filter(|line| line.starts_with('#')).count();
    println!("curated comment lines preserved: {kept}\n");

    // Programmatic consumers can drive the same engine against the
    // in-memory twin of the rewrite seam (the release-check walkthrough
    // shape): seed the document, witness, inspect the audit trail.
    let in_memory = InMemoryGapWitnessRewriter::default();
    in_memory
        .doc
        .borrow_mut()
        .insert(PathBuf::from("memory.toml"), CURATED.to_string());
    let edited = in_memory
        .witness(
            std::path::Path::new("memory.toml"),
            "GAP-0002",
            "fixtures/cleanup-known-gap.g",
        )
        .expect("the in-memory edit lands");
    println!("in-memory twin edit landed: {edited}");
    println!("audit trail: {:?}", in_memory.edits.borrow().len());
}
