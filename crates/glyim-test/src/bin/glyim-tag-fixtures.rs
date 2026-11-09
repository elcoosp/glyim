//! `glyim-tag-fixtures` — the M1.4 tagging codemod driver.
//!
//! Walks a fixture tree (default: this crate's `tests/` directory) and
//! rewrites every `.g` fixture header into the canonical `//@` grammar with
//! derived readiness-ledger tags. See `annotations::codemod` for the
//! rewrite rules.
//!
//! Usage:
//! - `cargo run -p glyim-test --bin glyim-tag-fixtures` (dry run: prints
//!   the per-fixture plan and a summary, writes nothing)
//! - `... -- --write` (apply the rewrite to disk)
//! - `... -- --verify-ids <ledger/features-dir>` (cross-check every ID in
//!   the keyword table against the M1.2 registry files)

use std::path::{Path, PathBuf};

use glyim_test::annotations::codemod::{
    KEYWORD_FEATURES, dir_key_of, mode_feature, rewrite_fixture,
};

fn main() {
    let mut args = std::env::args().skip(1);
    let mut write = false;
    let mut verify_ids: Option<String> = None;
    let mut root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests");
    while let Some(a) = args.next() {
        match a.as_str() {
            "--write" => write = true,
            "--verify-ids" => {
                verify_ids = Some(
                    args.next()
                        .unwrap_or_else(|| panic!("--verify-ids requires a directory argument")),
                );
            }
            other => {
                root = PathBuf::from(other);
            }
        }
    }

    if let Some(dir) = verify_ids {
        verify_registry_ids(Path::new(&dir));
        return;
    }

    if !root.is_dir() {
        eprintln!("fixture root is not a directory: {}", root.display());
        std::process::exit(2);
    }

    let mut paths: Vec<PathBuf> = walkdir::WalkDir::new(&root)
        .into_iter()
        .filter_map(Result::ok)
        .map(|e| e.into_path())
        .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("g"))
        .collect();
    paths.sort();

    let mut converted = 0usize;
    let mut tagged = 0usize;
    let mut unchanged = 0usize;
    let mut failed = 0usize;

    for path in &paths {
        let dir_key = match dir_key_of(&root, path) {
            Some(k) => k,
            None => {
                eprintln!("SKIP (outside corpus dirs): {}", path.display());
                failed += 1;
                continue;
            }
        };
        let rel = path
            .strip_prefix(&root)
            .unwrap_or(path)
            .to_string_lossy()
            .replace('\\', "/");
        let Ok(source) = std::fs::read_to_string(path) else {
            eprintln!("ERROR reading {rel}");
            failed += 1;
            continue;
        };
        match rewrite_fixture(&rel, &dir_key, &source) {
            Ok(new_source) if new_source == source => unchanged += 1,
            Ok(new_source) => {
                let was_annotated = source.contains("//@");
                if was_annotated {
                    converted += 1;
                } else {
                    tagged += 1;
                }
                if write {
                    if let Err(e) = std::fs::write(path, &new_source) {
                        eprintln!("ERROR writing {rel}: {e}");
                        failed += 1;
                        continue;
                    }
                }
                println!(
                    "{} {rel} -> {} feature(s)",
                    if write { "WROTE" } else { "WOULD" },
                    count_features(&new_source)
                );
            }
            Err(e) => {
                eprintln!("ERROR {e}");
                failed += 1;
            }
        }
    }

    let action = if write { "applied" } else { "dry run" };
    println!(
        "{action}: {total} fixture(s) — {converted} converted, {tagged} newly tagged, {unchanged} already canonical, {failed} error(s)",
        total = paths.len()
    );
    if !write {
        println!("re-run with --write to apply");
    }
    if failed > 0 {
        std::process::exit(1);
    }
}

fn count_features(source: &str) -> usize {
    glyim_test::annotations::directives::parse_directives(source)
        .map(|a| a.features.len())
        .unwrap_or(0)
}

/// Cross-check every ID the codemod can emit against the actual registry
/// files (`ledger/features/**/*.toml`), so the table can never drift from
/// the M1.2 seed unnoticed.
fn verify_registry_ids(features_dir: &Path) {
    let mut registry: Vec<String> = Vec::new();
    for entry in walkdir::WalkDir::new(features_dir) {
        let Ok(entry) = entry else { continue };
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("toml") {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(path) else {
            continue;
        };
        for line in text.lines() {
            let t = line.trim();
            if let Some(rest) = t.strip_prefix("id ")
                && let Some(v) = rest.trim_start().strip_prefix('=')
            {
                let v = v.trim().trim_matches('"');
                registry.push(v.to_string());
            }
        }
    }
    let mut emitted: Vec<&str> = vec![
        "TOOL-TEST-RUN-PASS",
        "TOOL-TEST-RUN-FAIL",
        "TOOL-TEST-COMPILE-PASS",
        "TOOL-TEST-COMPILE-FAIL",
        "TOOL-TEST-UI",
        "TOOL-TEST-NO-ICE",
        "LANG-ASYNC-AWAIT",
    ];
    for (_, ids) in KEYWORD_FEATURES {
        emitted.extend(*ids);
    }
    emitted.push(mode_feature("no-ice"));
    emitted.sort_unstable();
    emitted.dedup();

    let mut missing = 0usize;
    for id in &emitted {
        if !registry.iter().any(|r| r == id) {
            eprintln!("ID NOT IN REGISTRY: {id}");
            missing += 1;
        }
    }
    println!(
        "verified {} distinct emitted IDs against {} registry entries ({} missing)",
        emitted.len(),
        registry.len(),
        missing
    );
    if missing > 0 {
        std::process::exit(1);
    }
}
