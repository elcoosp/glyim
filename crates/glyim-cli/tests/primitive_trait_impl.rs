//! Regression tests for `impl Trait for <primitive>` resolution.
//!
//! `impl_method_fns` used to be keyed on `AdtId` and only matched
//! `TyKind::Adt` receivers, so a trait impl whose `Self` type is a primitive
//! (`impl FromStr for i32`) was registered under a phantom id and never
//! found — `"42".parse::<i32>()` reported an unsatisfied bound even with a
//! matching user impl. The table is now keyed on the `Self` `Ty`.
//!
//! Two distinct angles are pinned here:
//!
//! 1. `user_impl_on_primitive_resolves` — a *user-provided* impl on a
//!    primitive (`impl MyTrait for u16`) is found by method resolution. This
//!    is the original regression: if the impl table regresses to `AdtId`
//!    keying, this fails. A non-stdlib trait and type are used so the test
//!    exercises user code only.
//!
//! 2. `stdlib_fromstr_for_i32_is_available` — the standard library's own
//!    `impl FromStr for i32` (in `parse.g`) is reachable, so
//!    `"42".parse::<i32>()` type-checks *without* a user-supplied impl.

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

/// Original regression: a user `impl` on a *primitive* `Self` type must
/// resolve. Uses a user trait + `u16` so it does not collide with the
/// stdlib's `FromStr for i32`.
#[test]
fn user_impl_on_primitive_resolves() {
    let dir = tempdir();
    let src = dir.join("p.g");
    let mut f = std::fs::File::create(&src).unwrap();
    writeln!(
        f,
        "trait MyTrait {{ fn get(&self) -> u16; }}\n\
         impl MyTrait for u16 {{\n\
         \x20   fn get(&self) -> u16 {{ *self }}\n\
         }}\n\
         fn main() {{ let x: u16 = 5; let _ = x.get(); }}"
    )
    .unwrap();
    drop(f);

    let out = dir.join("p.o");
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
        "`impl MyTrait for u16` must resolve; exit={:?}\nstderr:\n{stderr}",
        output.status.code(),
    );
    assert!(
        !stderr.contains("is not satisfied"),
        "a provided `impl MyTrait for u16` must satisfy the bound; got:\n{stderr}",
    );
    assert!(out.exists(), "expected an object at {out:?}");

    let _ = std::fs::remove_dir_all(&dir);
}

/// The stdlib's `impl FromStr for i32` (in `parse.g`) must satisfy the
/// `T: FromStr` bound at `str::parse::<i32>`'s call site, with *no* user
/// impl in the program.
#[test]
fn stdlib_fromstr_for_i32_is_available() {
    let dir = tempdir();
    let src = dir.join("p.g");
    let mut f = std::fs::File::create(&src).unwrap();
    writeln!(
        f,
        "fn main() {{\n\
         \x20   let n = \"42\".parse::<i32>();\n\
         \x20   let _ = n;\n\
         }}"
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
        "the stdlib `impl FromStr for i32` must satisfy the bound; \
         exit={:?}\nstderr:\n{stderr}",
        output.status.code(),
    );
    assert!(
        !stderr.contains("is not satisfied") && !stderr.contains("conflicting"),
        "stdlib `FromStr for i32` must not conflict or miss the bound; got:\n{stderr}",
    );
    assert!(out.exists(), "expected an object at {out:?}");

    let _ = std::fs::remove_dir_all(&dir);
}

/// Every integer primitive's `FromStr` impl (in `parse.g`) must satisfy the
/// `T: FromStr` bound at `str::parse::<T>()`. Exercises the boundary value of
/// each type (max for unsigned, min for signed) in a single program.
#[test]
fn stdlib_fromstr_available_for_all_integer_primitives() {
    let dir = tempdir();
    let src = dir.join("p.g");
    let mut f = std::fs::File::create(&src).unwrap();
    writeln!(
        f,
        "fn main() {{\n\
         \x20   let a = \"255\".parse::<u8>(); let _ = a;\n\
         \x20   let b = \"65535\".parse::<u16>(); let _ = b;\n\
         \x20   let c = \"4294967295\".parse::<u32>(); let _ = c;\n\
         \x20   let d = \"18446744073709551615\".parse::<u64>(); let _ = d;\n\
         \x20   let e = \"42\".parse::<usize>(); let _ = e;\n\
         \x20   let f = \"-128\".parse::<i8>(); let _ = f;\n\
         \x20   let g = \"-32768\".parse::<i16>(); let _ = g;\n\
         \x20   let h = \"-2147483648\".parse::<i32>(); let _ = h;\n\
         \x20   let i = \"-9223372036854775808\".parse::<i64>(); let _ = i;\n\
         \x20   let j = \"-42\".parse::<isize>(); let _ = j;\n\
         }}"
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
        "every stdlib integer FromStr impl must satisfy the bound; \
         exit={:?}\nstderr:\n{stderr}",
        output.status.code(),
    );
    assert!(
        !stderr.contains("is not satisfied") && !stderr.contains("conflicting"),
        "stdlib integer FromStr impls must not conflict or miss the bound; got:\n{stderr}",
    );
    assert!(out.exists(), "expected an object at {out:?}");

    let _ = std::fs::remove_dir_all(&dir);
}
