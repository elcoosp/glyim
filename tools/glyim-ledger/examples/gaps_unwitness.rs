//! Gap-registry consumer walkthrough: detach one witnessed gap's witness
//! — the exact validation and reverse-rewrite path
//! `glyim-ledger gaps unwitness [ROOT] --id <GAP-ID>` runs — then inspect
//! the rewritten registry, the round-trip with the witness face, and the
//! typed rejections along the way.
//!
//! Run from the repository root:
//!
//! ```text
//! cargo run -p glyim-ledger --example gaps_unwitness
//! ```
//!
//! A real detach mutates the repository's `ledger/gaps/known/*.toml`, so
//! the walkthrough writes into a throwaway registry instead of the live
//! one. It seeds a curated registry (header prose, one witnessed entry,
//! one unwitnessed), detaches the witnessed gap's witness (the curated
//! prose survives byte-for-byte; only the matched block's `status` flips
//! back and the aligned `witness` line is removed), shows the typed
//! rejections that write nothing, and round-trips a fresh witness
//! through `gaps witness` and back. The remedy is the counterpart of the
//! audit face's `missing` verdict: when a witness fixture moves or is
//! renamed, detach the stale witness and re-witness it at its new
//! spelling. Try the CLI on your own registry deliberately:
//!
//! ```text
//! cargo run -p glyim-ledger -- gaps audit                # the finding
//! cargo run -p glyim-ledger -- gaps unwitness --id GAP-0002   # the remedy
//! ```

use std::path::PathBuf;

use glyim_ledger::{
    FileGapWitnessDetacher, FileGapWitnessRewriter, GapWitnessDetacher, GapsUnwitnessRequest,
    GapsWitnessRequest, InMemoryGapWitnessDetacher, format_gaps_unwitness_text, run_gaps_unwitness,
    run_gaps_witness,
};

/// The curated seed registry, in the committed `ledger/gaps/known/`
/// style: header prose the detacher must preserve, one witnessed entry,
/// one unwitnessed one.
const CURATED: &str = "# Curated header prose the detacher must preserve byte-for-byte:\n\
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
    // The detach face always rewrites, so the walkthrough uses a
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
    // help line, printed here through the same shape `main` uses.
    for (label, request) in [
        (
            "unknown ID",
            GapsUnwitnessRequest {
                id: "GAP-9999".to_string(),
            },
        ),
        (
            "unwitnessed gap (nothing to detach)",
            GapsUnwitnessRequest {
                id: "GAP-0002".to_string(),
            },
        ),
    ] {
        if let Err(err) = run_gaps_unwitness(&root, &request, &FileGapWitnessDetacher) {
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

    // The happy path: detach GAP-0001's witness. The engine loads the
    // registry, validates the request, and edits exactly the matched
    // block — the detacher returns whether the edit landed.
    let request = GapsUnwitnessRequest {
        id: "GAP-0001".to_string(),
    };
    let outcome =
        run_gaps_unwitness(&root, &request, &FileGapWitnessDetacher).expect("the detach lands");
    print!("{}", format_gaps_unwitness_text(&outcome));
    println!(
        "outcome: witness={:?} (the detached path, retired to git history)",
        outcome.witness,
    );
    println!();

    // A second detach for the same gap is a typed rejection (the gap is
    // honest witness debt again), again writing nothing.
    if let Err(err) = run_gaps_unwitness(&root, &request, &FileGapWitnessDetacher) {
        println!("already unwitnessed:");
        println!("  {err}");
        if let Some(help) = miette::Diagnostic::help(&err) {
            println!("  help: {help}");
        }
    }
    println!();

    // The registry is the source of truth: the rewritten file re-loads
    // cleanly and reports GAP-0001 as unwitnessed, and the curated header
    // prose is preserved byte-for-byte.
    let loaded = ledger_schema::load_project_registry(&root);
    let gap = loaded
        .registry
        .gaps()
        .iter()
        .map(|sourced| &sourced.entry)
        .find(|gap| gap.id == "GAP-0001")
        .expect("GAP-0001 still present");
    println!(
        "re-loaded registry: GAP-0001 status={}, witness={:?}",
        gap.status.as_str(),
        gap.witness,
    );
    let on_disk = std::fs::read_to_string(root.join("ledger/gaps/known/0001-seed.toml"))
        .expect("read the rewritten registry");
    let kept = on_disk.lines().filter(|line| line.starts_with('#')).count();
    println!("curated comment lines preserved: {kept}\n");

    // The move-witness remedy: re-witness the gap (any fixture spelling),
    // then detach again — the two faces are exact inverses, so a stale or
    // wrong witness path never needs a hand edit. Re-witnessing with the
    // original path restores the curated seed bytes exactly; detaching
    // again returns the gap to honest witness debt.
    run_gaps_witness(
        &root,
        &GapsWitnessRequest {
            id: "GAP-0001".to_string(),
            witness: "fixtures/await-loop-known-gap.g".to_string(),
        },
        &FileGapWitnessRewriter,
    )
    .expect("the re-witness lands");
    let re_witnessed = std::fs::read_to_string(root.join("ledger/gaps/known/0001-seed.toml"))
        .expect("read the re-witnessed registry");
    println!(
        "re-witness restored the curated seed bytes: {}",
        re_witnessed == CURATED,
    );
    run_gaps_unwitness(&root, &request, &FileGapWitnessDetacher).expect("the re-detach lands");
    let re_detached = std::fs::read_to_string(root.join("ledger/gaps/known/0001-seed.toml"))
        .expect("read the re-detached registry");
    println!(
        "re-detach flipped GAP-0001 back to unwitnessed: {}",
        re_detached.contains("id        = \"GAP-0001\"")
            && re_detached
                .lines()
                .any(|line| line.trim_start().starts_with("status")
                    && line.contains("unwitnessed")),
    );
    println!();

    // Programmatic consumers can drive the same engine against the
    // in-memory twin of the detach seam (the witness walkthrough shape):
    // seed the document, detach, inspect the audit trail.
    let in_memory = InMemoryGapWitnessDetacher::default();
    in_memory
        .doc
        .borrow_mut()
        .insert(PathBuf::from("memory.toml"), CURATED.to_string());
    let edited = in_memory
        .detach(std::path::Path::new("memory.toml"), "GAP-0001")
        .expect("the in-memory edit lands");
    println!("in-memory twin edit landed: {edited}");
    println!("audit trail: {:?}", in_memory.edits.borrow().len());
}
