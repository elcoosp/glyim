//! HIR-30 regression: `let x = loop { break 5; };` must evaluate to 5.
//!
//! The `Break` lowering evaluated the break value and immediately discarded it
//! (`let _ =`), and the `Loop` arm returned a `Unit`/`Never` constant, so `x`
//! was never assigned the break value. The fix writes the break value into a
//! per-loop result local (`LoopInfo::break_place`) that the loop expression
//! reads back at its exit block.

use std::io::Write;
use std::process::Command;

fn tempdir() -> std::path::PathBuf {
    let p = std::env::temp_dir().join(format!(
        "glyim_loop_break_{}_{}",
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

/// The break value must survive lowering: the MIR must contain an assignment
/// of the constant `5` into a local (the loop's result), and the loop must not
/// merely return unit.
#[test]
fn loop_break_value_is_materialized() {
    let dir = tempdir();
    let src = dir.join("b.g");
    let mut f = std::fs::File::create(&src).unwrap();
    writeln!(f, "fn main() {{ let x = loop {{ break 5; }}; let _ = x; }}").unwrap();
    drop(f);

    let out = dir.join("b.mir");
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
        "loop-break program must compile; stderr:\n{}",
        String::from_utf8_lossy(&output.stderr),
    );

    let mir = std::fs::read_to_string(&out).unwrap();
    assert!(
        mir.contains("Int(5)"),
        "the break value 5 must appear in the MIR:\n{mir}"
    );
    // The break value must be assigned to some local (not just discarded).
    let assigned = mir
        .lines()
        .any(|l| l.contains("Assign(") && l.contains("Int(5)"));
    assert!(
        assigned,
        "the break value must be assigned into the loop's result local:\n{mir}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}
