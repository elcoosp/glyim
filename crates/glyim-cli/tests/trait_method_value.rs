//! Regression tests for trait-method paths used as values vs callees.
//!
//! `check_path`'s trait-method arm (`Trait::method`) returns a benign error
//! node because the concrete impl is only known at a `Call` site (where
//! static/virtual dispatch rewrites the callee). A trait method used as a
//! *value* (`let x = T::f;`) has no such rewrite, so the error node survived
//! to codegen and ICEd with "Attempted to lower TyKind::Error".
//!
//! The fix adds an `in_callee_position` flag: the diagnostic fires only for
//! the value case, while call sites keep working.

use std::io::Write;
use std::process::Command;

fn tempdir() -> std::path::PathBuf {
    let p = std::env::temp_dir().join(format!(
        "glyim_tmv_{}_{}",
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

/// A trait method used as a value must produce a clean diagnostic, never an
/// ICE.
#[test]
fn trait_method_as_value_is_a_diagnostic_not_an_ice() {
    let dir = tempdir();
    let src = dir.join("v.g");
    let mut f = std::fs::File::create(&src).unwrap();
    writeln!(
        f,
        "trait T {{ fn f(&self); }}\n\
         fn main() {{ let x = T::f; let _ = x; }}"
    )
    .unwrap();
    drop(f);

    let out = dir.join("v.o");
    let output = Command::new(cli_bin())
        .args([
            src.to_str().unwrap(),
            "--emit=obj",
            "-o",
            out.to_str().unwrap(),
        ])
        .output()
        .expect("spawn glyim-cli");

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("panicked") && !stderr.contains("ICE"),
        "a trait-method value use must not ICE; got:\n{stderr}",
    );
    assert!(
        !output.status.success(),
        "a trait-method value use must be an error",
    );
    assert!(
        stderr.contains("cannot be used as a value"),
        "expected a clear `cannot be used as a value` diagnostic; got:\n{stderr}",
    );

    let _ = std::fs::remove_dir_all(&dir);
}

/// A normal method call (with a concrete impl) must still compile.
#[test]
fn trait_method_call_still_compiles() {
    let dir = tempdir();
    let src = dir.join("c.g");
    let mut f = std::fs::File::create(&src).unwrap();
    writeln!(
        f,
        "trait T {{ fn f(&self) -> i32; }}\n\
         struct S;\n\
         impl T for S {{ fn f(&self) -> i32 {{ 7 }} }}\n\
         fn main() {{ let s = S; let x = s.f(); let _ = x; }}"
    )
    .unwrap();
    drop(f);

    let out = dir.join("c.o");
    let output = Command::new(cli_bin())
        .args([
            src.to_str().unwrap(),
            "--emit=obj",
            "-o",
            out.to_str().unwrap(),
        ])
        .output()
        .expect("spawn glyim-cli");

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "a trait method call must still compile; exit={:?}\nstderr:\n{stderr}",
        output.status.code(),
    );

    let _ = std::fs::remove_dir_all(&dir);
}
