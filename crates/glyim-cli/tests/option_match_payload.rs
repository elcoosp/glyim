//! Regression test: matching an enum and binding/using the payload used to
//! ICE in codegen.
//!
//! Symptom: `IndexVec::get: index out of bounds` at
//! `glyim-core/src/arena.rs:173`, reached from
//! `glyim-codegen-llvm/src/lower.rs:544` (`place_ptr`, `ProjectionElem::Field`
//! over a multi-variant enum). The caller used
//! `layout.fields.offsets.get(FieldIdx::from_raw(1)).unwrap_or(0)` intending
//! a graceful "no tag prefix" fallback, but `IndexVec::get` carries a
//! `debug_assert!` that fires on an out-of-range index *before* returning
//! `None`. Any enum whose offsets table had fewer than two entries (a
//! single-variant / niche-tagged layout) panicked at compile time.
//!
//! Fix (commit `fix(codegen-llvm): bounds-check optional tag-prefix offset
//! reads`): read through `offsets.as_slice().get(n)` at the three sites that
//! want the optional fallback.
//!
//! The trigger is *any* multi-variant enum match that binds the payload. This
//! test uses a non-generic enum so it does not depend on the (separate,
//! still-open) layout bug for user-declared *generic* enums
//! (`[X0000] layout error building aggregate: UnknownType(Ty(0))`).

use std::io::Write;
use std::process::Command;

fn tempdir() -> std::path::PathBuf {
    let p = std::env::temp_dir().join(format!(
        "glyim_opt_match_{}_{}",
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
fn enum_match_payload_does_not_ice() {
    let dir = tempdir();
    let src = dir.join("o.g");
    let mut f = std::fs::File::create(&src).unwrap();
    writeln!(
        f,
        "enum MyOpt {{ Some(u64), None }}\n\
         fn unwrap_or_zero(o: MyOpt) -> u64 {{\n\
         \x20   match o {{\n\
         \x20       MyOpt::Some(v) => v,\n\
         \x20       MyOpt::None => 0,\n\
         \x20   }}\n\
         }}\n\
         fn main() {{\n\
         \x20   let a = unwrap_or_zero(MyOpt::Some(7));\n\
         \x20   let b = unwrap_or_zero(MyOpt::None);\n\
         \x20   let _ = a;\n\
         \x20   let _ = b;\n\
         }}"
    )
    .unwrap();
    drop(f);

    let out = dir.join("o.o");
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
        "compiling a match that binds an enum payload must not ICE; got:\n{stderr}",
    );
    assert!(
        output.status.success(),
        "the enum-match shape must compile; exit={:?}\nstderr:\n{stderr}",
        output.status.code(),
    );
    assert!(out.exists(), "expected an object at {out:?}");

    let _ = std::fs::remove_dir_all(&dir);
}

/// A user-declared generic enum's data-carrying variant, constructed at a
/// site with no external type constraint, must infer the enum's generic
/// argument from the constructor argument. Before the fix, the enum's
/// parameter stayed an unsolved inference var (`Adt1<<error>>` in MIR) and
/// codegen failed with `[X0000] layout error building aggregate:
/// UnknownType(Ty(0))`.
///
/// Root cause: `check_path`'s inline variant-constructor arm registered the
/// variant's *declared* field types (e.g. `Param(T)`) as the constructor's
/// input types without substituting them through the fresh inference vars,
/// and the call site only unified args against `Infer(Int/Float)` formals,
/// so a concrete `u64` argument never linked `T := u64`.
#[test]
fn generic_enum_data_variant_infers_type_arg() {
    let dir = tempdir();
    let src = dir.join("g.g");
    let mut f = std::fs::File::create(&src).unwrap();
    writeln!(
        f,
        "enum MyOpt<T> {{ S(T), N }}\n\
         fn main() {{\n\
         \x20   let a = MyOpt::S(7u64);\n\
         \x20   let _ = a;\n\
         }}"
    )
    .unwrap();
    drop(f);

    let out = dir.join("g.o");
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
        "compiling a generic enum data-variant constructor must not ICE; got:\n{stderr}",
    );
    assert!(
        output.status.success(),
        "the generic-enum data-variant shape must compile; exit={:?}\nstderr:\n{stderr}",
        output.status.code(),
    );
    assert!(out.exists(), "expected an object at {out:?}");

    let _ = std::fs::remove_dir_all(&dir);
}

/// A generic enum threaded through a function boundary (param + return type)
/// must also compile; this exercises the `variant_expr` helper path (used
/// when the variant local is not in `variant_map`) rather than the inline
/// `check_path` path.
#[test]
fn generic_enum_through_fn_boundary_compiles() {
    let dir = tempdir();
    let src = dir.join("g2.g");
    let mut f = std::fs::File::create(&src).unwrap();
    writeln!(
        f,
        "enum MyOpt<T> {{ S(T), N }}\n\
         fn get(o: MyOpt<u64>) -> u64 {{\n\
         \x20   match o {{ MyOpt::S(v) => v, MyOpt::N => 0 }}\n\
         }}\n\
         fn main() {{\n\
         \x20   let a = get(MyOpt::S(7u64));\n\
         \x20   let _ = a;\n\
         }}"
    )
    .unwrap();
    drop(f);

    let out = dir.join("g2.o");
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
        "a generic enum across a fn boundary must compile; exit={:?}\nstderr:\n{stderr}",
        output.status.code(),
    );
    assert!(out.exists(), "expected an object at {out:?}");

    let _ = std::fs::remove_dir_all(&dir);
}
