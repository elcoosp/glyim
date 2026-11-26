//! One-off helper for the M5.1 MIR-parity goldens (`mir_parity_tests`):
//! recomputes the normalized MIR+diagnostics hash for the two parity
//! fixtures so a stale golden can be re-captured after an intentional,
//! documented behavior change. Run:
//!
//! ```text
//! cargo run -p glyim-pipeline --example parity_probe
//! ```

use glyim_core::StableHasher;
use glyim_db::{CrateConfig, Database};

fn normalized_hash(
    result: &Result<glyim_pipeline::MirCompilation, Vec<glyim_diag::GlyimDiagnostic>>,
) -> [u8; 16] {
    let mut hasher = StableHasher::new();
    match result {
        Ok(mir) => {
            let mut entries: Vec<_> = mir.bodies.iter().collect();
            entries.sort_by_key(|(def_id, _)| (def_id.krate.to_raw(), def_id.local_id.to_raw()));
            for (def_id, body) in entries {
                hasher.write_str(&format!("BODY:{def_id:?}"));
                hasher.write_str(&format!("{body:#?}"));
            }
            let mut diags = mir.warnings.clone();
            diags.sort_by(|a, b| format!("{a:?}").cmp(&format!("{b:?}")));
            for d in &diags {
                hasher.write_str("DIAG:");
                hasher.write_str(&format!("{d:#?}"));
            }
        }
        Err(diags) => {
            let mut diags = diags.clone();
            diags.sort_by(|a, b| format!("{a:?}").cmp(&format!("{b:?}")));
            for d in &diags {
                hasher.write_str("DIAG:");
                hasher.write_str(&format!("{d:#?}"));
            }
        }
    }
    hasher.finish()
}

fn hash_of(src: &str) -> String {
    let path = std::env::temp_dir().join("glyim_parity_probe.g");
    std::fs::write(&path, src).expect("write fixture");
    let mut db = Database::new(CrateConfig {
        name: "parity".to_string(),
        target_triple: "x86_64-unknown-linux-gnu".to_string(),
        opt_level: 0,
    });
    let result = glyim_pipeline::compile_file_to_mir(&mut db, &path, 0);
    let mut hex = String::new();
    for b in normalized_hash(&result).iter() {
        use std::fmt::Write as _;
        let _ = write!(hex, "{b:02x}");
    }
    hex
}

fn main() {
    println!(
        "simple_main {}",
        hash_of("fn main() -> i32 {\n    let a = 3;\n    a * 2\n}\n")
    );
    println!(
        "struct_impl {}",
        hash_of(
            "struct Point { x: i32, y: i32 }\nimpl Point {\n    fn new(x: i32, y: i32) -> Point {\n        Point { x, y }\n    }\n}\nfn main() -> i32 {\n    let p = Point::new(1, 2);\n    p.x + p.y\n}\n"
        )
    );
}
