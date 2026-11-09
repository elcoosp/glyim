//! M1.4 tagging codemod: rewrite every fixture header into the canonical
//! `//@` grammar and derive the readiness-ledger tags.
//!
//! The rewrite is pure and deterministic ([`rewrite_fixture`]); the binary
//! `glyim-tag-fixtures` (see `src/bin`) only walks the fixture tree and
//! applies it. Per fixture the codemod:
//!
//! 1. converts every legacy `// key:` directive line in the leading comment
//!    block into its canonical `//@` spelling, in place, preserving prose
//!    comment lines;
//! 2. derives `//@ feature:` tags from a curated keyword table over the
//!    fixture's repository-relative path, always appending the fixture
//!    *mode* feature (e.g. `TOOL-TEST-RUN-PASS`) so coverage is total by
//!    construction — coarse where the name is uninformative, precise where
//!    the construct is named. Re-running with a better table is the
//!    intended refinement path (the tags are evidence hints for
//!    `ledger compute`, M1.8, not status claims);
//! 3. derives `//@ backends:` (`gam` everywhere; `gam, llvm` when the
//!    fixture is run in CI under the native backend or declares
//!    `needs-llvm`);
//! 4. makes the outcome expectation explicit: run-pass fixtures get
//!    `//@ expect-exit: 0` and run-fail fixtures `//@ expect-exit: 1` when
//!    nothing else pins the outcome (the strategies' defaults).

use std::path::Path;

use super::directives::parse_directives;
use crate::harness::config::TestMode;

/// The registry feature a fixture is tagged with by virtue of its corpus
/// directory (the harness-level claim; always true by construction).
#[must_use]
pub fn mode_feature(dir_key: &str) -> &'static str {
    match dir_key {
        "run-pass" => "TOOL-TEST-RUN-PASS",
        "run-fail" => "TOOL-TEST-RUN-FAIL",
        "compile-pass" | "typeck" | "codegen" => "TOOL-TEST-COMPILE-PASS",
        "compile-fail" => "TOOL-TEST-COMPILE-FAIL",
        "ui" => "TOOL-TEST-UI",
        "no-ice" => "TOOL-TEST-NO-ICE",
        // The runtime/m5 corpus is the async runtime proof (see
        // `tests/runtime.rs`): native LLVM codegen + link + execute.
        "runtime" => "TOOL-TEST-RUN-PASS",
        _ => "TOOL-TEST-COMPILE-PASS",
    }
}

/// Curated keyword table: `(needle tokens, feature IDs)` matched against
/// the lowercased, non-alphanumeric-split token stream of the fixture's
/// repository-relative path (directories included). A needle matches when
/// its tokens appear contiguously (single-token needles also match as a
/// token prefix when at least 4 chars long, so `index` matches
/// `indexing`). `generic` carries a negative-predecessor guard so
/// `non_generic_call` is not mis-tagged as monomorphization evidence.
///
/// Every ID here was checked against `ledger/features/**` (216 entries,
/// M1.2 seed); `bin/glyim-tag-fixtures --verify-ids` re-checks the table
/// against the registry on demand.
pub const KEYWORD_FEATURES: &[(&[&str], &[&str])] = &[
    (&["atomic"], &["RUNTIME-FFI-ATOMIC"]),
    (&["closure"], &["LANG-CLOSURE-EXPR"]),
    (&["closure", "move"], &["LANG-CLOSURE-MOVE"]),
    (&["closure", "fnptr"], &["LANG-TY-FN-PTR"]),
    (&["for", "loop"], &["LANG-FOR-SLICE-ITER"]),
    (&["for", "range"], &["LANG-FOR-SLICE-ITER"]),
    (&["for", "slice"], &["LANG-FOR-SLICE-ITER"]),
    (&["match"], &["LANG-MATCH-GUARDS"]),
    (&["pattern"], &["LANG-MATCH-GUARDS"]),
    (&["enum"], &["LANG-SYN-ENUM"]),
    (&["struct"], &["LANG-SYN-STRUCT"]),
    (&["struct", "update"], &["LANG-SYN-STRUCT-UPDATE"]),
    (&["update", "spread"], &["LANG-SYN-STRUCT-UPDATE"]),
    (&["tuple", "struct"], &["LANG-SYN-TUPLE-STRUCT"]),
    (&["unit"], &["LANG-SYN-TUPLE-STRUCT"]),
    (&["ctor"], &["LANG-SYN-TUPLE-STRUCT"]),
    (&["impl"], &["LANG-SYN-IMPL"]),
    (&["self"], &["LANG-SYN-SELF"]),
    (&["if", "let"], &["LANG-SYN-IF-LET"]),
    (&["if"], &["LANG-SYN-IF"]),
    (&["while"], &["LANG-SYN-WHILE"]),
    (&["loop", "break"], &["LANG-SYN-LOOP-BREAK"]),
    (&["break"], &["LANG-SYN-LOOP-BREAK"]),
    (&["return"], &["LANG-SYN-RETURN"]),
    (&["precedence"], &["LANG-SYN-OPERATORS"]),
    (&["parens"], &["LANG-SYN-OPERATORS"]),
    (&["chained"], &["LANG-SYN-OPERATORS"]),
    (&["arith"], &["LANG-SYN-OPERATORS"]),
    (&["div"], &["LANG-SYN-OPERATORS"]),
    (&["rem"], &["LANG-SYN-OPERATORS"]),
    (&["cast"], &["LANG-SYN-OPERATORS"]),
    (&["checked"], &["LANG-SYN-OPERATORS"]),
    (&["saturating"], &["LANG-SYN-OPERATORS"]),
    (&["wrapping"], &["LANG-SYN-OPERATORS"]),
    (&["bit"], &["LANG-SYN-OPERATORS"]),
    (&["power"], &["LANG-SYN-OPERATORS"]),
    (&["compound", "assign"], &["LANG-SYN-COMPOUND-ASSIGN"]),
    (&["field"], &["LANG-SYN-FIELD-ACCESS"]),
    (&["access"], &["LANG-SYN-FIELD-ACCESS"]),
    (&["methods"], &["LANG-SYN-FIELD-ACCESS"]),
    (&["array"], &["LANG-TY-ARRAY"]),
    (&["slice"], &["LANG-TY-SLICE", "STD-CORE-SLICE"]),
    (&["vec"], &["LANG-BUILTIN-VEC", "STD-ALLOC-VEC"]),
    (&["push"], &["STD-ALLOC-STRING"]),
    (&["string"], &["LANG-BUILTIN-STRING", "STD-ALLOC-STRING"]),
    (&["str"], &["LANG-TY-STR"]),
    (&["trim"], &["LANG-TY-STR"]),
    (&["contains"], &["LANG-TY-STR"]),
    (&["starts"], &["LANG-TY-STR"]),
    (&["utf8"], &["RUNTIME-FFI-VECSTR"]),
    (&["tuple"], &["LANG-TY-TUPLE"]),
    (&["ref", "mut"], &["LANG-TY-REF-MUT", "LANG-SYN-MUT"]),
    (&["ref"], &["LANG-TY-REF"]),
    (&["alias"], &["LANG-TY-REF"]),
    (&["raw"], &["LANG-TY-RAW-PTR"]),
    (&["ptr"], &["LANG-TY-RAW-PTR", "STD-CORE-PTR"]),
    (&["fn", "ptr"], &["LANG-TY-FN-PTR"]),
    (&["generic", "fn"], &["COMPILER-MONO-MONOMORPHIZE"]),
    (&["generic", "method"], &["COMPILER-MONO-MONOMORPHIZE"]),
    (&["generic", "struct"], &["COMPILER-MONO-MONOMORPHIZE"]),
    (&["generic", "two"], &["COMPILER-MONO-MONOMORPHIZE"]),
    (&["generic", "identity"], &["COMPILER-MONO-MONOMORPHIZE"]),
    (&["generic", "iterator"], &["COMPILER-MONO-MONOMORPHIZE"]),
    (&["turbofish"], &["COMPILER-MONO-MONOMORPHIZE"]),
    (&["unused", "generic"], &["COMPILER-MONO-POLYMORPHIZE"]),
    (&["option"], &["LANG-BUILTIN-OPTION", "STD-CORE-OPTION"]),
    (&["some"], &["STD-CORE-OPTION"]),
    (&["none"], &["STD-CORE-OPTION"]),
    (&["unwrap"], &["STD-CORE-OPTION"]),
    (&["result"], &["LANG-BUILTIN-RESULT", "STD-CORE-RESULT"]),
    (&["ok"], &["STD-CORE-RESULT"]),
    (&["err"], &["STD-CORE-RESULT"]),
    (&["try"], &["LANG-ERR-TRY-RESULT"]),
    (&["static"], &["LANG-SCOPE-STATIC-ITEM"]),
    (
        &["const"],
        &["LANG-SYN-CONST-ITEM", "COMPILER-INTERP-CONST-EVAL"],
    ),
    (&["unsafe"], &["LANG-SYN-UNSAFE-BLOCK"]),
    (
        &["unsafecell"],
        &["LANG-BUILTIN-UNSAFECELL", "STD-CORE-CELL"],
    ),
    (&["maybe"], &["STD-CORE-MEM"]),
    (&["assert"], &["STD-CORE-PANIC"]),
    (&["panic"], &["STD-CORE-PANIC", "RUNTIME-FFI-PANIC"]),
    (&["unreachable"], &["STD-CORE-PANIC"]),
    (&["abort"], &["RUNTIME-FFI-PROCESS"]),
    (&["exit"], &["RUNTIME-FFI-PROCESS"]),
    (&["thread"], &["STD-STD-THREAD", "RUNTIME-FFI-THREAD"]),
    (&["env"], &["STD-STD-ENV", "RUNTIME-FFI-ENV"]),
    (&["fs"], &["STD-STD-FS", "RUNTIME-FFI-FS"]),
    (&["net"], &["STD-STD-NET", "RUNTIME-FFI-NET"]),
    (&["tcp"], &["STD-STD-NET", "RUNTIME-FFI-NET"]),
    (&["udp"], &["STD-STD-NET", "RUNTIME-FFI-NET"]),
    (&["time"], &["STD-STD-TIME", "RUNTIME-FFI-TIME"]),
    (&["sleep"], &["STD-STD-TIME", "RUNTIME-FFI-TIME"]),
    (&["mutex"], &["STD-STD-SYNC"]),
    (&["rwlock"], &["STD-STD-SYNC"]),
    (&["condvar"], &["STD-STD-SYNC", "RUNTIME-FFI-CONDVAR"]),
    (&["barrier"], &["STD-STD-SYNC"]),
    (&["oncelock"], &["STD-STD-SYNC"]),
    (&["async"], &["LANG-ASYNC-AWAIT"]),
    (&["await"], &["LANG-ASYNC-AWAIT"]),
    (&["include"], &["LANG-MACRO-BUILTIN-INCLUDE"]),
    (&["macro"], &["LANG-MACRO-RULES"]),
    (&["println"], &["STD-STD-IO", "RUNTIME-FFI-IO"]),
    (&["print"], &["STD-STD-IO", "RUNTIME-FFI-IO"]),
    (&["stdout"], &["STD-STD-IO", "RUNTIME-FFI-IO"]),
    (&["stderr"], &["STD-STD-IO", "RUNTIME-FFI-IO"]),
    (&["format"], &["STD-STD-IO", "RUNTIME-FFI-IO"]),
    (&["mod"], &["LANG-SCOPE-MOD-DECL"]),
    (&["module"], &["LANG-SCOPE-MOD-DECL"]),
    (&["modules"], &["LANG-SCOPE-MOD-DECL"]),
    (&["size"], &["COMPILER-LAYOUT-SIZE-ALIGN"]),
    (&["align"], &["COMPILER-LAYOUT-SIZE-ALIGN"]),
    (&["mem"], &["COMPILER-LAYOUT-SIZE-ALIGN"]),
    (&["clone"], &["STD-CORE-MARKER"]),
    (&["copy"], &["STD-CORE-MARKER"]),
    (&["iter"], &["STD-CORE-ITER"]),
    (&["into", "iter"], &["STD-CORE-ITER"]),
    (&["drop"], &["COMPILER-OPT-DROP-ELAB", "STD-CORE-OPS"]),
    (&["write"], &["LANG-SYN-MUT"]),
    (&["mut"], &["LANG-SYN-MUT"]),
    (&["shadow"], &["LANG-SCOPE-SHADOWING"]),
    (&["trait"], &["LANG-TRAIT-IMPL"]),
    (&["default"], &["LANG-TRAIT-DEFAULT-METHODS"]),
    (&["attr"], &["LANG-SCOPE-ATTR-ALLOW"]),
    (&["allow"], &["LANG-SCOPE-ATTR-ALLOW"]),
    (&["lifetime"], &["COMPILER-BCK-NLL"]),
    (&["huge"], &["LANG-TY-PRIMITIVES"]),
    (&["int", "to", "string"], &["STD-STD-INT-STR"]),
    (&["to", "string"], &["STD-STD-INT-STR"]),
    (&["parse"], &["STD-CORE-PARSE"]),
    (&["function"], &["LANG-SYN-FN-DEF"]),
    (&["fn"], &["LANG-SYN-FN-DEF"]),
    (&["call"], &["LANG-SYN-FN-DEF"]),
    (&["recursion"], &["LANG-SYN-FN-DEF"]),
    (&["infer"], &["COMPILER-TI-UNIFICATION"]),
    (&["guard"], &["LANG-MATCH-GUARDS"]),
    (&["box"], &["LANG-BUILTIN-BOX", "STD-ALLOC-BOX"]),
    (&["rc"], &["STD-ALLOC-RC"]),
    (&["mutex"], &["STD-STD-SYNC"]),
    (&["spawn"], &["STD-STD-THREAD", "RUNTIME-FFI-THREAD"]),
    (&["iter"], &["STD-CORE-ITER"]),
    (&["cmp"], &["STD-CORE-CMP"]),
    (&["swap"], &["STD-CORE-MEM"]),
    (&["replace"], &["STD-CORE-MEM"]),
    (&["file"], &["STD-STD-FS", "RUNTIME-FFI-FS"]),
    (&["read"], &["STD-STD-IO", "RUNTIME-FFI-IO"]),
    (&["coercion"], &["LANG-TY-REF"]),
    (&["resolution"], &["COMPILER-FRONT-DEFTABLE"]),
    (&["glue"], &["COMPILER-OPT-DROP-ELAB"]),
    (&["default"], &["STD-CORE-DEFAULT"]),
    (&["concat"], &["STD-ALLOC-STRING"]),
    (&["extend"], &["STD-ALLOC-VEC"]),
    (&["pop"], &["STD-ALLOC-VEC"]),
    (&["projection"], &["COMPILER-FRONT-HIR"]),
    (&["panic"], &["STD-CORE-PANIC"]),
    (&["unconstrained"], &["COMPILER-TI-UNIFICATION"]),
    (&["mismatched"], &["COMPILER-TI-UNIFICATION"]),
    (&["annotation"], &["COMPILER-TI-UNIFICATION"]),
    (&["unresolved"], &["COMPILER-FRONT-DEFTABLE"]),
    (&["no", "method"], &["COMPILER-FRONT-DEFTABLE"]),
    (&["unknown", "field"], &["COMPILER-FRONT-DEFTABLE"]),
    (&["missing", "field"], &["COMPILER-FRONT-DEFTABLE"]),
    (&["missing", "module"], &["COMPILER-FRONT-DEFTABLE"]),
];

/// Split a path into lowercase tokens on non-alphanumeric characters.
fn tokenize(rel: &str) -> Vec<String> {
    rel.to_ascii_lowercase()
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|t| !t.is_empty())
        .map(ToString::to_string)
        .collect()
}

/// Registry feature IDs implied by the fixture path (keyword table only,
/// no mode fallback).
#[must_use]
pub fn keyword_features(rel: &str) -> Vec<&'static str> {
    let tokens = tokenize(rel);
    let mut out: Vec<&'static str> = Vec::new();
    'needles: for (needle, ids) in KEYWORD_FEATURES {
        let n = needle.len();
        if n > tokens.len() {
            continue;
        }
        for start in 0..=(tokens.len() - n) {
            let matched = if n == 1 {
                let needle = needle[0];
                let tok = &tokens[start];
                let hit = tok == needle || (needle.len() >= 4 && tok.starts_with(needle));
                // `generic` guard: `non_generic_*` is *not* monomorphization
                // evidence.
                if hit && needle == "generic" && start > 0 && tokens[start - 1] == "non" {
                    false
                } else {
                    hit
                }
            } else {
                needle
                    .iter()
                    .zip(&tokens[start..start + n])
                    .all(|(n_tok, t_tok)| n_tok == t_tok)
            };
            if matched {
                for id in *ids {
                    if !out.contains(id) {
                        out.push(id);
                    }
                }
                continue 'needles;
            }
        }
    }
    out
}

/// Backend list for a fixture: `gam` always; `llvm` added when the fixture
/// runs under the native backend in CI (run corpora, the async runtime
/// proof) or explicitly declares `needs-llvm`; `bytecode` replaces `llvm`
/// when the fixture pins `--backend=bytecode` (the stack-machine executor).
#[must_use]
pub fn backends_for(
    dir_key: &str,
    needs_llvm: bool,
    backend_flag: Option<&str>,
) -> Vec<&'static str> {
    if backend_flag == Some("bytecode") {
        return vec!["gam", "bytecode"];
    }
    let runs_native = matches!(dir_key, "run-pass" | "run-fail" | "runtime")
        || needs_llvm
        || backend_flag == Some("llvm");
    if runs_native {
        vec!["gam", "llvm"]
    } else {
        vec!["gam"]
    }
}

/// The `--backend=` value declared by a fixture's compile-flags, if any.
fn backend_flag_of(
    legacy: &LegacyHeader,
    parsed: &super::directives::FixtureAnnotations,
) -> Option<&'static str> {
    for flags in legacy
        .compile_flags
        .iter()
        .chain(parsed.compile_flags.iter())
    {
        for word in flags.split_whitespace() {
            if let Some(v) = word.strip_prefix("--backend=") {
                return match v {
                    "llvm" => Some("llvm"),
                    "bytecode" => Some("bytecode"),
                    _ => None,
                };
            }
        }
    }
    None
}

/// The mode implied by the fixture's corpus directory (`no-ice` has none —
/// its contract is "must not panic", not a compile-pass claim).
#[must_use]
pub fn mode_for_dir(dir_key: &str) -> Option<TestMode> {
    match dir_key {
        "run-pass" | "runtime" => Some(TestMode::RunPass),
        "run-fail" => Some(TestMode::RunFail),
        "compile-pass" | "typeck" | "codegen" => Some(TestMode::CompilePass),
        "compile-fail" => Some(TestMode::CompileFail),
        "ui" => Some(TestMode::Ui),
        _ => None,
    }
}

/// Rewrite one fixture's header into the canonical `//@` grammar with
/// derived tags. `dir_key` is the corpus directory name (first path
/// component under `tests/`); `rel` is the full repository-relative path
/// used by the keyword table.
///
/// # Errors
/// Errors when the embedded `//@` header (if any) does not parse, or when
/// the fixture has no recognizable corpus directory.
pub fn rewrite_fixture(rel: &str, dir_key: &str, source: &str) -> Result<String, String> {
    let parsed =
        parse_directives(source).map_err(|(line, msg)| format!("{rel}: line {line}: {msg}"))?;

    // Split into (leading comment block | code). The block includes blank
    // lines; trailing blanks of the block are re-emitted *after* the
    // derived directives so the header stays one contiguous block.
    let lines: Vec<&str> = source.lines().collect();
    let code_start = lines
        .iter()
        .position(|l| {
            let t = l.trim();
            !t.is_empty() && !t.starts_with("//")
        })
        .unwrap_or(lines.len());
    let block = &lines[..code_start];
    let rest = &lines[code_start..];

    // Legacy values (the directives parser saw only `//@` lines).
    let legacy = LegacyHeader::parse(block);

    // 1. Convert legacy directive lines in place; keep prose and `//@`.
    let mut out_lines: Vec<String> = block
        .iter()
        .map(|l| convert_legacy_line(l.trim_start()).unwrap_or_else(|| (*l).to_string()))
        .collect();
    // Trailing blank lines of the block move after the derived directives.
    while out_lines.last().is_some_and(|l| l.trim().is_empty()) {
        out_lines.pop();
    }
    let block_blank_tail = block.len() - out_lines.len();

    // 2. Derive missing canonical directives.
    if parsed.mode.is_none()
        && legacy.test_mode.is_none()
        && let Some(mode) = mode_for_dir(dir_key)
    {
        out_lines.push(format!("//@ mode: {}", mode.dir_name()));
    }

    let declared = &parsed.features;
    let mut derived: Vec<&'static str> = keyword_features(rel);
    // The `runtime/m5` corpus is the async runtime proof (see
    // `tests/runtime.rs`); other runtime fixtures are not async claims.
    if rel.starts_with("runtime/m5") {
        derived.push("LANG-ASYNC-AWAIT");
    }
    derived.push(mode_feature(dir_key));
    let new_ids: Vec<&&'static str> = derived
        .iter()
        .filter(|id| !declared.iter().any(|d| d == **id))
        .collect();
    // Preserve declared order first, then derived additions (sorted).
    if !new_ids.is_empty() {
        let ids: Vec<&str> = if declared.is_empty() {
            derived.clone()
        } else {
            new_ids.iter().map(|&&id| id).collect()
        };
        out_lines.push(format!("//@ feature: {}", ids.join(", ")));
    }

    if parsed.backends.is_none() {
        let backend_flag = backend_flag_of(&legacy, &parsed);
        let bs = backends_for(
            dir_key,
            legacy.needs_llvm || parsed.needs_llvm,
            backend_flag,
        );
        out_lines.push(format!("//@ backends: {}", bs.join(", ")));
    }

    // Outcome expectation: only for run fixtures; the strategies' defaults
    // (0 for run-pass, 1 for run-fail) are made explicit.
    let has_outcome = parsed.expect_exit.is_some()
        || legacy.exit_code.is_some()
        || parsed.expect_stdout.is_some()
        || legacy.check_stdout.is_some()
        || parsed.expect_stderr.is_some()
        || legacy.check_stderr.is_some();
    if !has_outcome {
        match parsed.mode.or_else(|| mode_for_dir(dir_key)) {
            Some(TestMode::RunPass) => out_lines.push("//@ expect-exit: 0".to_string()),
            Some(TestMode::RunFail) => out_lines.push("//@ expect-exit: 1".to_string()),
            _ => {}
        }
    }

    for _ in 0..block_blank_tail {
        out_lines.push(String::new());
    }

    // 3. Reassemble.
    let mut result = out_lines.join("\n");
    if !rest.is_empty() {
        if !result.is_empty() {
            result.push('\n');
        }
        result.push_str(&rest.join("\n"));
    }
    if source.ends_with('\n') {
        result.push('\n');
    }
    Ok(result)
}

/// Legacy header values (parsed from `// key:` lines only).
#[derive(Default, Debug)]
struct LegacyHeader {
    test_mode: Option<String>,
    exit_code: Option<String>,
    check_stdout: Option<String>,
    check_stderr: Option<String>,
    needs_llvm: bool,
    compile_flags: Vec<String>,
}

impl LegacyHeader {
    fn parse(header: &[&str]) -> Self {
        let mut h = Self::default();
        for line in header {
            let t = line.trim();
            if !t.starts_with("//") {
                continue;
            }
            let content = t[2..].trim();
            if let Some(v) = content.strip_prefix("test-mode:") {
                h.test_mode = Some(v.trim().to_string());
            } else if let Some(v) = content.strip_prefix("exit-code:") {
                h.exit_code = Some(v.trim().to_string());
            } else if let Some(v) = content.strip_prefix("check-stdout:") {
                h.check_stdout = Some(v.trim().to_string());
            } else if let Some(v) = content.strip_prefix("check-stderr:") {
                h.check_stderr = Some(v.trim().to_string());
            } else if let Some(v) = content.strip_prefix("compile-flags:") {
                h.compile_flags.push(v.trim().to_string());
            } else if content == "needs-llvm" {
                h.needs_llvm = true;
            }
        }
        h
    }
}

/// Convert one legacy directive line to its canonical `//@` spelling.
/// Returns `None` when the line is not a recognized legacy directive
/// (prose comments pass through untouched).
fn convert_legacy_line(line: &str) -> Option<String> {
    let t = line.trim();
    if !t.starts_with("//") || t.starts_with("//@") {
        return None;
    }
    let content = t[2..].trim();
    let canonical = |key: &str, value: &str| Some(format!("//@ {key}: {value}"));

    if let Some(v) = content.strip_prefix("test-mode:") {
        // `test-mode: no-ice` appears in the no-ICE corpus: "no-ice" is not
        // a `TestMode` and the value was always ignored there. Converting
        // it would produce an invalid directive, so drop the dead line and
        // let the directory-derived tags take over.
        if v.trim().parse::<TestMode>().is_ok() {
            return canonical("mode", v.trim());
        }
        return Some(String::new());
    }
    if let Some(v) = content.strip_prefix("exit-code:") {
        return canonical("expect-exit", v.trim());
    }
    if let Some(v) = content.strip_prefix("check-stdout:") {
        return canonical("expect-stdout", v.trim());
    }
    if let Some(v) = content.strip_prefix("check-stderr:") {
        return canonical("expect-stderr", v.trim());
    }
    if let Some(v) = content.strip_prefix("error-pattern:") {
        return canonical("expect-error", v.trim());
    }
    if let Some(v) = content.strip_prefix("expect-warning:") {
        return canonical("expect-warning", v.trim());
    }
    if let Some(v) = content.strip_prefix("forbid-warning:") {
        return canonical("forbid-warning", v.trim());
    }
    if let Some(v) = content.strip_prefix("only-target:") {
        return canonical("only-target", v.trim());
    }
    if let Some(v) = content.strip_prefix("min-version:") {
        return canonical("min-version", v.trim());
    }
    if let Some(v) = content.strip_prefix("timeout:") {
        return canonical("timeout", v.trim());
    }
    if let Some(v) = content.strip_prefix("aux-file:") {
        return canonical("aux-file", v.trim());
    }
    if let Some(v) = content.strip_prefix("compile-flags:") {
        return canonical("compile-flags", v.trim());
    }
    if let Some(v) = content.strip_prefix("revisions:") {
        return canonical("revisions", v.trim());
    }
    // Revision-scoped: `// [rev] compile-flags: ...`
    if content.starts_with('[')
        && let Some(end) = content.find(']')
        && content[end + 1..].trim().starts_with("compile-flags:")
    {
        let rev = &content[..=end];
        let v = content[end + 1..].trim().strip_prefix("compile-flags:")?;
        return Some(format!("//@ {rev} compile-flags: {}", v.trim()));
    }
    if content == "needs-llvm" {
        return Some("//@ needs-llvm".to_string());
    }
    if content == "ignore" {
        return Some("//@ ignore".to_string());
    }
    None
}

/// Directory key of a fixture under the corpus root (`run-pass`, `ui`, …).
#[must_use]
pub fn dir_key_of(fixture_root: &Path, path: &Path) -> Option<String> {
    let rel = path.strip_prefix(fixture_root).ok()?;
    rel.components()
        .next()?
        .as_os_str()
        .to_str()
        .map(String::from)
}

#[cfg(test)]
mod tests;
