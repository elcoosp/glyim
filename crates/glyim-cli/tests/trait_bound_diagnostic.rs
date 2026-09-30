//! Regression test: an unsatisfied trait bound at a generic call site must
//! produce a clean, spanned diagnostic — never a compiler panic.
//!
//! Background: `needs_foo::<Bar>(..)` where `Bar: !Foo` (and
//! `"42".parse::<i32>()` where there is no `impl FromStr for i32`) used to
//! leave an undevirtualized `MirConstKind::VirtualMethod` callee that codegen
//! ICEd on with `MirConstKind::VirtualMethod reached LLVM codegen`.
//!
//! The fix emits a `[T0001] the trait bound ... is not satisfied` diagnostic
//! at monomorphization time instead. This test pins the *behavior*: exit 1,
//! a "not satisfied" message on stderr, and no "panicked"/"ICE" text.

use std::io::Write;
use std::process::Command;

fn tempdir() -> std::path::PathBuf {
    let p = std::env::temp_dir().join(format!(
        "glyim_trait_bound_{}_{}",
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
fn unsatisfied_trait_bound_is_a_diagnostic_not_a_panic() {
    let dir = tempdir();
    let src = dir.join("b.g");
    let mut f = std::fs::File::create(&src).unwrap();
    writeln!(
        f,
        "trait Foo {{ fn foo(&self); }}\n\
         fn needs_foo<T: Foo>(x: T) {{ x.foo(); }}\n\
         struct Bar;\n\
         fn main() {{ needs_foo(Bar); }}"
    )
    .unwrap();
    drop(f);

    let out = dir.join("b.o");
    let output = Command::new(cli_bin())
        .args([src.to_str().unwrap(), "--emit=obj", "-o", out.to_str().unwrap()])
        .output()
        .expect("spawn glyim-cli");

    assert!(
        !output.status.success(),
        "an unsatisfied bound must be a compile error",
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("is not satisfied"),
        "expected a `trait bound ... is not satisfied` diagnostic; got:\n{stderr}",
    );
    assert!(
        !stderr.contains("panicked") && !stderr.contains("ICE"),
        "an unsatisfied bound must NOT panic/ICE; got:\n{stderr}",
    );

    let _ = std::fs::remove_dir_all(&dir);
}
