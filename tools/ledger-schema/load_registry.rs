//! Registry load + validate timing harness for the Readiness Ledger
//! (M1.1).
//!
//! Dependency-free (`harness = false`): synthesizes a registry of feature
//! and gap files in a temp directory, then measures repeated
//! `load_registry` + `validate` runs so CI logs can track the
//! `ledger-check` job's headroom. Run with:
//!
//! ```text
//! cargo bench -p ledger-schema
//! ```
//!
//! The synthetic root is `$GLYIM_LEDGER_BENCH_ROOT`, defaulting to a
//! fresh temp directory (`FEATURES` feature files plus `GAPS` unwitnessed
//! gaps).

use std::hint::black_box;
use std::path::{Path, PathBuf};
use std::time::Instant;

use ledger_schema::load_registry;

const FEATURES: usize = 200;
const GAPS: usize = 40;

fn feature_toml(id: &str) -> String {
    format!(
        "[[feature]]\n\
         id = \"{id}\"\n\
         title = \"synthetic feature {id}\"\n\
         area = \"lang.synthetic\"\n\
         tier = \"core\"\n\
         spec = \"docs/specs/v0.1.0.md\"\n\
         target_level = \"L5\"\n\
         backends = [\"gam\", \"llvm\"]\n\
         owner_lane = \"M\"\n"
    )
}

fn gap_toml(id: &str, affects: &str) -> String {
    format!(
        "[[gap]]\n\
         id = \"{id}\"\n\
         class = \"a\"\n\
         title = \"synthetic gap {id}\"\n\
         affects = [\"{affects}\"]\n\
         caps = \"L2\"\n\
         opened_in = \"v0.3.0\"\n\
         status = \"unwitnessed\"\n"
    )
}

fn write(root: &Path, rel: &str, contents: &str) {
    let path = root.join(rel);
    std::fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
    std::fs::write(path, contents).expect("write");
}

fn build_registry(root: &Path) {
    let ledger = root.join("ledger");
    for i in 0..FEATURES {
        let id = format!("LANG-SYNTH-{i:04}");
        write(
            &ledger,
            &format!("features/lang/synth-{i:04}.toml"),
            &feature_toml(&id),
        );
    }
    for i in 0..GAPS {
        let id = format!("GAP-9{i:03}");
        let affected = format!("LANG-SYNTH-{:04}", i % FEATURES);
        write(
            &ledger,
            &format!("gaps/async/gap-{i:04}.toml"),
            &gap_toml(&id, &affected),
        );
    }
}

fn main() {
    const RUNS: usize = 5;

    let scratch = std::env::var_os("GLYIM_LEDGER_BENCH_ROOT").map_or_else(
        || {
            let dir = std::env::temp_dir().join("glyim-ledger-bench");
            let _ = std::fs::remove_dir_all(&dir);
            dir
        },
        PathBuf::from,
    );
    build_registry(&scratch);
    let ledger = scratch.join("ledger");

    // Warmup (also caches the files in the page cache).
    let warm = load_registry(black_box(&ledger));
    let entries = warm.registry.feature_count() + warm.registry.gap_count();
    println!(
        "root: {} ({} files, {entries} entries, {} schema diagnostics)",
        ledger.display(),
        warm.registry.file_count(),
        warm.diagnostics.len()
    );

    let mut elapsed_ns: Vec<u128> = Vec::with_capacity(RUNS);
    for i in 0..RUNS {
        let start = Instant::now();
        let loaded = load_registry(black_box(&ledger));
        let diags = loaded.registry.validate();
        black_box(diags.len());
        let d = start.elapsed();
        black_box(loaded);
        elapsed_ns.push(d.as_nanos());
        println!("run {i}: {:>10} ns", d.as_nanos());
    }

    let min = elapsed_ns.iter().min().copied().unwrap_or(0);
    let mean: u128 = elapsed_ns.iter().sum::<u128>() / RUNS.max(1) as u128;
    println!("min:  {min:>10} ns/load+validate");
    println!("mean: {mean:>10} ns/load+validate");
    if warm.registry.file_count() > 0 {
        println!(
            "min per file: {:>8} ns",
            min / warm.registry.file_count() as u128
        );
    }
    if entries > 0 {
        println!("min per entry: {:>7} ns", min / entries as u128);
    }

    // Leave the synthetic tree in place (GLYIM_LEDGER_BENCH_ROOT owners
    // may want to inspect it); the default temp dir is wiped on rerun.
    let _ = scratch;
}
