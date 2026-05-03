use std::path::PathBuf;
use std::process::{Command, Output};

/// Locate the `glyim` binary next to the test executable.
fn glyim_bin() -> Option<PathBuf> {
    let exe = std::env::current_exe().unwrap();
    let dir = exe.parent().unwrap().parent().unwrap();
    let bin = dir.join("glyim");
    if bin.exists() { Some(bin) } else { None }
}

/// Run `glyim` with given arguments on a temporary source file.
fn run_glyim(args: &[&str], source: &str) -> Option<Output> {
    let bin = glyim_bin()?;
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("main.g");
    std::fs::write(&input, source).unwrap();
    let mut cmd = Command::new(bin);
    for arg in args {
        cmd.arg(arg);
    }
    cmd.arg(&input);
    Some(cmd.output().expect("failed to execute glyim"))
}

macro_rules! try_glyim {
    ($args:expr, $src:expr) => {
        match run_glyim($args, $src) {
            Some(output) => output,
            None => return,
        }
    };
}

#[test]
fn cli_run_returns_exit_code() {
    let output = try_glyim!(&["run"], "main = () => 42");
    assert_eq!(output.status.code(), Some(42));
}

#[test]
fn cli_run_with_println_output() {
    let output = try_glyim!(&["run"], r#"main = () => { println(42) }"#);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("42"));
}

#[test]
fn cli_ir_output() {
    let output = try_glyim!(&["ir"], "main = () => 42");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("define i32 @main"));
}

#[test]
fn cli_check_valid() {
    let output = try_glyim!(&["check"], "main = () => 42");
    assert!(output.status.success());
}

#[test]
fn cli_check_invalid() {
    let output = try_glyim!(&["check"], "main = () => 42 as Str");
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("error") || stderr.contains("type mismatch"));
}

#[test]
fn cli_dump_tokens() {
    let output = try_glyim!(&["dump-tokens"], "main = () => 42");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("TOK"));
    assert!(stdout.contains("ident"));
}

#[test]
fn cli_dump_ast() {
    let output = try_glyim!(&["dump-ast"], "main = () => 42");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(!stdout.is_empty());
    assert!(stdout.contains("main"));
}

#[test]
fn cli_dump_hir() {
    let output = try_glyim!(&["dump-hir"], "main = () => 42");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("HIR fn main"));
}

#[test]
fn cli_test_passing() {
    let src = "#[test]\nfn a() { 0 }\n#[test]\nfn b() { 0 }";
    let output = try_glyim!(&["test"], src);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success());
    assert!(stderr.contains("2 passed"));
}

#[test]
fn cli_test_with_failure() {
    let src = "#[test]\nfn a() { 0 }\n#[test]\nfn b() { 1 }";
    let output = try_glyim!(&["test"], src);
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("FAILED"));
}

#[test]
fn cli_test_filter() {
    let src = "#[test]\nfn a() { 0 }\n#[test]\nfn b() { 1 }";
    let output = try_glyim!(&["test", "--filter", "a"], src);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success());
    assert!(stderr.contains("1 passed"));
    assert!(!stderr.contains("2 passed"));
}

#[test]
fn cli_init_creates_project() {
    let Some(bin) = glyim_bin() else {
        return;
    };
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
    let output = try_glyim!(&["build"], "main = () => 42");
    assert!(output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("Built:"));
}

#[test]
fn cli_doc_open_flag_works() {
    let Some(bin) = glyim_bin() else { return; };
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("test.g");
    std::fs::write(&input, "fn main() -> i64 { 42 }").unwrap();
    let out_dir = dir.path().join("outdoc");
    let output = std::process::Command::new(bin)
        .arg("doc")
        .arg(&input)
        .arg("--output")
        .arg(&out_dir)
        .output()
        .expect("glyim doc");
    assert!(output.status.success(), "doc command failed: {}
stderr: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr));
    let index_html = out_dir.join("index.html");
    assert!(index_html.exists(), "expected {} to exist", index_html.display());
}


#[test]
fn cli_publish_wasm_stores_blob() {
    let Some(bin) = glyim_bin() else { return; };
    let dir = tempfile::tempdir().unwrap();
    let toml_content = "[package]\nname = \"testpkg\"\nversion = \"0.1.0\"\n";
    std::fs::write(dir.path().join("glyim.toml"), toml_content).unwrap();
    let src_dir = dir.path().join("src");
    std::fs::create_dir(&src_dir).unwrap();
    let source = "fn main() -> i64 { 42 }";
    std::fs::write(src_dir.join("main.g"), source).unwrap();

    let output = std::process::Command::new(bin)
        .arg("publish")
        .arg("--wasm")
        .current_dir(dir.path())
        .output()
        .expect("failed to run glyim publish --wasm");
    assert!(output.status.success(), "publish --wasm exited with: {}", output.status);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("Macro Wasm content hash:"), "missing hash in output:\nstdout: {}\nstderr: {}", String::from_utf8_lossy(&output.stdout), stderr);
}

#[test]
fn cli_macro_inspect_shows_expansion() {
    let output = try_glyim!(&["macro-inspect"], "@identity(main = () => 42)");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Expanded:"), "missing Expanded section");
    assert!(stdout.contains("main = () => 42"), "missing expanded content");
}

