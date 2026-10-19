//! End-to-end `--lto=thin`: emits per-CGU bitcode, thin-links it with
//! `llvm-lto2` (including the `-r=` symbol-resolution directives it
//! requires), and links the resulting object into a runnable binary.
//!
//! Skipped on hosts without `llvm-lto2` (the compile must then fail with a
//! diagnostic naming the tool — covered by the unit test in `linker.rs`).

use std::io::Write;
use std::process::Command;

fn cli_bin() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_BIN_EXE_glyim-cli"))
}

fn tempdir() -> std::path::PathBuf {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let p = std::env::temp_dir().join(format!("glyim_thinlto_e2e_{}_{}", std::process::id(), n));
    std::fs::create_dir_all(&p).unwrap();
    p
}

/// A program whose functions must survive the thin-link (each is a
/// separate symbol the merge could drop).
const SRC: &str = "\
fn a() -> i32 { 1 }
fn b() -> i32 { 2 }
fn c() -> i32 { 3 }
fn d() -> i32 { 4 }
fn main() -> i32 { a() + b() + c() + d() }
";

#[test]
fn thin_lto_multi_cgu_produces_runnable_binary() {
    let dir = tempdir();
    let src = dir.join("s.g");
    std::fs::File::create(&src)
        .unwrap()
        .write_all(SRC.as_bytes())
        .unwrap();
    let out = dir.join("s.out");

    let compile = Command::new(cli_bin())
        .args([
            src.to_str().unwrap(),
            "--with-stdlib",
            "--emit=exec",
            "--lto=thin",
            "--codegen-units=4",
            "-o",
            out.to_str().unwrap(),
        ])
        .output()
        .expect("spawn glyim-cli");

    if !compile.status.success() {
        let stderr = String::from_utf8_lossy(&compile.stderr);
        // Host without llvm-lto2: the diagnostic must name the tool.
        assert!(
            stderr.contains("llvm-lto2"),
            "thin-LTO compile failed for an unexpected reason:\n{stderr}"
        );
        let _ = std::fs::remove_dir_all(&dir);
        return;
    }

    let run = Command::new(&out).status().expect("run binary");
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(
        run.code(),
        Some(10),
        "a()+b()+c()+d() must equal 10 after thin-LTO"
    );
}
