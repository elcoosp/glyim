//! Consumer walkthrough: generate a small corpus and validate it
//! end-to-end through the shared front half + interpreter.
//!
//! Run with:
//!
//! ```text
//! cargo run -p glyim-smith --example generate_corpus -- <out-dir> [programs-per-profile]
//! ```
//!
//! The example is the M4.3 consumer slice the plan names in its exit
//! path: generated programs land in `<out-dir>` as complete `//@`-headed
//! fixtures (mode, expect-exit checksum, feature tags, backends) that
//! `glyim-test` can execute directly, each named by its recorded
//! `(profile, seed, size)` identity.

use glyim_smith::{GeneratorProfile, generate};
use std::fs;
use std::path::PathBuf;

fn main() {
    let mut args = std::env::args().skip(1);
    let out_dir = args
        .next()
        .map(PathBuf::from)
        .expect("usage: generate_corpus <out-dir> [programs-per-profile]");
    let per_profile: u64 = args.next().and_then(|n| n.parse().ok()).unwrap_or(3);

    fs::create_dir_all(&out_dir).expect("create output directory");
    let mut written = 0usize;
    for profile in GeneratorProfile::ALL {
        for seed in 0..per_profile {
            let program = generate(*profile, seed, 16).expect("size in-bounds");
            let path = out_dir.join(format!("smith-{}-{seed}-16.g", program.profile.as_str()));
            fs::write(&path, &program.source).expect("write program");
            written += 1;
            println!(
                "{} — expected exit {} (masked {})",
                path.display(),
                program.expected_exit,
                glyim_smith::masked_exit(program.expected_exit)
            );
        }
    }
    println!("{written} programs written to {}", out_dir.display());
    println!(
        "verify one with: glyim-cli --emit=run {}",
        out_dir.join("smith-scalars-0-16.g").display()
    );
}
