//! M0.4 TDD: `docs/adr/` must complete the master plan's §8 decision register.
//!
//! The ADR set is *derived* from the plan, never hardcoded here: the test
//! parses the §8 decision-register table in
//! `docs/roadmaps/glyim-master-plan.md` and asserts that every ADR ID in the
//! register has a canonical stub file (`docs/adr/NNNN-*.md`, headers Title /
//! Status / Context / Decision / Consequences), that no orphan ADR file
//! exists outside the register, and that `docs/adr/README.md` indexes every
//! register entry with a link and a status-table row. Adding an ADR row to
//! §8 without its file — or a file without its register row — fails here
//! before CI's docs gates ever see it.

use std::fs;
use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    // tools/glyim-metrics -> tools -> repo root.
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("glyim-metrics lives under <repo>/tools/")
        .to_path_buf()
}

/// ADR numbers parsed from the plan's §8 decision-register table rows
/// (`| NNN | decision | default | decide-by | falsified-if |`). The register
/// prints 3-digit IDs (`001`); files are zero-padded to four (`0001-…`), so
/// all comparisons below are numeric.
fn register_adr_numbers(root: &Path) -> Vec<u32> {
    let plan = fs::read_to_string(root.join("docs/roadmaps/glyim-master-plan.md"))
        .expect("master plan exists");
    let register = plan
        .split("## 8. Decision register")
        .nth(1)
        .expect("plan carries the §8 decision register")
        .split("\n## ")
        .next()
        .expect("register section is terminated by the next section");
    let mut numbers = Vec::new();
    for line in register.lines() {
        let cells: Vec<&str> = line.trim().split('|').map(str::trim).collect();
        // `| 001 | ... |` splits into ["", "001", ..., ""] — the first
        // non-empty cell is the register ID.
        let first = cells.iter().find(|c| !c.is_empty());
        if let Some(id) = first {
            if id.len() == 3 && id.chars().all(|c| c.is_ascii_digit()) {
                numbers.push(id.parse::<u32>().expect("3 ascii digits fit u32"));
            }
        }
    }
    assert!(
        !numbers.is_empty(),
        "the §8 register must yield at least one ADR ID — parser drifted"
    );
    numbers
}

/// `docs/adr/NNNN-*.md` files, sorted, as (number, path) pairs.
fn adr_files(root: &Path) -> Vec<(u32, PathBuf)> {
    let dir = root.join("docs/adr");
    let mut files = Vec::new();
    for entry in fs::read_dir(&dir).expect("docs/adr exists") {
        let path = entry.expect("readable dir entry").path();
        let name = path
            .file_name()
            .expect("filename")
            .to_string_lossy()
            .into_owned();
        let is_readme = name == "README.md";
        let is_markdown = path
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("md"));
        if is_readme || !is_markdown {
            continue;
        }
        let prefix = name.split('-').next().expect("NNNN- prefix").to_string();
        let number = prefix
            .parse::<u32>()
            .unwrap_or_else(|e| panic!("{name}: ADR prefix must be numeric: {e}"));
        files.push((number, path));
    }
    files.sort_by_key(|(n, _)| *n);
    files
}

/// The canonical ADR headers M0.4 requires on every stub.
fn assert_canonical_headers(number: u32, path: &Path) {
    let id = format!("ADR-{number:04}");
    let body = fs::read_to_string(path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    let title = body
        .lines()
        .find(|l| l.starts_with("# ADR-"))
        .unwrap_or_else(|| panic!("{}: missing `# ADR-...` title line", path.display()));
    assert!(
        title.contains(&id),
        "{}: title line does not name {id}",
        path.display()
    );
    for header in ["## Context", "## Decision", "## Consequences"] {
        assert!(
            body.contains(header),
            "{}: missing the canonical `{header}` section",
            path.display()
        );
    }
    assert!(
        body.contains("**Status:**"),
        "{}: missing the canonical `Status` header field",
        path.display()
    );
    assert!(
        body.contains("Proposed"),
        "{}: M0.4 stubs are created as Proposed",
        path.display()
    );
    // The stub must trace back to the plan's §8 register row it was seeded
    // from, so it stays plan-derived instead of drifting into invented
    // decisions.
    assert!(
        body.contains(&id) && body.contains("§8"),
        "{}: stub must trace back to the plan §8 register row {id}",
        path.display()
    );
}

#[test]
fn every_register_adr_has_a_canonical_stub() {
    let root = repo_root();
    for number in register_adr_numbers(&root) {
        let matches: Vec<PathBuf> = adr_files(&root)
            .into_iter()
            .filter(|(n, _)| *n == number)
            .map(|(_, p)| p)
            .collect();
        let id = format!("ADR-{number:04}");
        assert_eq!(
            matches.len(),
            1,
            "{id} must have exactly one `docs/adr/{number:04}-*.md` stub, found {}",
            matches.len()
        );
        assert_canonical_headers(number, &matches[0]);
    }
}

#[test]
fn no_orphan_adr_files_outside_the_register() {
    let root = repo_root();
    let numbers = register_adr_numbers(&root);
    for (number, path) in adr_files(&root) {
        assert!(
            numbers.contains(&number),
            "{path:?} carries ADR-{number:04} which is not in the plan §8 register — \
             add the register row or move the file to docs/archive/"
        );
    }
}

#[test]
fn adr_index_lists_and_links_every_register_entry() {
    let root = repo_root();
    let numbers = register_adr_numbers(&root);
    let index_path = root.join("docs/adr/README.md");
    let index = fs::read_to_string(&index_path).expect("docs/adr/README.md index exists");
    for number in numbers {
        let id = format!("ADR-{number:04}");
        let file_prefix = format!("{number:04}-");
        assert!(index.contains(&id), "docs/adr/README.md must list {id}");
        assert!(
            index
                .lines()
                .any(|l| l.contains(&format!("[`{file_prefix}")) && l.contains("](")),
            "docs/adr/README.md must link the `{file_prefix}*.md` file for {id}"
        );
        assert!(
            index
                .lines()
                .any(|l| l.starts_with('|') && l.contains(&id) && l.contains("Proposed")),
            "docs/adr/README.md's status table must carry a row for {id}"
        );
    }
}

#[test]
fn register_is_not_secretly_truncated() {
    // The plan §8 register at M0.4 time carries ADR-001..015; if the register
    // shrinks below that, this test wants the truncation to be deliberate, so
    // it fails loudly instead of letting a hand-edit silently drop decisions.
    let root = repo_root();
    let count = register_adr_numbers(&root).len();
    assert!(
        count >= 15,
        "the §8 register lost rows ({count} < 15) — restoring ADR-001..015 is M0.4 scope"
    );
}
