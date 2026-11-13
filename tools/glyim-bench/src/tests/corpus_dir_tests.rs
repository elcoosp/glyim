//! Integration tests for the M2.3 remainder: the committed corpora
//! directory. The manifest substrate (`CorpusManifest::verify`) pins
//! template identity; these tests pin the **directory** contract:
//! `write_corpus_dir` materializes programs + `corpus.json` through the
//! [`crate::corpus_dir::CorpusSink`] seam byte-deterministically, and
//! `check_corpus_dir` re-verifies the committed bytes ("corpora committed
//! with hashes") — a hand-edited program, a deleted program, or a drifted
//! manifest all fail the gate.

use std::path::Path;

use tempfile::TempDir;

use crate::corpus::{CORPUS_SCHEMA, CorpusFamily};
use crate::corpus_dir::{
    CORPUS_DIR_MANIFEST, CorpusDirPlan, CorpusSink, DirCorpusSink, check_corpus_dir,
    corpus_file_name, write_corpus_dir,
};
use crate::error::BenchError;

/// The test plan: two families × two seeds at the minimum size — small,
/// deterministic, and exercising both the manifest and the file writes.
fn test_plan() -> CorpusDirPlan {
    CorpusDirPlan::from_flags(Some("wide,match"), Some("0,1"), 8).expect("plan")
}

#[test]
fn file_names_are_deterministic() {
    assert_eq!(corpus_file_name(CorpusFamily::Wide, 0, 8), "wide-0-8.g");
    assert_eq!(
        corpus_file_name(CorpusFamily::Macro, 17, 256),
        "macro-17-256.g"
    );
}

#[test]
fn plan_parses_family_and_seed_lists() {
    let plan = test_plan();
    assert_eq!(plan.families, vec![CorpusFamily::Wide, CorpusFamily::Match]);
    assert_eq!(plan.seeds, vec![0, 1]);
    assert_eq!(plan.size, 8);

    // Every family name parses; a bad one is a CorpusPlan error.
    for family in CorpusFamily::ALL {
        assert!(CorpusDirPlan::from_flags(Some(family.as_str()), Some("0"), 8).is_ok());
    }
    assert!(matches!(
        CorpusDirPlan::from_flags(Some("wide,nope"), Some("0"), 8),
        Err(BenchError::CorpusPlan { .. })
    ));
    assert!(matches!(
        CorpusDirPlan::from_flags(Some("wide"), Some(""), 8),
        Err(BenchError::CorpusPlan { .. })
    ));
    assert!(matches!(
        CorpusDirPlan::from_flags(Some("wide"), Some("0,x"), 8),
        Err(BenchError::CorpusPlan { .. })
    ));
    assert!(matches!(
        CorpusDirPlan::from_flags(Some("wide"), Some("0"), 0),
        Err(BenchError::CorpusPlan { .. })
    ));
    assert!(matches!(
        CorpusDirPlan::from_flags(Some("wide"), Some("0"), crate::corpus::MAX_SIZE + 1),
        Err(BenchError::CorpusPlan { .. })
    ));
}

#[test]
fn sink_trait_writes_programs_and_manifest() {
    let dir = TempDir::new().expect("tmp");
    let sink: &mut dyn CorpusSink = &mut DirCorpusSink::new(dir.path());
    let manifest = write_corpus_dir(sink, &test_plan()).expect("write");
    assert_eq!(manifest.schema, CORPUS_SCHEMA);
    assert_eq!(manifest.programs.len(), 4);

    let manifest_path = dir.path().join(CORPUS_DIR_MANIFEST);
    assert!(manifest_path.exists(), "corpus.json must land");
    for entry in &manifest.programs {
        let file = dir
            .path()
            .join(corpus_file_name(entry.family, entry.seed, entry.size));
        assert!(file.exists(), "{} must land", file.display());
        let source = std::fs::read_to_string(&file).expect("read program");
        assert_eq!(crate::corpus::fnv1a64(&source), entry.identity);
    }
}

#[test]
fn write_is_byte_deterministic_across_dirs() {
    let first = TempDir::new().expect("tmp");
    let second = TempDir::new().expect("tmp");
    write_corpus_dir(&mut DirCorpusSink::new(first.path()), &test_plan()).expect("write");
    write_corpus_dir(&mut DirCorpusSink::new(second.path()), &test_plan()).expect("write");
    let manifest_bytes =
        |dir: &TempDir| std::fs::read(dir.path().join(CORPUS_DIR_MANIFEST)).expect("manifest");
    assert_eq!(manifest_bytes(&first), manifest_bytes(&second));
    for entry in write_corpus_dir(&mut DirCorpusSink::new(first.path()), &test_plan())
        .expect("write")
        .programs
    {
        let name = corpus_file_name(entry.family, entry.seed, entry.size);
        assert_eq!(
            std::fs::read(first.path().join(&name)).expect("first"),
            std::fs::read(second.path().join(&name)).expect("second"),
            "{name} must be byte-identical"
        );
    }
}

#[test]
fn check_passes_on_a_freshly_committed_dir() {
    let dir = TempDir::new().expect("tmp");
    write_corpus_dir(&mut DirCorpusSink::new(dir.path()), &test_plan()).expect("write");
    let check = check_corpus_dir(dir.path()).expect("check");
    assert!(check.is_ok(), "{check:?}");
    assert!(check.missing.is_empty());
    assert!(check.mismatched.is_empty());
    assert_eq!(check.entries, 4);
    let text = check.render_text();
    assert!(text.contains("4 program(s)"), "{text}");
}

#[test]
fn hand_edited_program_fails_the_check() {
    let dir = TempDir::new().expect("tmp");
    write_corpus_dir(&mut DirCorpusSink::new(dir.path()), &test_plan()).expect("write");
    let victim = dir.path().join(corpus_file_name(CorpusFamily::Wide, 0, 8));
    std::fs::write(&victim, "// hand edit\n").expect("tamper");
    let check = check_corpus_dir(dir.path()).expect("check still reports, not errors");
    assert!(!check.is_ok());
    assert_eq!(
        check.mismatched,
        vec![corpus_file_name(CorpusFamily::Wide, 0, 8)]
    );
}

#[test]
fn deleted_program_fails_the_check() {
    let dir = TempDir::new().expect("tmp");
    write_corpus_dir(&mut DirCorpusSink::new(dir.path()), &test_plan()).expect("write");
    let victim = dir.path().join(corpus_file_name(CorpusFamily::Match, 1, 8));
    std::fs::remove_file(&victim).expect("delete");
    let check = check_corpus_dir(dir.path()).expect("check still reports, not errors");
    assert!(!check.is_ok());
    assert_eq!(
        check.missing,
        vec![corpus_file_name(CorpusFamily::Match, 1, 8)]
    );
}

#[test]
fn drifted_manifest_identity_fails_with_the_template_error() {
    let dir = TempDir::new().expect("tmp");
    write_corpus_dir(&mut DirCorpusSink::new(dir.path()), &test_plan()).expect("write");
    let manifest_path = dir.path().join(CORPUS_DIR_MANIFEST);
    // Flip one recorded identity: the template regenerates the true value,
    // so `CorpusManifest::verify` (running inside the dir check) must fail
    // with the existing template-drift error.
    let mut manifest: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&manifest_path).expect("read manifest"))
            .expect("parse manifest");
    manifest["programs"][0]["identity"] = serde_json::Value::from(u64::MAX);
    std::fs::write(
        &manifest_path,
        serde_json::to_string_pretty(&manifest).expect("serialize manifest"),
    )
    .expect("tamper manifest");
    assert!(
        matches!(
            check_corpus_dir(dir.path()),
            Err(BenchError::CorpusIdentityMismatch { .. })
        ),
        "identity drift must surface as the template-drift error"
    );
}

#[test]
fn missing_manifest_is_a_corpus_dir_read_error() {
    let dir = TempDir::new().expect("tmp");
    assert!(matches!(
        check_corpus_dir(dir.path()),
        Err(BenchError::CorpusDirRead { .. })
    ));
}

#[test]
fn corrupted_manifest_is_a_corpus_dir_read_error() {
    let dir = TempDir::new().expect("tmp");
    std::fs::write(dir.path().join(CORPUS_DIR_MANIFEST), "not json").expect("write junk");
    assert!(matches!(
        check_corpus_dir(dir.path()),
        Err(BenchError::CorpusDirRead { .. })
    ));
}

#[test]
fn corpus_dir_errors_carry_code_and_help() {
    for err in [
        BenchError::CorpusDirRead {
            path: "perf/corpus/corpus.json".into(),
            detail: "missing".into(),
        },
        BenchError::CorpusFileMissing {
            file: "perf/corpus/wide-0-8.g".into(),
            detail: "No such file".into(),
        },
        BenchError::CorpusFileMismatch {
            file: "perf/corpus/wide-0-8.g".into(),
            expected: 0xab,
            actual: 0xcd,
        },
    ] {
        assert!(!err.to_string().is_empty(), "{err:?} must render");
        let help =
            miette::Diagnostic::help(&err).expect("every BenchError variant must carry help text");
        assert!(
            !help.to_string().is_empty(),
            "{err:?} help must be non-empty"
        );
    }
}

#[test]
fn cli_accepts_the_corpus_subcommand() {
    use crate::cli::{Cli, Command};
    use clap::Parser as _;

    let cli = Cli::try_parse_from([
        "glyim-bench",
        "corpus",
        "write",
        "--dir",
        "perf/corpus",
        "--families",
        "wide,deep",
        "--seeds",
        "0,1",
        "--size",
        "16",
    ])
    .expect("parse corpus write");
    match cli.command {
        Command::Corpus { action } => match action {
            crate::cli::CorpusAction::Write {
                dir,
                families,
                seeds,
                size,
            } => {
                assert_eq!(dir, Path::new("perf/corpus"));
                assert_eq!(families.as_deref(), Some("wide,deep"));
                assert_eq!(seeds.as_deref(), Some("0,1"));
                assert_eq!(size, 16);
            }
            other => panic!("expected CorpusAction::Write, got {other:?}"),
        },
        other => panic!("expected Command::Corpus, got {other:?}"),
    }

    let cli = Cli::try_parse_from(["glyim-bench", "corpus", "check"]).expect("parse corpus check");
    match cli.command {
        Command::Corpus { action } => match action {
            crate::cli::CorpusAction::Check { dir } => {
                assert_eq!(dir, Path::new("perf/corpus"));
            }
            other => panic!("expected CorpusAction::Check, got {other:?}"),
        },
        other => panic!("expected Command::Corpus, got {other:?}"),
    }
}
