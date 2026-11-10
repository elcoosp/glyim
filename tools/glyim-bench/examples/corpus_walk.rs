//! Consumer walkthrough for the M2.3 corpus substrate: build a manifest over
//! all seven generator families, print the table, verify the identity
//! contract, dump one regenerated program, and print the `glyim.corpus/1`
//! JSON. Runs anywhere — pure Rust, no compiler invocation, no `perf`.
//!
//! This is the "adopt the corpus as a library" path the M2.3 slice that
//! commits the corpora directory will reuse: build → verify → write, with
//! every failure rendered through the crate's typed, help-annotated errors.
//!
//! ```text
//! cargo run -p glyim-bench --example corpus_walk
//! ```

use glyim_bench::corpus::{CorpusFamily, CorpusManifest, generate};

/// The M2.3 default program size (kept small so the example's output stays
/// readable; the bench uses the same default).
const SIZE: usize = 12;

/// Seeds walked per family.
const SEEDS: [u64; 3] = [1, 2, 3];

fn main() {
    let manifest = match CorpusManifest::build(&CorpusFamily::ALL, &SEEDS, SIZE) {
        Ok(manifest) => manifest,
        Err(err) => {
            eprint!("{:?}", miette::Report::new(err));
            std::process::exit(1);
        }
    };

    println!("{}", manifest.render_text());

    // The "committed with hashes" contract: regenerate every row and check
    // the identity column. A template edit after a manifest was committed
    // surfaces here as CorpusIdentityMismatch with a remedy in its help.
    if let Err(err) = manifest.verify() {
        eprint!("{:?}", miette::Report::new(err));
        std::process::exit(1);
    }
    println!("  verify: {} rows regenerate byte-identically\n", manifest.programs.len());

    // One regenerated program, header included: the source a consumer would
    // write into the corpora directory (or hand to the harness in the
    // follow-up slice).
    match generate(CorpusFamily::Wide, SEEDS[0], SIZE) {
        Ok(program) => {
            println!(
                "-- wide/seed {} program (identity {:016x}, expect-exit {}) --",
                program.seed, program.identity, program.reference_exit
            );
            print!("{}", program.source);
        }
        Err(err) => {
            eprint!("{:?}", miette::Report::new(err));
            std::process::exit(1);
        }
    }

    println!("\n-- glyim.corpus/1 JSON --");
    match manifest.to_json_pretty() {
        Ok(json) => print!("{json}"),
        Err(err) => {
            eprint!("{:?}", miette::Report::new(err));
            std::process::exit(1);
        }
    }
}
