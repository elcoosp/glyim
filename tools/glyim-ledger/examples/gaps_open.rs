//! Gap-lifecycle consumer walkthrough: open one gap — the exact
//! validation and append path `glyim-ledger gaps open [ROOT] --id
//! <GAP-NNNN> --class <a|b|c> --title <TEXT> --affects <ID[,ID...]>
//! --caps <L0..L5> --opened-in <TAG> [--file PATH]` runs — then inspect
//! the minted registry file, the refreshed render, and the typed
//! rejections along the way.
//!
//! Run from the repository root:
//!
//! ```text
//! cargo run -p glyim-ledger --example gaps_open
//! ```
//!
//! A real open mutates the repository's `ledger/gaps/known/*.toml`, so
//! the walkthrough writes into a throwaway registry instead of the live
//! one. It seeds a curated registry (header prose, one witnessed gap),
//! shows the typed rejections that write nothing (a non-canonical ID, a
//! phantom affects ID), opens one gap (the canonical block is appended
//! with the explicit `status = "unwitnessed"` line — witness debt is
//! never silent), and re-runs the read face and the render `--check`
//! gate after the append. Try the CLI on your own registry deliberately:
//!
//! ```text
//! cargo run -p glyim-ledger -- gaps                     # the witness report
//! cargo run -p glyim-ledger -- gaps open . --id GAP-0006 --class b \
//!   --title "no lowering site sets cleanup" --affects LANG-UNWIND-CLEANUP \
//!   --caps L2 --opened-in v0.4.0
//! ```

use std::path::PathBuf;

use glyim_ledger::{
    FileGapOpener, GapOpener, GapsOpenRequest, InMemoryGapOpener, format_gaps_open_text, run_gaps,
    run_gaps_open,
};
use ledger_schema::{GapClass, Level};

fn main() {
    // The open face always rewrites, so the walkthrough uses a throwaway
    // registry instead of the repository's live gap files.
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
            "# Curated header prose the opener must preserve byte-for-byte.\n\
             \n\
             [[gap]]\n\
             id        = \"GAP-0001\"\n\
             class     = \"a\"\n\
             title     = \".await inside a loop body is diagnosed (T0060)\"\n\
             affects   = [\"LANG-DEMO-AWAIT\"]\n\
             caps      = \"L2\"\n\
             opened_in = \"v0.3.0\"\n\
             status    = \"witnessed\"\n\
             witness   = \"fixtures/await-loop-known-gap.g\"\n",
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

    // The typed rejections write nothing — the face mints IDs, so the
    // canonical GAP-NNNN shape is enforced up front, and every affected
    // feature must exist in the loaded registry.
    for (label, request) in [
        (
            "non-canonical ID",
            GapsOpenRequest {
                id: "GAP-6".to_string(),
                class: GapClass::B,
                title: "no lowering site sets cleanup".to_string(),
                affects: vec!["LANG-DEMO-AWAIT".to_string()],
                caps: Level::L2,
                opened_in: "v0.4.0".to_string(),
                file: None,
            },
        ),
        (
            "phantom affects ID",
            GapsOpenRequest {
                id: "GAP-0006".to_string(),
                class: GapClass::B,
                title: "no lowering site sets cleanup".to_string(),
                affects: vec!["LANG-NOPE".to_string()],
                caps: Level::L2,
                opened_in: "v0.4.0".to_string(),
                file: None,
            },
        ),
    ] {
        if let Err(err) = run_gaps_open(&root, &request, &FileGapOpener) {
            println!("{label}:");
            println!("  {err}");
            if let Some(help) = miette::Diagnostic::help(&err) {
                println!("  help: {help}");
            }
            println!();
        }
    }

    // The happy path: open GAP-0006. The engine loads the registry,
    // validates the request, renders the canonical block, and appends it
    // to the next numbered file (0002-gaps-open.toml) — the curated
    // header prose and the sibling block pass through untouched.
    let request = GapsOpenRequest {
        id: "GAP-0006".to_string(),
        class: GapClass::B,
        title: "Windows SEH cleanup edges are unreachable".to_string(),
        affects: vec!["LANG-DEMO-AWAIT".to_string()],
        caps: Level::L2,
        opened_in: "v0.4.0".to_string(),
        file: None,
    };
    let outcome = run_gaps_open(&root, &request, &FileGapOpener).expect("the open lands");
    print!("{}", format_gaps_open_text(&outcome));
    println!();

    // The registry is the source of truth: the minted file re-loads
    // cleanly, the new entry is explicit witness debt, and the read face
    // reports it in the debt line.
    let loaded = ledger_schema::load_project_registry(&root);
    println!(
        "re-loaded registry: {} gap(s), GAP-0006 status: {:?}",
        loaded.registry.gap_count(),
        loaded
            .registry
            .gaps()
            .iter()
            .find(|sourced| sourced.entry.id == "GAP-0006")
            .map(|sourced| sourced.entry.status),
    );
    let report = run_gaps(&root);
    println!(
        "read face: {} open gap(s), witness debt: {:?}\n",
        report.gaps.len(),
        report.unwitnessed,
    );

    // Programmatic consumers can drive the same engine against the
    // in-memory twin of the append seam (the witness walkthrough shape):
    // seed the document, open, inspect the audit trail.
    let in_memory = InMemoryGapOpener::default();
    in_memory
        .doc
        .borrow_mut()
        .insert(PathBuf::from("memory.toml"), String::new());
    let opened = in_memory
        .open(std::path::Path::new("memory.toml"), "GAP-0006", "block")
        .expect("the in-memory open lands");
    println!("in-memory twin opened: {opened}");
    println!("audit trail: {:?}", in_memory.opened.borrow().len());
}
