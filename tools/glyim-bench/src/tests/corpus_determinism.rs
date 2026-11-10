//! Determinism/strictness contract for the M2.3 corpus substrate.
//!
//! The corpus manifest inherits the M1.5/M2.2 discipline: two runs with the
//! same inputs must be byte-identical, the manifest is strict
//! (`deny_unknown_fields`), and a committed manifest *fails* once a
//! generator template drifts (the "corpora committed with hashes" DoD).

use glyim_bench::corpus::{
    self, CorpusFamily, CorpusManifest, MAX_SIZE, element_constant, fnv1a64, generate,
};
use glyim_bench::error::BenchError;

#[test]
fn generation_is_deterministic() {
    let first = generate(CorpusFamily::Wide, 7, 12).expect("generates");
    let second = generate(CorpusFamily::Wide, 7, 12).expect("generates");
    assert_eq!(first, second, "same (family, seed, size) must be identical");
    assert!(!first.source.is_empty());
    assert_eq!(first.identity, fnv1a64(&first.source));
}

#[test]
fn families_are_pairwise_distinct() {
    let sources: Vec<String> =
        CorpusFamily::ALL.map(|family| generate(family, 42, 6).expect("generates").source);
    for (i, a) in sources.iter().enumerate() {
        for b in &sources[i + 1..] {
            assert_ne!(a, b, "families {i} and later must emit distinct shapes");
        }
    }
}

#[test]
fn deep_exit_tracks_size() {
    let small = generate(CorpusFamily::Deep, 9, 4).expect("generates");
    let large = generate(CorpusFamily::Deep, 9, 9).expect("generates");
    assert_eq!(small.reference_exit, 70 + 4);
    assert_eq!(large.reference_exit, 70 + 9);
    assert_ne!(small.source, large.source);
}

#[test]
fn wide_exit_is_the_sum_of_element_constants() {
    let size = 16;
    let program = generate(CorpusFamily::Wide, 5, size).expect("generates");
    let expected: i32 = (0..size).map(|i| element_constant(5, i)).sum();
    assert_eq!(program.reference_exit, expected);
    // The generated header must pin the same exit code the manifest records.
    let pinned = format!("//@ expect-exit: {}", program.reference_exit);
    assert!(
        program.source.contains(&pinned),
        "generated header must pin the reference exit: {}",
        program.source
    );
}

#[test]
fn generic_exit_is_the_sum_of_element_constants() {
    let size = 10;
    let program = generate(CorpusFamily::Generic, 3, size).expect("generates");
    let expected: i32 = (0..size).map(|i| element_constant(3, i)).sum();
    assert_eq!(program.reference_exit, expected);
}

#[test]
fn closure_exit_sums_capture_and_argument() {
    let size = 8;
    let program = generate(CorpusFamily::Closure, 11, size).expect("generates");
    // The Closure template sums (capture, argument) pairs; both derive from
    // the same (seed, index) mix, so the total must stay within i32 for the
    // MAX_SIZE bound (documented overflow-safety note on the module).
    assert!(program.reference_exit > 0);
    assert!(program.reference_exit < i32::MAX / 2);
    assert!(program.source.contains("|x: i32| -> i32"));
}

#[test]
fn macro_family_declares_the_stdlib_flag() {
    let program = generate(CorpusFamily::Macro, 3, 5).expect("generates");
    assert_eq!(program.compile_flags, vec!["--with-stdlib".to_string()]);
    assert!(program.source.contains("format!("));
    for family in CorpusFamily::ALL {
        if family == CorpusFamily::Macro {
            continue;
        }
        let program = generate(family, 3, 5).expect("generates");
        assert!(
            program.compile_flags.is_empty(),
            "{family:?} must not require the stdlib"
        );
    }
}

#[test]
fn size_bounds_are_enforced() {
    let zero = generate(CorpusFamily::Wide, 1, 0).expect_err("size 0 rejected");
    let huge = generate(CorpusFamily::Wide, 1, MAX_SIZE + 1).expect_err("size cap");
    for err in [zero, huge] {
        assert!(matches!(err, BenchError::CorpusPlan { .. }), "{err:?}");
        let help = miette::Diagnostic::help(&err).expect("help text");
        assert!(help.to_string().contains("1..="), "{help}");
    }
    generate(CorpusFamily::Wide, 1, MAX_SIZE).expect("boundary size is legal");
}

#[test]
fn family_parse_round_trips() {
    for family in CorpusFamily::ALL {
        assert_eq!(CorpusFamily::parse(family.as_str()), Some(family));
    }
    assert_eq!(CorpusFamily::parse("nope"), None);
    assert_eq!(CorpusFamily::parse("Wide"), None, "parse is case-sensitive");
}

#[test]
fn manifest_build_is_sorted_and_deterministic() {
    let build = || {
        CorpusManifest::build(&[CorpusFamily::Match, CorpusFamily::Wide], &[2, 1], 8)
            .expect("builds")
    };
    let first = build();
    let second = build();
    assert_eq!(first, second);
    assert_eq!(first.programs.len(), 4);
    // Row order: families outer, seeds inner, both in the caller's order.
    assert_eq!(first.programs[0].family, CorpusFamily::Match);
    assert_eq!(first.programs[0].seed, 2);
    assert_eq!(first.programs[1].seed, 1);
    assert_eq!(first.programs[2].family, CorpusFamily::Wide);
}

#[test]
fn manifest_build_rejects_empty_plan() {
    let err = CorpusManifest::build(&[], &[1], 8).expect_err("no families");
    assert!(matches!(err, BenchError::CorpusPlan { .. }));
    let err = CorpusManifest::build(&[CorpusFamily::Wide], &[], 8).expect_err("no seeds");
    assert!(matches!(err, BenchError::CorpusPlan { .. }));
}

#[test]
fn manifest_round_trips_through_json() {
    let manifest = CorpusManifest::build(&CorpusFamily::ALL, &[1, 2], 6).expect("builds");
    let json = manifest.to_json_pretty().expect("serialises");
    let back = CorpusManifest::from_json(&json).expect("deserialises");
    assert_eq!(back, manifest);
}

#[test]
fn manifest_json_is_byte_deterministic() {
    let manifest = CorpusManifest::build(&CorpusFamily::ALL, &[1], 6).expect("builds");
    assert_eq!(
        manifest.to_json_pretty().expect("serialises"),
        manifest.to_json_pretty().expect("serialises")
    );
}

#[test]
fn manifest_json_rejects_unknown_fields() {
    let manifest = CorpusManifest::build(&[CorpusFamily::Wide], &[1], 4).expect("builds");
    let json = manifest.to_json_pretty().expect("serialises");
    let tampered = json.replace("\"programs\"", "\"sneaky_extra\": true,\n  \"programs\"");
    let err = CorpusManifest::from_json(&tampered).expect_err("unknown field rejected");
    assert!(
        matches!(err, BenchError::CorpusManifestParse { .. }),
        "{err:?}"
    );
    let help = miette::Diagnostic::help(&err).expect("help text");
    assert!(help.to_string().contains("glyim.corpus/1"), "{help}");
}

#[test]
fn manifest_verify_catches_template_drift() {
    let mut manifest = CorpusManifest::build(&[CorpusFamily::Wide], &[1], 4).expect("builds");
    manifest.verify().expect("fresh manifest verifies");
    let before = manifest.programs[0].identity;
    manifest.programs[0].identity = before.wrapping_add(1);
    let err = manifest.verify().expect_err("tampered identity");
    assert!(
        matches!(err, BenchError::CorpusIdentityMismatch { .. }),
        "{err:?}"
    );
}

#[test]
fn manifest_render_is_byte_deterministic() {
    let manifest = CorpusManifest::build(&CorpusFamily::ALL, &[1, 2], 6).expect("builds");
    let first = manifest.render_text();
    let second = manifest.render_text();
    assert_eq!(first, second);
    assert!(first.contains("glyim.corpus/1"));
    assert!(first.contains("wide"));
    assert!(first.contains("--with-stdlib"), "macro row shows its flag");
}

#[test]
fn fnv1a64_pins_the_standard_vectors() {
    assert_eq!(fnv1a64(""), 0xcbf2_9ce4_8422_2325);
    assert_eq!(fnv1a64("a"), 0xaf63_dc4c_8601_ec8c);
    assert_eq!(corpus::FNV_OFFSET_BASIS, 0xcbf2_9ce4_8422_2325);
    assert_eq!(corpus::FNV_PRIME, 0x0000_0100_0000_01b3);
}

#[test]
fn element_constant_is_bounded_and_deterministic() {
    for seed in [0u64, 1, 42, u64::MAX] {
        for index in [0usize, 1, 255] {
            let c = element_constant(seed, index);
            assert!((7..=67).contains(&c), "constant out of range: {c}");
            assert_eq!(c, element_constant(seed, index), "deterministic");
        }
    }
}

#[test]
fn generated_headers_carry_the_registry_tags() {
    for family in CorpusFamily::ALL {
        let program = generate(family, 1, 4).expect("generates");
        assert!(
            program
                .source
                .contains("//@ feature: TOOL-TEST-RUN-PASS, TOOL-BENCH-CORPUS-GEN"),
            "{family:?} must tag the registry features it exercises"
        );
        assert!(
            program.source.starts_with("//@ mode: run-pass\n"),
            "{family:?} must be a run-pass fixture with an explicit mode"
        );
    }
}
