//! TDD tests for `glyim-cli --with-stdlib`.
//!
//! RED state before implementation: `--with-stdlib` is not a recognised flag.
//! GREEN after: the flag prepends the assembled stdlib + a generated prelude
//! to the user's source so `println`, `Vec`, `Option`, `Result`, etc. resolve
//! without explicit `use` statements.

use std::io::Write;
use std::process::Command;

fn tempdir() -> std::path::PathBuf {
    let p = std::env::temp_dir().join(format!(
        "glyim_cli_withstdlib_{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    std::fs::create_dir_all(&p).unwrap();
    p
}

fn cli_bin() -> std::path::PathBuf {
    // Cargo sets CARGO_BIN_EXE_<name> for integration tests.
    std::path::PathBuf::from(env!("CARGO_BIN_EXE_glyim-cli"))
}

fn host_triple() -> String {
    // Best-effort: read from rustc -vV output.
    let out = Command::new("rustc").args(["-vV"]).output().unwrap();
    for line in String::from_utf8_lossy(&out.stdout).lines() {
        if let Some(rest) = line.strip_prefix("host: ") {
            return rest.trim().to_string();
        }
    }
    "unknown".to_string()
}

#[test]
fn with_stdlib_flag_is_accepted() {
    // RED before implementation: unknown flag → non-zero exit + help text.
    // GREEN after: flag accepted; we use `--emit=mir` so no native toolchain
    // is required (works on every CI host).
    let dir = tempdir();
    let src = dir.join("hello.g");
    let mut f = std::fs::File::create(&src).unwrap();
    writeln!(f, "fn main() {{ println(\"hi\"); }}").unwrap();
    drop(f);

    let out_mir = dir.join("hello.mir");
    let status = Command::new(cli_bin())
        .args([
            src.to_str().unwrap(),
            "--with-stdlib",
            "--emit=mir",
            "--target",
            &host_triple(),
            "-o",
            out_mir.to_str().unwrap(),
        ])
        .status()
        .expect("spawn glyim-cli");
    assert!(
        status.success(),
        "--with-stdlib --emit=mir must compile println without diagnostics; \
         got exit {:?}",
        status.code()
    );
    assert!(out_mir.exists(), "expected {out_mir:?} to be written");
}

#[test]
fn without_with_stdlib_println_is_unresolved() {
    // Sanity: the flag is the difference-maker. Without it, `println` is not
    // in scope (matches the wave-2 probe finding).
    let dir = tempdir();
    let src = dir.join("hello.g");
    let mut f = std::fs::File::create(&src).unwrap();
    writeln!(f, "fn main() {{ println(\"hi\"); }}").unwrap();
    drop(f);

    let out_mir = dir.join("hello.mir");
    let output = Command::new(cli_bin())
        .args([
            src.to_str().unwrap(),
            "--emit=mir",
            "--target",
            &host_triple(),
            "-o",
            out_mir.to_str().unwrap(),
        ])
        .output()
        .expect("spawn glyim-cli");
    assert!(
        !output.status.success(),
        "expected non-zero without --with-stdlib (println not in scope)"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let combined = format!("{stderr}\n{stdout}");
    assert!(
        combined.contains("println"),
        "expected an unresolved-name diagnostic mentioning `println`; got: {combined}"
    );
}
