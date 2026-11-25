//! Determinism and identity tests (plan M4.3: "seeds recorded"; §3.11:
//! the generator must be reproducible from the recorded identity triple).
//!
//! Constitution C6: same inputs ⇒ byte-identical outputs. The generation
//! triple `(profile, seed, size)` is the recorded identity, so every test
//! here regenerates and compares.

use glyim_smith::{GeneratorProfile, MAX_SIZE, SmithError, generate};

#[test]
fn same_triple_is_byte_identical() {
    for profile in GeneratorProfile::ALL {
        for seed in [0u64, 1, 42, 0xDEAD_BEEF, u64::MAX] {
            for size in [1usize, 7, 33, MAX_SIZE] {
                let a = generate(*profile, seed, size).expect("in-bounds");
                let b = generate(*profile, seed, size).expect("in-bounds");
                assert_eq!(
                    a.source, b.source,
                    "{profile:?} seed={seed} size={size} drifted"
                );
                assert_eq!(a.expected_exit, b.expected_exit);
                assert_eq!(a.seed, seed, "the recorded seed must round-trip");
                assert_eq!(a.profile, *profile);
                assert_eq!(a.size, size);
            }
        }
    }
}

#[test]
fn distinct_seeds_explore_the_space() {
    // Not a proof of injectivity (the source space is finite), but a
    // sanity bound: across 200 seeds every profile must produce more
    // than one distinct program, and most seeds must differ from seed 0.
    for profile in GeneratorProfile::ALL {
        let sources: Vec<String> = (0..200u64)
            .map(|seed| generate(*profile, seed, 12).expect("in-bounds").source)
            .collect();
        let distinct = sources.iter().filter(|s| **s != sources[0]).count();
        assert!(
            distinct > 100,
            "{profile:?}: 200 seeds produced only {distinct} non-canonical programs"
        );
    }
}

#[test]
fn size_out_of_bounds_is_the_only_error() {
    assert_eq!(
        generate(GeneratorProfile::Scalars, 0, 0),
        Err(SmithError {
            detail: "size must be within 1..=96, got 0".to_string(),
        })
    );
    assert!(generate(GeneratorProfile::Scalars, 0, MAX_SIZE + 1).is_err());
    assert!(generate(GeneratorProfile::Generics, 0, 1).is_ok());
}

#[test]
fn fixture_header_is_wellformed() {
    let program = generate(GeneratorProfile::ControlFlow, 9, 4).expect("in-bounds");
    let mut lines = program.source.lines();
    assert_eq!(lines.next(), Some("//@ mode: run-pass"));
    let exit = lines.next().expect("exit line");
    let expected_masked = glyim_smith::masked_exit(program.expected_exit);
    assert_eq!(exit, format!("//@ expect-exit: {expected_masked}"));
    let feature = lines.next().expect("feature line");
    assert!(feature.starts_with("//@ feature: TOOL-TEST-RUN-PASS, TOOL-TEST-PROP"));
    let backends = lines.next().expect("backends line");
    assert_eq!(backends, "//@ backends: gam");
    // The header is followed by the program proper.
    assert!(program.source.contains("fn main() -> i32 {"));
}
