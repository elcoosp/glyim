//! LL-10 regression: `&&`/`||` must short-circuit.
//!
//! They were lowered to an eager `BinaryOp(And/Or)` (→ `build_and`/`build_or`
//! in LLVM), so the RHS ran unconditionally — `i < len && arr[i] == x` read
//! `arr[i]` even when the bounds check failed. The fix desugars `&&`/`||` into
//! control flow (SwitchInt + a destination local), mirroring the `If` arm.

use std::io::Write;
use std::process::Command;

fn tempdir() -> std::path::PathBuf {
    let p = std::env::temp_dir().join(format!(
        "glyim_shortcircuit_{}_{}",
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
    std::path::PathBuf::from(env!("CARGO_BIN_EXE_glyim-cli"))
}

fn mir_of(dir: &std::path::Path, src_text: &str) -> String {
    let src = dir.join("s.g");
    let mut f = std::fs::File::create(&src).unwrap();
    f.write_all(src_text.as_bytes()).unwrap();
    drop(f);
    let out = dir.join("s.mir");
    let output = Command::new(cli_bin())
        .args([
            src.to_str().unwrap(),
            "--emit=mir",
            "-o",
            out.to_str().unwrap(),
        ])
        .output()
        .expect("spawn glyim-cli");
    assert!(
        output.status.success(),
        "must compile; stderr:\n{}",
        String::from_utf8_lossy(&output.stderr),
    );
    std::fs::read_to_string(&out).unwrap()
}

/// `a && b` must lower to control flow (a SwitchInt on `a`), NOT an eager
/// `BinaryOp(And)`.
#[test]
fn logical_and_lowers_to_control_flow() {
    let dir = tempdir();
    let mir = mir_of(
        &dir,
        "fn main() { let a = true; let b = false; let c = a && b; let _ = c; }",
    );
    assert!(
        !mir.contains("BinaryOp(And"),
        "`&&` must NOT lower to an eager BinaryOp(And):\n{mir}"
    );
    assert!(
        mir.contains("SwitchInt"),
        "`&&` must lower to a SwitchInt (short-circuit):\n{mir}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// `a || b` likewise.
#[test]
fn logical_or_lowers_to_control_flow() {
    let dir = tempdir();
    let mir = mir_of(
        &dir,
        "fn main() { let a = true; let b = false; let c = a || b; let _ = c; }",
    );
    assert!(
        !mir.contains("BinaryOp(Or"),
        "`||` must NOT lower to an eager BinaryOp(Or):\n{mir}"
    );
    assert!(
        mir.contains("SwitchInt"),
        "`||` must lower to a SwitchInt (short-circuit):\n{mir}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// Bitwise `&`/`|` on integers must STILL be eager `BitAnd`/`BitOr` (the
/// short-circuit desugar must not touch them).
#[test]
fn bitwise_and_or_stay_eager() {
    let dir = tempdir();
    let mir = mir_of(&dir, "fn main() { let x = 6 & 3; let _ = x; }");
    assert!(
        mir.contains("BinaryOp(BitAnd"),
        "integer `&` must remain an eager BitAnd:\n{mir}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}
