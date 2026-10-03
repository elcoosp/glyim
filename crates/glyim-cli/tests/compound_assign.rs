//! Regression: compound assignment (`+=`, `-=`, …) must lower to
//! `lhs = lhs <op> rhs`, not `lhs = rhs`.
//!
//! `lower_assign_expr` ignored the operator token entirely, so `i += 1`
//! lowered to `i = 1` — which made `while i < 1000 { i += 1; }` loop forever
//! (i was reset to 1 each iteration) and segfault programs reading the result.

use std::io::Write;
use std::process::Command;

fn tempdir() -> std::path::PathBuf {
    let p = std::env::temp_dir().join(format!(
        "glyim_compound_{}_{}",
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

/// `i += 1` must produce a `BinaryOp(Add, (copy i, 1))`, not a bare `i = 1`.
#[test]
fn plus_assign_lowers_to_add() {
    let dir = tempdir();
    let mir = mir_of(
        &dir,
        "fn main() { let mut i = 0; i += 1; let _ = i; }",
    );
    assert!(
        mir.contains("BinaryOp(Add"),
        "`i += 1` must lower to `i = i + 1` (BinaryOp Add); got:\n{mir}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// `x *= 3` must produce a `BinaryOp(Mul, …)`.
#[test]
fn star_assign_lowers_to_mul() {
    let dir = tempdir();
    let mir = mir_of(
        &dir,
        "fn main() { let mut x = 2; x *= 3; let _ = x; }",
    );
    assert!(
        mir.contains("BinaryOp(Mul"),
        "`x *= 3` must lower to `x = x * 3` (BinaryOp Mul); got:\n{mir}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// Plain `=` must remain a plain assignment (no spurious BinaryOp).
#[test]
fn plain_assign_is_not_a_binary_op() {
    let dir = tempdir();
    let mir = mir_of(&dir, "fn main() { let mut x = 2; x = 5; let _ = x; }");
    assert!(
        !mir.contains("BinaryOp(Add") && !mir.contains("BinaryOp(Mul"),
        "plain `=` must not synthesize a binary op; got:\n{mir}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}
