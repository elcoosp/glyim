//! Contract tests for the `//@` fixture annotation grammar (M1.4).

use super::{KNOWN_BACKENDS, is_valid_feature_id, parse_directives};
use crate::harness::config::TestMode;

fn parse(src: &str) -> super::FixtureAnnotations {
    parse_directives(src).expect("fixture should parse")
}

fn parse_err(src: &str) -> String {
    match parse_directives(src) {
        Ok(_) => panic!("expected parse error, got Ok"),
        Err((_, msg)) => msg,
    }
}

#[test]
fn empty_source_has_no_annotations() {
    let a = parse("");
    assert!(!a.present);
    assert!(a.features.is_empty());
    assert!(!a.has_outcome_expectation());
}

#[test]
fn feature_tag_single_and_multi() {
    let a = parse("//@ feature: LANG-TY-ARRAY\nfn main() {}\n");
    assert!(a.present);
    assert_eq!(a.features, vec!["LANG-TY-ARRAY".to_string()]);

    let a = parse("//@ feature: LANG-TY-ARRAY, LANG-SYN-ENUM\n");
    assert_eq!(
        a.features,
        vec!["LANG-TY-ARRAY".to_string(), "LANG-SYN-ENUM".to_string()]
    );

    let a = parse("//@ feature: LANG-TY-ARRAY\n//@ feature: COMPILER-MONO-MONOMORPHIZE\n");
    assert_eq!(
        a.features,
        vec![
            "LANG-TY-ARRAY".to_string(),
            "COMPILER-MONO-MONOMORPHIZE".to_string()
        ]
    );
}

#[test]
fn feature_rejects_malformed_ids_and_duplicates() {
    let msg = parse_err("//@ feature: lang.ty-array\n");
    assert!(msg.contains("malformed feature ID"), "{msg}");

    let msg = parse_err("//@ feature: LANG-TY-ARRAY\n//@ feature: LANG-TY-ARRAY\n");
    assert!(msg.contains("duplicate feature ID"), "{msg}");

    let msg = parse_err("//@ feature: -LEADING\n");
    assert!(msg.contains("malformed feature ID"), "{msg}");
}

#[test]
fn mode_and_backends_validate() {
    let a = parse("//@ mode: run-pass\n//@ backends: gam, llvm\n");
    assert_eq!(a.mode, Some(TestMode::RunPass));
    assert_eq!(a.backends, Some(vec!["gam".into(), "llvm".into()]));

    let msg = parse_err("//@ mode: super-pass\n");
    assert!(msg.contains("unknown test-mode"), "{msg}");

    let msg = parse_err("//@ backends: gpu\n");
    assert!(msg.contains("unknown backend `gpu`"), "{msg}");
    assert!(msg.contains("gam, llvm, cranelift, bytecode"), "{msg}");

    let msg = parse_err("//@ backends: gam, gam\n");
    assert!(msg.contains("duplicate backend"), "{msg}");
}

#[test]
fn backend_set_matches_registry_serialization() {
    // The ledger's Backend serde names; the two lists must not drift.
    assert_eq!(KNOWN_BACKENDS, &["gam", "llvm", "cranelift", "bytecode"]);
}

#[test]
fn expect_family_parses() {
    let a = parse(
        "//@ expect-exit: 42\n\
         //@ expect-stdout: hello\n\
         //@ expect-stderr: thread panicked\n\
         //@ expect-error: index out of bounds\n\
         //@ expect-error: no field\n\
         //@ expect-warning: unused\n\
         //@ forbid-warning: dead code\n",
    );
    assert_eq!(a.expect_exit, Some(42));
    assert_eq!(a.expect_stdout.as_deref(), Some("hello"));
    assert_eq!(a.expect_stderr.as_deref(), Some("thread panicked"));
    assert_eq!(a.expect_errors.len(), 2);
    assert_eq!(a.expect_warnings, vec!["unused".to_string()]);
    assert_eq!(a.forbid_warnings, vec!["dead code".to_string()]);
    assert!(a.has_outcome_expectation());
}

#[test]
fn flags_parse_without_value() {
    let a = parse("//@ needs-llvm\n//@ ignore\n");
    assert!(a.needs_llvm);
    assert!(a.ignore);
}

#[test]
fn harness_keys_parse() {
    let a = parse(
        "//@ only-target: x86_64-unknown-linux-gnu\n\
         //@ min-version: 0.6.0\n\
         //@ timeout: 120\n\
         //@ aux-file: helper.g\n\
         //@ compile-flags: --with-stdlib -O\n\
         //@ revisions: v1 v2\n\
         //@ [v2] compile-flags: --backend=llvm\n",
    );
    assert_eq!(a.only_target.as_deref(), Some("x86_64-unknown-linux-gnu"));
    assert_eq!(a.min_version.as_deref(), Some("0.6.0"));
    assert_eq!(a.timeout_secs, Some(120));
    assert_eq!(a.aux_files, vec![std::path::PathBuf::from("helper.g")]);
    assert_eq!(
        a.compile_flags,
        vec!["--with-stdlib".to_string(), "-O".to_string()]
    );
    assert_eq!(a.revisions, vec!["v1".to_string(), "v2".to_string()]);
    assert_eq!(
        a.revision_compile_flags.get("v2").map(Vec::as_slice),
        Some(&["--backend=llvm".to_string()][..])
    );
}

#[test]
fn unknown_keys_are_hard_errors() {
    let msg = parse_err("//@ features: LANG-TY-ARRAY\n"); // plural typo
    assert!(
        msg.contains("unknown `//@` directive key `features`"),
        "{msg}"
    );

    let msg = parse_err("//@ wat: 1\n");
    assert!(msg.contains("unknown `//@` directive key `wat`"), "{msg}");

    let msg = parse_err("//@ wat\n");
    assert!(msg.contains("unknown directive"), "{msg}");

    let msg = parse_err("//@\n");
    assert!(msg.contains("empty"), "{msg}");
}

#[test]
fn duplicates_of_singular_keys_are_errors() {
    for src in [
        "//@ mode: ui\n//@ mode: ui\n",
        "//@ backends: gam\n//@ backends: gam\n",
        "//@ expect-exit: 0\n//@ expect-exit: 0\n",
        "//@ expect-stdout: a\n//@ expect-stdout: a\n",
        "//@ expect-stderr: a\n//@ expect-stderr: a\n",
        "//@ only-target: x\n//@ only-target: x\n",
        "//@ min-version: 1\n//@ min-version: 1\n",
        "//@ timeout: 5\n//@ timeout: 5\n",
    ] {
        let res = parse_directives(src);
        assert!(res.is_err(), "expected duplicate error for {src:?}");
    }
}

#[test]
fn valueless_key_is_an_error() {
    let msg = parse_err("//@ feature:\n");
    assert!(msg.contains("requires a value"), "{msg}");
}

#[test]
fn directives_after_code_are_not_headers() {
    // The header block ends at the first code line; a later `//@` is a
    // comment, not a directive (mirrors parse_test_config).
    let a = parse("fn main() {}\n//@ feature: LANG-TY-ARRAY\n");
    assert!(!a.present);
}

#[test]
fn errors_report_one_based_line_numbers() {
    // Blank lines are skipped; the bad directive sits at source line 3.
    let src = "\n\n//@ feature: bad-id\n";
    match parse_directives(src) {
        Ok(_) => panic!("expected error"),
        Err((line, _)) => assert_eq!(line, 3),
    }
}

#[test]
fn prose_comments_are_ignored() {
    let a = parse(
        "// A regression note: this fixture once needed `// needs-llvm`.\n\
         // exit-code mentioned in prose stays prose.\n\
         //@ mode: run-pass\n",
    );
    assert!(a.present);
    assert_eq!(a.mode, Some(TestMode::RunPass));
    assert!(!a.needs_llvm);
}

#[test]
fn feature_id_shape_check() {
    assert!(is_valid_feature_id("LANG-TY-ARRAY"));
    assert!(is_valid_feature_id("X"));
    assert!(is_valid_feature_id("RUNTIME-FFI-VECSTR"));
    assert!(!is_valid_feature_id("lang.ty-array"));
    assert!(!is_valid_feature_id("lang_ty_array"));
    assert!(!is_valid_feature_id(""));
    assert!(!is_valid_feature_id("-X"));
    assert!(!is_valid_feature_id("X-"));
    assert!(!is_valid_feature_id("X--Y"));
}
