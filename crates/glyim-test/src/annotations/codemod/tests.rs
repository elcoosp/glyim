//! Tests for the M1.4 tagging codemod (`annotations::codemod`).

use crate::annotations::codemod::{
    backends_for, keyword_features, mode_feature, mode_for_dir, rewrite_fixture,
};
use crate::annotations::directives::parse_directives;
use crate::harness::config::TestMode;

#[test]
fn legacy_header_is_converted_in_place() {
    let src = "// test-mode: run-pass\n// exit-code: 42\nfn main() -> i32 { 42 }\n";
    let out = rewrite_fixture("run-pass/x.g", "run-pass", src).unwrap();
    assert!(out.contains("//@ mode: run-pass"), "{out}");
    assert!(out.contains("//@ expect-exit: 42"), "{out}");
    assert!(!out.contains("test-mode:"), "{out}");
    assert!(!out.contains("exit-code:"), "{out}");
    // Legacy lines converted in their original spots; derived tags follow
    // before the untouched code body.
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines[0], "//@ mode: run-pass");
    assert_eq!(lines[1], "//@ expect-exit: 42");
    assert!(lines[2].starts_with("//@ feature:"), "{out}");
    assert!(lines[3].starts_with("//@ backends:"), "{out}");
    assert_eq!(lines[4], "fn main() -> i32 { 42 }");
}

#[test]
fn prose_comments_survive_the_rewrite() {
    let src = "// Regression note: this fixture once needed `// needs-llvm`.\n\
               // test-mode: run-pass\nfn main() {}\n";
    let out = rewrite_fixture("run-pass/x.g", "run-pass", src).unwrap();
    assert!(
        out.starts_with("// Regression note: this fixture once needed `// needs-llvm`."),
        "{out}"
    );
    assert!(out.contains("//@ mode: run-pass"));
}

#[test]
fn derived_tags_cover_feature_backends_and_default_exit() {
    let src = "fn main() {}\n"; // no header at all
    let out = rewrite_fixture("compile-pass/empty_main.g", "compile-pass", src).unwrap();
    let a = parse_directives(&out).unwrap();
    assert!(a.present);
    assert_eq!(a.mode, Some(TestMode::CompilePass));
    assert!(a.features.contains(&"TOOL-TEST-COMPILE-PASS".to_string()));
    assert_eq!(a.backends, Some(vec!["gam".to_string()]));
}

#[test]
fn run_pass_without_exit_code_gets_explicit_zero() {
    let src = "fn main() {}\n";
    let out = rewrite_fixture("run-pass/x.g", "run-pass", src).unwrap();
    assert!(out.contains("//@ expect-exit: 0"), "{out}");
    assert!(out.contains("//@ backends: gam, llvm"), "{out}");
}

#[test]
fn run_fail_without_exit_code_gets_explicit_one() {
    let src = "fn main() { panic!(\"x\") }\n";
    let out = rewrite_fixture("run-fail/x.g", "run-fail", src).unwrap();
    assert!(out.contains("//@ expect-exit: 1"), "{out}");
}

#[test]
fn existing_exit_code_is_preserved_not_duplicated() {
    let src = "// test-mode: run-pass\n// exit-code: 42\nfn main() -> i32 { 42 }\n";
    let out = rewrite_fixture("run-pass/x.g", "run-pass", src).unwrap();
    assert_eq!(out.matches("//@ expect-exit").count(), 1, "{out}");
    assert!(out.contains("//@ expect-exit: 42"));
}

#[test]
fn rewrite_is_idempotent() {
    let src = "// A prose line.\n// test-mode: run-pass\n// check-stdout: hi\nfn main() {}\n";
    let once = rewrite_fixture("run-pass/stdout_x.g", "run-pass", src).unwrap();
    let twice = rewrite_fixture("run-pass/stdout_x.g", "run-pass", &once).unwrap();
    assert_eq!(once, twice, "second pass must be a no-op");
}

#[test]
fn keyword_table_maps_named_constructs() {
    let f = keyword_features("run-pass/behavior/atomic_usize_fetch_add.g");
    assert!(f.contains(&"RUNTIME-FFI-ATOMIC"), "{f:?}");

    let f = keyword_features("run-pass/behavior/closure_move_capture.g");
    assert!(f.contains(&"LANG-CLOSURE-EXPR"), "{f:?}");
    assert!(f.contains(&"LANG-CLOSURE-MOVE"), "{f:?}");

    let f = keyword_features("run-pass/behavior/for_loop_over_slice.g");
    assert!(f.contains(&"LANG-FOR-SLICE-ITER"), "{f:?}");

    let f = keyword_features("run-pass/behavior/try_operator_option_none.g");
    assert!(f.contains(&"LANG-ERR-TRY-RESULT"), "{f:?}");
    assert!(
        f.contains(&"LANG-ERR-TRY-OPTION") || f.contains(&"STD-CORE-OPTION"),
        "{f:?}"
    );

    let f = keyword_features("run-pass/behavior/vec_clone.g");
    assert!(f.contains(&"STD-ALLOC-VEC"), "{f:?}");

    // Token-boundary: `if` must not match inside `unification`.
    let f = keyword_features("typeck/unification_stubs.g");
    assert!(!f.contains(&"LANG-SYN-IF"), "{f:?}");

    // `non_generic_*` must not claim monomorphization evidence.
    let f = keyword_features("run-pass/non_generic_call.g");
    assert!(!f.contains(&"COMPILER-MONO-MONOMORPHIZE"), "{f:?}");
    assert!(f.contains(&"LANG-SYN-FN-DEF"), "{f:?}");

    // Prefix rule: `index` matches `indexing`.
    let f = keyword_features("run-pass/str/str_indexing_works.g");
    assert!(
        f.contains(&"LANG-SYN-INDEX") || f.contains(&"LANG-TY-STR"),
        "{f:?}"
    );
}

#[test]
fn every_keyword_feature_id_is_registry_shaped() {
    // The codemod only emits IDs verified against the M1.2 registry; the
    // bin re-checks against `ledger/features/**` on demand
    // (`--verify-ids`). Here we pin the shape contract.
    for (needle, ids) in crate::annotations::codemod::KEYWORD_FEATURES {
        assert!(!needle.is_empty(), "empty needle in table");
        for id in *ids {
            assert!(
                crate::annotations::directives::is_valid_feature_id(id),
                "malformed ID {id} in keyword table"
            );
        }
    }
}

#[test]
fn mode_features_and_dirs() {
    assert_eq!(mode_feature("run-pass"), "TOOL-TEST-RUN-PASS");
    assert_eq!(mode_feature("run-fail"), "TOOL-TEST-RUN-FAIL");
    assert_eq!(mode_feature("compile-pass"), "TOOL-TEST-COMPILE-PASS");
    assert_eq!(mode_feature("typeck"), "TOOL-TEST-COMPILE-PASS");
    assert_eq!(mode_feature("compile-fail"), "TOOL-TEST-COMPILE-FAIL");
    assert_eq!(mode_feature("ui"), "TOOL-TEST-UI");
    assert_eq!(mode_feature("no-ice"), "TOOL-TEST-NO-ICE");

    assert_eq!(mode_for_dir("run-pass"), Some(TestMode::RunPass));
    assert_eq!(mode_for_dir("no-ice"), None, "no-ice is not a TestMode");
}

#[test]
fn backends_follow_the_ci_reality() {
    assert_eq!(backends_for("run-pass", false, None), vec!["gam", "llvm"]);
    assert_eq!(backends_for("run-fail", false, None), vec!["gam", "llvm"]);
    assert_eq!(backends_for("runtime", false, None), vec!["gam", "llvm"]);
    assert_eq!(backends_for("compile-pass", false, None), vec!["gam"]);
    assert_eq!(backends_for("ui", false, None), vec!["gam"]);
    assert_eq!(backends_for("no-ice", false, None), vec!["gam"]);
    assert_eq!(
        backends_for("compile-pass", true, None),
        vec!["gam", "llvm"]
    );
    // A fixture pinning `--backend=bytecode` exercises the stack-machine
    // executor, not the native backend.
    assert_eq!(
        backends_for("runtime", false, Some("bytecode")),
        vec!["gam", "bytecode"]
    );
}

#[test]
fn runtime_dir_claims_the_async_proof() {
    let src = "fn main() {}\n";
    let out = rewrite_fixture("runtime/m5/one_step.g", "runtime", src).unwrap();
    assert!(out.contains("LANG-ASYNC-AWAIT"), "{out}");
    assert!(out.contains("//@ mode: run-pass"), "{out}");
}

#[test]
fn header_blank_line_stays_between_header_and_code() {
    let src = "// test-mode: compile-pass\n\nfn main() {}\n";
    let out = rewrite_fixture("compile-pass/x.g", "compile-pass", src).unwrap();
    let lines: Vec<&str> = out.lines().collect();
    // Derived directives must come BEFORE the blank separator.
    let blank = lines.iter().position(|l| l.trim().is_empty()).unwrap();
    assert!(blank > 0);
    for l in &lines[..blank] {
        assert!(l.starts_with("//") || l.trim().is_empty(), "{l}");
    }
    assert_eq!(lines[blank + 1], "fn main() {}");
}

#[test]
fn invalid_embedded_annotation_is_an_error() {
    let src = "//@ wat: 1\nfn main() {}\n";
    assert!(rewrite_fixture("compile-pass/x.g", "compile-pass", src).is_err());
}
