//! Regression test: `impl Trait for <primitive>` must resolve.
//!
//! `impl_method_fns` used to be keyed on `AdtId` and only matched
//! `TyKind::Adt` receivers, so a trait impl whose `Self` type is a primitive
//! (`impl FromStr for i32`) was registered under a phantom id and never
//! found — `"42".parse::<i32>()` reported an unsatisfied bound even with a
//! matching user impl. The table is now keyed on the `Self` `Ty`.
//!
//! If the table regresses to `AdtId` keying, this test fails.

use std::io::Write;
use std::process::Command;

fn tempdir() -> std::path::PathBuf {
    let p = std::env::temp_dir().join(format!(
        "glyim_prim_trait_{}_{}",
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

#[test]
fn impl_trait_for_primitive_resolves() {
    let dir = tempdir();
    let src = dir.join("p.g");
    let mut f = std::fs::File::create(&src).unwrap();
    writeln!(
        f,
        "impl FromStr for i32 {{\n\
         \x20   type Err = ();\n\
         \x20   fn from_str(s: &str) -> Result<i32, ()> {{ Result::Ok(0) }}\n\
         }}\n\
         fn main() {{ let x = \"42\".parse::<i32>(); }}"
    )
    .unwrap();
    drop(f);

    let out = dir.join("p.o");
    let output = Command::new(cli_bin())
        .args([
            src.to_str().unwrap(),
            "--with-stdlib",
            "--emit=obj",
            "-o",
            out.to_str().unwrap(),
        ])
        .output()
        .expect("spawn glyim-cli");

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "`impl FromStr for i32` must resolve; exit={:?}\nstderr:\n{stderr}",
        output.status.code(),
    );
    assert!(
        !stderr.contains("is not satisfied"),
        "a provided `impl FromStr for i32` must satisfy the bound; got:\n{stderr}",
    );
    assert!(out.exists(), "expected an object at {out:?}");

    let _ = std::fs::remove_dir_all(&dir);
}
