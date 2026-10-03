//! Pin: an out-of-order struct literal is laid out in DECLARATION order, not
//! source order.
//!
//! Audit [LL-6] claimed `Point { y: 7, x: 2 }` wrote 7 into `x`. That is NOT
//! reproducible on the current tree (the aggregate operands are already in
//! declaration order `[x, y]`), so no code change was needed. This test pins
//! the correct behavior against future regression.

use std::io::Write;
use std::process::Command;

fn tempdir() -> std::path::PathBuf {
    let p = std::env::temp_dir().join(format!(
        "glyim_field_order_{}_{}",
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

/// Compile `Point { y: 7, x: 2 }` and inspect the MIR aggregate operand order:
/// it must be `[x_operand, y_operand]` (declaration order), not source order.
#[test]
fn out_of_order_struct_literal_uses_declaration_order() {
    let dir = tempdir();
    let src = dir.join("p.g");
    let mut f = std::fs::File::create(&src).unwrap();
    writeln!(
        f,
        "struct Point {{ x: i32, y: i32 }}\n\
         fn main() {{ let p = Point {{ y: 2, x: 1 }}; let _ = p; }}"
    )
    .unwrap();
    drop(f);

    let out = dir.join("p.mir");
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
        "struct literal must compile; stderr:\n{}",
        String::from_utf8_lossy(&output.stderr),
    );

    let mir = std::fs::read_to_string(&out).unwrap();
    let line = mir
        .lines()
        .find(|l| l.contains("Aggregate(Adt"))
        .expect("expected an Adt aggregate in the MIR");
    // Declaration order is x then y, so the x operand (value 1, span later in
    // source) must come BEFORE the y operand (value 2, span earlier). The
    // operand values are what distinguish them.
    let x_pos = line.find("Int(1)").expect("x operand Int(1) present");
    let y_pos = line.find("Int(2)").expect("y operand Int(2) present");
    assert!(
        x_pos < y_pos,
        "struct fields must be laid out in declaration order (x before y); \
         got aggregate line:\n{line}",
    );

    let _ = std::fs::remove_dir_all(&dir);
}
