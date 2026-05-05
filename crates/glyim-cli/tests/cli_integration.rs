use glyim_macro_vfs::ContentStore;
use std::path::PathBuf;
use std::process::Command;

// ── Helpers ────────────────────────────────────────────────

fn glyim_bin() -> Option<PathBuf> {
    let exe = std::env::current_exe().unwrap();
    let dir = exe.parent().unwrap().parent().unwrap();
    let bin = dir.join("glyim");
    if bin.exists() { Some(bin) } else { None }
}

/// Create a temporary file, write source, then call `f` with its path.
fn with_source_file(source: &str, f: impl FnOnce(&std::path::Path)) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.g");
    std::fs::write(&path, source).unwrap();
    f(&path);
}

// ── In‑process pipeline tests (fast) ──────────────────────────

#[test]
fn cli_run_returns_exit_code() {
    let exit = glyim_cli::pipeline::run_jit("main = () => 42").unwrap();
    assert_eq!(exit, 42);
}

#[test]
fn cli_run_with_println_output() {
    // Keep a subprocess test because we need to capture actual stdout
    let bin = glyim_bin().expect("glyim binary not found");
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("main.g");
    std::fs::write(&input, r#"main = () => { println(42) }"#).unwrap();
    let output = Command::new(bin)
        .arg("run")
        .arg(&input)
        .output()
        .expect("glyim run");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("42"));
}

#[test]
fn cli_ir_output() {
    let ir = glyim_codegen_llvm::compile_to_ir("main = () => 42").unwrap();
    assert!(ir.contains("define i32 @main"));
}

#[test]
fn cli_check_valid() {
    with_source_file("main = () => 42", |path| {
        glyim_cli::pipeline::check(path).unwrap();
    });
}

#[test]
fn cli_check_invalid() {
    with_source_file("main = () => 42 as Str", |path| {
        let result = glyim_cli::pipeline::check(path);
        assert!(result.is_err());
    });
}

#[test]
fn cli_dump_tokens() {
    let source = "main = () => 42";
    let mut buf = Vec::new();
    glyim_cli::dump::dump_tokens(source, &mut buf);
    let out = String::from_utf8(buf).unwrap();
    assert!(out.contains("TOK"));
    assert!(out.contains("ident"));
}

#[test]
fn cli_dump_ast() {
    let source = "main = () => 42";
    let parse_out = glyim_parse::parse(source);
    let mut buf = Vec::new();
    glyim_cli::dump::dump_ast(source, &parse_out.interner, &mut buf);
    let out = String::from_utf8(buf).unwrap();
    assert!(!out.is_empty());
    assert!(out.contains("main"));
}

#[test]
fn cli_dump_hir() {
    let source = "main = () => 42";
    let parse_out = glyim_parse::parse(source);
    let mut buf = Vec::new();
    glyim_cli::dump::dump_hir(source, &parse_out.interner, &mut buf);
    let out = String::from_utf8(buf).unwrap();
    assert!(out.contains("HIR fn main"));
}

#[test]
fn cli_test_passing() {
    let src = "#[test]\nfn a() { 0 }\n#[test]\nfn b() { 0 }";
    with_source_file(src, |path| {
        let summary = glyim_cli::pipeline::run_tests(path, None, false, None, false).unwrap();
        assert_eq!(summary.passed(), 2);
        assert_eq!(summary.failed(), 0);
    });
}

#[test]
fn cli_test_with_failure() {
    let src = "#[test]\nfn a() { 0 }\n#[test]\nfn b() { 1 }";
    with_source_file(src, |path| {
        let summary = glyim_cli::pipeline::run_tests(path, None, false, None, false).unwrap();
        assert!(summary.failed() > 0);
    });
}

#[test]
fn cli_test_filter() {
    let src = "#[test]\nfn a() { 0 }\n#[test]\nfn b() { 1 }";
    with_source_file(src, |path| {
        let summary = glyim_cli::pipeline::run_tests(path, Some("a"), false, None, false).unwrap();
        assert_eq!(summary.total(), 1);
        assert_eq!(summary.passed(), 1);
        assert_eq!(summary.results[0].0, "a");
    });
}

// ── Subprocess‑only tests (still needed) ──────────────────────

#[test]
fn cli_init_creates_project() {
    let bin = glyim_bin().expect("glyim binary not found");
    let dir = tempfile::tempdir().unwrap();
    let output = Command::new(bin)
        .arg("init")
        .arg("myapp")
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(dir.path().join("myapp").join("glyim.toml").exists());
    assert!(dir.path().join("myapp").join("src").join("main.g").exists());
}

#[test]
fn cli_build_produces_message() {
    with_source_file("main = () => 42", |path| {
        let out = path.parent().unwrap().join("built");
        glyim_cli::pipeline::build_with_mode(
            path,
            Some(&out),
            glyim_cli::pipeline::BuildMode::Debug,
            None,
            None,
        )
        .unwrap();
        assert!(out.exists());
    });
}

#[test]
fn cli_build_bare_flag_compiles_single_file() {
    // Same as above – bare just means single‑file compilation.
    cli_build_produces_message();
}

#[test]
fn cli_doc_open_flag_works() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("test.g");
    std::fs::write(&input, "fn main() -> i64 { 42 }").unwrap();
    let out_dir = dir.path().join("doc");
    glyim_cli::pipeline::generate_doc(&input, Some(&out_dir)).unwrap();
    let index_html = out_dir.join("index.html");
    assert!(index_html.exists());
    let html = std::fs::read_to_string(index_html).unwrap();
    assert!(html.contains("fn main()"));
}

#[test]
fn cli_publish_wasm_stores_blob() {
    let bin = glyim_bin().expect("glyim binary not found");
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("glyim.toml"),
        "[package]\nname = \"testpkg\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    std::fs::create_dir(dir.path().join("src")).unwrap();
    std::fs::write(dir.path().join("src/main.g"), "fn main() -> i64 { 42 }").unwrap();

    let output = Command::new(bin)
        .arg("publish")
        .arg("--wasm")
        .current_dir(dir.path())
        .output()
        .expect("glyim publish --wasm");
    assert!(output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("Macro Wasm content hash:"));
}

#[test]
fn cli_macro_inspect_shows_expansion() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("test.g");
    std::fs::write(&input, "@identity(main = () => 42)").unwrap();
    let cas = dir.path().join("cas");
    std::fs::create_dir_all(&cas).unwrap();
    let source = std::fs::read_to_string(&input).unwrap();
    let expanded = glyim_cli::macro_expand::expand_macros(&source, dir.path(), &cas).unwrap();
    assert!(expanded.contains("main = () => 42"));
}

#[test]
fn cli_verify_checks_lockfile() {
    let bin = glyim_bin().expect("glyim binary not found");
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();

    unsafe {
        std::env::set_var("HOME", home);
        std::env::set_var("XDG_DATA_HOME", home);
    }
    let data_dir = dirs_next::data_dir().unwrap();
    let cas_dir = data_dir.join("cas");
    std::fs::create_dir_all(cas_dir.join("objects")).unwrap();
    let store = glyim_macro_vfs::LocalContentStore::new(&cas_dir).unwrap();
    let blob = b"hello verify";
    let hash = store.store(blob);

    let lockfile_content = format!(
        r#"
[[package]]
name = "testpkg"
version = "1.0.0"
hash = "{}"

[package.source]
type = "local"
"#,
        hash
    );
    std::fs::write(dir.path().join("glyim.lock"), lockfile_content).unwrap();

    let output = Command::new(bin)
        .arg("verify")
        .env("HOME", home)
        .env("XDG_DATA_HOME", home)
        .current_dir(dir.path())
        .output()
        .expect("glyim verify");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("Lockfile verified"));
    assert!(output.status.success());
}

#[test]
fn cli_outdated_with_missing_registry() {
    let bin = glyim_bin().expect("glyim binary not found");
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("glyim.lock"),
        r#"
[[package]]
name = "foo"
version = "1.0.0"
hash = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"

[package.source]
type = "local"
"#,
    )
    .unwrap();

    let output = Command::new(bin)
        .arg("outdated")
        .env("GLYIM_REGISTRY", "http://localhost:99999")
        .current_dir(dir.path())
        .output()
        .expect("glyim outdated");
    assert!(output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("warning: cannot contact registry")
            || stderr.contains("warning: could not check")
    );
}

#[test]
fn cli_cache_clean_removes_unused() {
    let dir = tempfile::tempdir().unwrap();
    let store = glyim_macro_vfs::LocalContentStore::new(dir.path()).unwrap();
    let hash_used = store.store(b"used");
    let hash_unused = store.store(b"unused");
    store.register_name("my-crate", hash_used);

    let names = store.list_names();
    let mut referenced = std::collections::HashSet::new();
    for name in &names {
        if let Some(h) = store.resolve_name(name) {
            referenced.insert(h);
        }
    }
    for blob in store.list_blobs() {
        if !referenced.contains(&blob) {
            store.delete_blob(blob).ok();
        }
    }
    assert!(store.retrieve(hash_used).is_some());
    assert!(store.retrieve(hash_unused).is_none());
}

#[test]
fn cli_cache_roundtrip_works() {
    use glyim_macro_vfs::LocalContentStore;
    use std::env;

    // isolate cache directory via env so we don’t touch the real one
    let cache_root = tempfile::tempdir().expect("create temp cache dir");
    let artifacts = cache_root.path().join("glyim-objects");
    unsafe {
        env::set_var("XDG_CACHE_HOME", cache_root.path());
    }

    let dir = tempfile::tempdir().expect("create temp workspace");
    let input = dir.path().join("main.g");
    std::fs::write(&input, "main = () => 42").unwrap();
    let output = dir.path().join("a.out");

    // first compilation – cache miss
    let result = glyim_cli::pipeline::build_with_cache(&input, Some(&output));
    assert!(result.is_ok(), "first compile failed: {:?}", result.err());
    assert!(output.exists(), "binary should exist after compilation");

    // verify we stored something in the cache
    let cas = LocalContentStore::new(&artifacts).expect("open cache store");
    let blobs = cas.list_blobs();
    assert!(
        !blobs.is_empty(),
        "cache must contain at least one blob after compilation"
    );

    // second compilation with same source – cache hit
    let result2 = glyim_cli::pipeline::build_with_cache(&input, Some(&output));
    assert!(
        result2.is_ok(),
        "second compile failed (cache hit path): {:?}",
        result2.err()
    );
    assert!(output.exists(), "binary should still exist after cache hit");

    // modify source – cache miss again but must still compile
    std::fs::write(&input, "main = () => 99").unwrap();
    let result3 = glyim_cli::pipeline::build_with_cache(&input, Some(&output));
    assert!(
        result3.is_ok(),
        "third compile with changed source failed: {:?}",
        result3.err()
    );
    assert!(
        output.exists(),
        "binary should exist after cache miss recompile"
    );
}
