//! Gap-lifecycle consumer walkthrough: retire one gap — the exact
//! validation and removal path `glyim-ledger gaps retire [ROOT] --id
//! <GAP-ID>` runs — then inspect the rewritten registry and the typed
//! rejections along the way.
//!
//! Run from the repository root:
//!
//! ```text
//! cargo run -p glyim-ledger --example gaps_retire
//! ```
//!
//! A real retire mutates the repository's `ledger/gaps/known/*.toml`, so
//! the walkthrough writes into a throwaway registry instead of the live
//! one. It seeds a curated registry (header prose, two open gaps), first
//! shows the typed rejections that write nothing (unknown ID), retires
//! one gap (the whole `[[gap]]` block is gone; the curated prose and the
//! sibling block survive byte-for-byte), and re-runs the read face after
//! the removal. Try the CLI on your own registry deliberately:
//!
//! ```text
//! cargo run -p glyim-ledger -- gaps                     # the witness report
//! cargo run -p glyim-ledger -- gaps retire --id GAP-0002
//! ```

use std::path::PathBuf;

use glyim_ledger::{
    FileGapRetirer, GapRetirer, GapsRetireRequest, InMemoryGapRetirer, format_gaps_retire_text,
    run_gaps, run_gaps_retire,
};

/// The curated seed registry, in the committed `ledger/gaps/known/`
/// style: header prose the retirer must preserve, two open entries.
const CURATED: &str = "# Curated header prose the retirer must preserve byte-for-byte:\n\
     # closed entries retire to the git history of this file and\n\
     # of the registry.\n\
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
    // The retire face always rewrites, so the walkthrough uses a
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
    ] {
        let path = root.join(rel);
        std::fs::create_dir_all(path.parent().expect("mkdir")).expect("mkdir");
        std::fs::write(path, contents).expect("write seed");
    }

    // The typed rejections write nothing — the unknown-ID rejection names
    // a few existing candidates for the help line (the revoke-face shape).
    let unknown = GapsRetireRequest {
        id: "GAP-9999".to_string(),
    };
    if let Err(err) = run_gaps_retire(&root, &unknown, &FileGapRetirer) {
        println!("unknown ID:");
        println!("  {err}");
        if let Some(help) = miette::Diagnostic::help(&err) {
            println!("  help: {help}");
        }
        println!();
    }

    // The happy path: retire GAP-0002. The engine loads the registry,
    // validates the request, and drops exactly the matched block — the
    // curated header prose and the sibling block pass through untouched.
    let request = GapsRetireRequest {
        id: "GAP-0002".to_string(),
    };
    let outcome = run_gaps_retire(&root, &request, &FileGapRetirer).expect("the retire lands");
    print!("{}", format_gaps_retire_text(&outcome));
    println!();

    // The registry is the source of truth: the rewritten file re-loads
    // cleanly with one gap left, and the read face reports the debt that
    // remains.
    let loaded = ledger_schema::load_project_registry(&root);
    println!(
        "re-loaded registry: {} gap(s), GAP-0002 present: {}",
        loaded.registry.gap_count(),
        loaded
            .registry
            .gaps()
            .iter()
            .any(|sourced| sourced.entry.id == "GAP-0002"),
    );
    let report = run_gaps(&root);
    println!(
        "read face: {} open gap(s), witness debt: {:?}\n",
        report.gaps.len(),
        report.unwitnessed,
    );

    // Programmatic consumers can drive the same engine against the
    // in-memory twin of the rewrite seam (the witness walkthrough shape):
    // seed the document, retire, inspect the audit trail.
    let in_memory = InMemoryGapRetirer::default();
    in_memory
        .doc
        .borrow_mut()
        .insert(PathBuf::from("memory.toml"), CURATED.to_string());
    let removed = in_memory
        .retire(std::path::Path::new("memory.toml"), "GAP-0002")
        .expect("the in-memory retire lands");
    println!("in-memory twin removed: {removed:?}");
    println!("audit trail: {:?}", in_memory.removed.borrow().len());
}
