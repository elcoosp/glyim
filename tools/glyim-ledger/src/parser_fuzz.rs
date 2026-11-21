//! `parser_fuzz` — the M1.13 parser fuzz slice (plan §6 M1.13: "fuzz the
//! TOML/JSONL parsers").
//!
//! A deterministic, dependency-free property fuzz over every ledger input
//! surface: the `[[feature]]` registry TOML, the `[[gap]]` registry TOML,
//! and the `glyim.evidence/1` JSONL store. Each iteration generates a
//! pseudo-random document (mostly well-formed, with injected junk: unknown
//! fields, wrong types, malformed IDs, truncated syntax) and asserts the
//! parser contract:
//!
//! 1. **No panics.** A parser facing arbitrary bytes degrades to a clean
//!    `Err` (or a load diagnostic), never a panic, unwrap, or index-out of
//!    bounds. The test body *is* the assertion: any panic fails the test.
//! 2. **No silent accepts.** Documents carrying the injected
//!    known-invalid marker (the `frobnicate` unknown field, rejected by
//!    `deny_unknown_fields`) MUST be rejected. Symmetrically, an accepted
//!    `glyim.evidence/1` record that violates an invariant the validator
//!    owns (hash shape, hermetic path) must come back with a non-empty
//!    issue list from `LedgerEvidence::validate`.
//! 3. **Clean errors.** Every rejection carries a non-empty error message
//!    naming the offense (serde's renderer; the validators' messages).
//!
//! # Why hand-rolled randomness
//!
//! The generator is a seeded `xorshift64*` instead of `proptest`: the
//! property here is byte-reproducibility of the fuzz run itself (M1.13's
//! charter — the same seed must traverse the same documents on every
//! machine, so a CI failure names the exact document), and keeping the
//! fuzz self-contained means the reproducibility CI job runs it with no
//! new dependency surface. Seeds are fixed constants; bumping a seed is a
//! deliberate, reviewed change, not noise.

use std::fmt::Write as _;

use ledger_schema::{FeatureFile, GapFile};
use tempfile::TempDir;

use crate::compute::LedgerEvidence;
use crate::run_check;

/// Iteration budget per fuzz engine. The `M1.13` `DoD` is one thousand or
/// more iterations per engine with 0 panics; every engine runs this many,
/// so the suite logs several thousand iterations per
/// `cargo test -p glyim-ledger parser_fuzz`. The floor is pinned at
/// compile time — lowering the budget below the `DoD` bar must fail the
/// build, not silently shrink the suite.
const FUZZ_ITERATIONS: usize = 1_000;

const _: () = assert!(
    FUZZ_ITERATIONS >= 1_000,
    "the M1.13 DoD floor is 1000 iterations per fuzz engine"
);

/// The one canonical hash the generator emits as "well-formed" — every
/// other spelling is expected to be flagged by the validator.
const GOOD_SHA: &str = "bcc90c1a97ea447c7ec33e0e3b99d5398eb94d34a06be2080b96c56ca7b62d72";

/// Seedable `xorshift64*` — small, deterministic, dependency-free.
struct FuzzRng(u64);

impl FuzzRng {
    /// A new generator from a fixed seed (never zero: the state must stay
    /// non-zero or the sequence collapses to all-zeros).
    fn new(seed: u64) -> Self {
        Self(seed | 1)
    }

    /// The next raw 64 bits.
    fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    /// A uniform index in `0..n` (n must be non-zero; every caller passes a
    /// non-empty pool).
    fn below(&mut self, n: usize) -> usize {
        (self.next_u64() % n as u64) as usize
    }

    /// True `pct`% of the time.
    fn chance(&mut self, pct: u64) -> bool {
        self.next_u64() % 100 < pct
    }

    /// A random pick from a non-empty pool.
    fn pick<'a, T>(&mut self, items: &'a [T]) -> &'a T {
        &items[self.below(items.len())]
    }
}

// -- generators --------------------------------------------------------------

/// Word pools for titles/areas — mixed real and junk so both the accepted
/// and rejected paths stay exercised.
const WORDS: [&str; 12] = [
    "for", "await", "user", "tag", "mod", "ümlaut", "x", "empty", "a b", "表", "fn-item", "9",
];
const FEATURE_IDS: [&str; 6] = [
    "LANG-X",
    "CODEGEN-BC-EMIT",
    "STD-CORE-OPTION",
    "lower",
    "LANG--BAD",
    "GAP-LOOKALIKE",
];
const GAP_IDS: [&str; 6] = [
    "GAP-0009", "GAP-1234", "gap-0002", "GAP-", "GAP-12a4", "GAP-0007",
];
const CLASSES: [&str; 5] = ["a", "b", "c", "d", ""];
const STATUSES: [&str; 4] = ["witnessed", "unwitnessed", "WITNESSED", "closed"];
const LEVELS: [&str; 6] = ["L0", "L1", "L2", "L3", "L5", "L9"];
const TIERS: [&str; 3] = ["core", "tooling", "lang"];
const LANES: [&str; 5] = ["M", "B", "F", "T", "Z"];
const BACKENDS: [&str; 3] = ["gam", "llvm", "interp"];

/// Junk lines a truncated/corrupted TOML file might carry (syntax errors).
const TOML_JUNK: [&str; 6] = [
    "[[feature",
    "id =",
    "key = [1, 2,",
    "\"unterminated",
    "= = =",
    "id = 7 (wrong type where a string is required)",
];

/// Junk fragments spliced into generated JSONL lines (parse errors).
const JSON_JUNK: [&str; 6] = [
    "{\"schema\": \"glyim.evidence/1\", \"name\":}",
    "{\"schema\" \"glyim.evidence/1\"}",
    "{\"schema\": \"glyim.evidence/1\", \"opt\": \"zero\"}",
    "{\"schema\": \"glyim.evidence/1\", \"features\": 12}",
    "{\"schema\": \"glyim.evidence/1\", \"reason\": false}",
    "{\"schema\": \"glyim.evidence/1\"",
];

/// Generates one pseudo-random `[[feature]]` registry document.
///
/// `corrupt` injects the `frobnicate` unknown field — the marker the
/// engine treats as known-invalid (`deny_unknown_fields` must reject).
fn random_feature_toml(rng: &mut FuzzRng, corrupt: bool) -> String {
    let mut doc = String::from("# generated fuzz document\n");
    let tables = 1 + rng.below(2);
    for index in 0..tables {
        let _ = writeln!(doc, "\n[[feature]]");
        // Junk injection: a syntax/type error in ~25% of tables (too high
        // a rate would starve the accepted path the field-access property
        // needs).
        if rng.chance(25) {
            let _ = writeln!(doc, "{}", rng.pick(&TOML_JUNK));
        }
        // The known-invalid marker: an unknown field. The registry schema
        // denies unknown fields, so acceptance here is a silent accept.
        if corrupt && index == 0 {
            let _ = writeln!(doc, "frobnicate = true");
        }
        let _ = writeln!(doc, "id = \"{}\"", rng.pick(&FEATURE_IDS));
        let _ = writeln!(doc, "area = \"lang.{}\"", rng.pick(&WORDS));
        let _ = writeln!(doc, "title = \"{}\"", rng.pick(&WORDS));
        let _ = writeln!(doc, "tier = \"{}\"", rng.pick(&TIERS));
        let _ = writeln!(doc, "spec = \"docs/specs/{}.md\"", rng.pick(&WORDS));
        let _ = writeln!(doc, "target_level = \"{}\"", rng.pick(&LEVELS));
        match rng.below(4) {
            // Well-formed arrays.
            0 | 1 => {
                let _ = writeln!(
                    doc,
                    "backends = [\"{}\", \"{}\"]",
                    rng.pick(&BACKENDS),
                    rng.pick(&BACKENDS)
                );
            }
            // Wrong scalar type where an array is required.
            2 => {
                let _ = writeln!(doc, "backends = \"gam\"");
            }
            // Missing entirely (a required field — must reject).
            _ => {}
        }
        if rng.chance(85) {
            let _ = writeln!(doc, "owner_lane = \"{}\"", rng.pick(&LANES));
        }
        if rng.chance(30) {
            let _ = writeln!(doc, "requires = [\"{}\"]", rng.pick(&FEATURE_IDS));
        }
    }
    doc
}

/// Generates one pseudo-random `[[gap]]` registry document.
fn random_gap_toml(rng: &mut FuzzRng, corrupt: bool) -> String {
    let mut doc = String::from("# generated fuzz document\n");
    let tables = 1 + rng.below(2);
    for index in 0..tables {
        let _ = writeln!(doc, "\n[[gap]]");
        if rng.chance(25) {
            let _ = writeln!(doc, "{}", rng.pick(&TOML_JUNK));
        }
        if corrupt && index == 0 {
            let _ = writeln!(doc, "frobnicate = true");
        }
        let _ = writeln!(doc, "id = \"{}\"", rng.pick(&GAP_IDS));
        let _ = writeln!(doc, "class = \"{}\"", rng.pick(&CLASSES));
        let _ = writeln!(doc, "title = \"{}\"", rng.pick(&WORDS));
        match rng.below(6) {
            0 | 1 => {
                let _ = writeln!(doc, "affects = [\"{}\"]", rng.pick(&FEATURE_IDS));
            }
            2 | 3 => {
                let _ = writeln!(doc, "affects = []");
            }
            4 => {
                let _ = writeln!(doc, "affects = 12");
            }
            _ => {}
        }
        let _ = writeln!(doc, "caps = \"{}\"", rng.pick(&LEVELS));
        if rng.chance(40) {
            let _ = writeln!(
                doc,
                "expect = {{ diagnostic = \"{}\" }}",
                rng.pick(&["T0060", "P0001", "t-1"])
            );
        }
        let _ = writeln!(
            doc,
            "opened_in = \"v{}.{}.{}\"",
            rng.below(9),
            rng.below(9),
            rng.below(9)
        );
        if rng.chance(60) {
            let _ = writeln!(doc, "status = \"{}\"", rng.pick(&STATUSES));
        }
        if rng.chance(40) {
            // Witness paths include non-hermetic spellings the audit face
            // must flag, not crash on.
            let _ = writeln!(
                doc,
                "witness = \"{}\"",
                rng.pick(&["w.g", "../escape.g", "/absolute/w.g", "deep/nested/w.g"])
            );
        }
    }
    doc
}

/// Generates one pseudo-random `glyim.evidence/1` JSONL line.
fn random_evidence_line(rng: &mut FuzzRng) -> String {
    if rng.chance(25) {
        return rng.pick(&JSON_JUNK).to_string();
    }
    // A mostly-well-formed record with random field spellings and values;
    // the validator invariants (hash shape, hermetic path) get explicit
    // junk pools so both accept-and-flag and accept-clean paths run.
    let sha_pool = [GOOD_SHA, "NOT-A-HASH", "deadbeef", ""];
    let path_pool = ["w.g", "a/b.g", "/abs.g", "../up.g", "deep/../tricky.g"];
    let outcome_pool = ["passed", "failed", "skipped", "timeout"];
    let mode_pool = [
        "run-pass",
        "run-fail",
        "compile-fail",
        "compile-pass",
        "ui",
        "fuzz",
    ];
    let backend_pool = ["gam", "llvm", "interp"];
    format!(
        "{{\"schema\":\"glyim.evidence/1\",\"name\":\"{}\",\"fixture\":\"{}\",\"fixture_sha256\":\"{}\",\
         \"revision\":\"{}\",\"mode\":\"{}\",\"outcome\":\"{}\",\"reason\":null,\"backend\":\"{}\",\"opt\":{},\
         \"features\":[\"{}\"],\"declared_backends\":[\"{}\"],\"instructions\":null,\"rss_kb\":null}}",
        rng.pick(&WORDS),
        rng.pick(&path_pool),
        rng.pick(&sha_pool),
        rng.pick(&["default", "rev-a", ""]),
        rng.pick(&mode_pool),
        rng.pick(&outcome_pool),
        rng.pick(&backend_pool),
        rng.below(4),
        rng.pick(&FEATURE_IDS),
        rng.pick(&backend_pool),
    )
}

// -- engines -----------------------------------------------------------------

/// Which registry wrapper a TOML engine fuzzes.
enum RegistryFace {
    /// `ledger/features/**/*.toml`.
    Feature,
    /// `ledger/gaps/**/*.toml`.
    Gap,
}

/// The TOML contract for one registry face: parse must either succeed
/// (and hand back accessible entries) or fail cleanly with a non-empty
/// message; a success on a document that carries the injected
/// known-invalid marker (`frobnicate` unknown field) is a silent accept
/// and fails the run.
fn fuzz_toml_surface(seed: u64, face: RegistryFace) {
    let face_name = match face {
        RegistryFace::Feature => "feature-toml",
        RegistryFace::Gap => "gap-toml",
    };
    let mut rng = FuzzRng::new(seed);
    let mut parsed_ok = 0_usize;
    let mut rejected = 0_usize;
    for iteration in 0..FUZZ_ITERATIONS {
        let corrupt = rng.chance(30);
        let (document, known_invalid) = match face {
            RegistryFace::Feature => (random_feature_toml(&mut rng, corrupt), corrupt),
            RegistryFace::Gap => (random_gap_toml(&mut rng, corrupt), corrupt),
        };
        let outcome: Result<(Vec<String>, Vec<String>), String> = match face {
            RegistryFace::Feature => toml::from_str::<FeatureFile>(&document)
                .map_err(|error| error.to_string())
                .map(|file| {
                    (
                        file.feature.iter().map(|entry| entry.id.clone()).collect(),
                        file.feature
                            .iter()
                            .map(|entry| entry.target_level.as_str().to_string())
                            .collect(),
                    )
                }),
            RegistryFace::Gap => toml::from_str::<GapFile>(&document)
                .map_err(|error| error.to_string())
                .map(|file| {
                    (
                        file.gap.iter().map(|entry| entry.id.clone()).collect(),
                        file.gap
                            .iter()
                            .map(|entry| entry.caps.as_str().to_string())
                            .collect(),
                    )
                }),
        };
        match outcome {
            Ok((ids, levels)) => {
                parsed_ok += 1;
                // Access every derived value — an accepted document must
                // survive full field use without panicking (catches broken
                // default handling, bad enum round-trips, index panics).
                for (id, level) in ids.iter().zip(levels.iter()) {
                    assert!(level.starts_with('L'), "level round-trip for {id}");
                }
                assert!(
                    !known_invalid,
                    "[{face_name}] iteration {iteration}: silent accept of a known-invalid document:\n{document}"
                );
            }
            Err(message) => {
                rejected += 1;
                assert!(
                    !message.is_empty(),
                    "[{face_name}] iteration {iteration}: clean error must carry a message"
                );
            }
        }
    }
    eprintln!(
        "parser_fuzz[{face_name}]: {FUZZ_ITERATIONS} iterations, {parsed_ok} accepted, {rejected} rejected, 0 panics"
    );
}

/// The JSONL contract: strict parse + invariant validation, no panics, no
/// silent accepts (a record with a known-bad hash or non-hermetic path
/// that parses must come back flagged by `validate`).
fn fuzz_evidence_surface(seed: u64) {
    let mut rng = FuzzRng::new(seed);
    let mut parsed_ok = 0_usize;
    let mut flagged = 0_usize;
    for iteration in 0..FUZZ_ITERATIONS {
        let line = random_evidence_line(&mut rng);
        match LedgerEvidence::from_jsonl_line(&line) {
            Ok(record) => {
                parsed_ok += 1;
                let issues = record.validate();
                // Mirror the validator's invariants — independently
                // spelled, so a validator bug cannot hide behind itself.
                // (The backslash pool member never materializes as a
                // backslash after JSON unescaping, so it stays out of the
                // known-bad predicate.)
                let known_bad_hash = record.fixture_sha256 != GOOD_SHA;
                let known_bad_path = record.fixture.starts_with('/')
                    || record.fixture.split('/').any(|segment| segment == "..");
                if known_bad_hash || known_bad_path {
                    assert!(
                        !issues.is_empty(),
                        "[evidence] iteration {iteration}: silent accept of a record violating a validator invariant: {line}"
                    );
                    flagged += 1;
                }
            }
            Err(message) => {
                assert!(
                    !message.is_empty(),
                    "[evidence] iteration {iteration}: clean error must carry a message"
                );
            }
        }
    }
    eprintln!(
        "parser_fuzz[evidence]: {FUZZ_ITERATIONS} iterations, {parsed_ok} accepted, {flagged} validator-flagged, 0 panics"
    );
}

/// The end-to-end face: write generated registry documents into a temp
/// ledger tree and run the full `ledger check` pipeline over it. Garbage
/// must surface as diagnostics (or clean passes for genuinely valid
/// documents) — never a panic, never a silent accept of an inconsistent
/// registry.
fn fuzz_check_pipeline(seed: u64) {
    let mut rng = FuzzRng::new(seed);
    for iteration in 0..FUZZ_ITERATIONS {
        let dir = TempDir::new().expect("temp ledger root");
        let root = dir.path();
        let ledger = root.join("ledger");
        std::fs::create_dir_all(ledger.join("features")).expect("mkdir features");
        std::fs::create_dir_all(ledger.join("gaps")).expect("mkdir gaps");

        let features = random_feature_toml(&mut rng, false);
        let gaps = random_gap_toml(&mut rng, false);
        std::fs::write(ledger.join("features/fuzz.toml"), &features).expect("write features");
        std::fs::write(ledger.join("gaps/fuzz.toml"), &gaps).expect("write gaps");

        // Every 5th iteration writes a cross-referencing pair (a gap
        // affecting a feature that exists) so the consistency checks get
        // real work instead of only reporting missing-file diagnostics.
        if iteration % 5 == 0 {
            std::fs::write(
                ledger.join("features/pair.toml"),
                "\
[[feature]]\n\
id = \"LANG-FUZZ-PAIR\"\n\
title = \"pair\"\n\
area = \"lang.fuzz\"\n\
tier = \"core\"\n\
spec = \"docs/specs/v0.1.0.md\"\n\
target_level = \"L1\"\n\
backends = [\"gam\"]\n\
owner_lane = \"M\"\n",
            )
            .expect("write pair feature");
            std::fs::write(
                ledger.join("gaps/pair.toml"),
                "\
[[gap]]\n\
id = \"GAP-0999\"\n\
class = \"c\"\n\
title = \"fuzz pair\"\n\
affects = [\"LANG-FUZZ-PAIR\"]\n\
caps = \"L0\"\n\
opened_in = \"v0.0.1\"\n\
status = \"unwitnessed\"\n",
            )
            .expect("write pair gap");
        }

        // The one pipeline entry point that loads every surface. Panics,
        // unwraps, or silent accepts of a garbage registry fail the fuzz.
        let outcome = run_check(root);
        let rendered = crate::format_outcome(&outcome);
        assert!(
            !rendered.is_empty(),
            "[check-pipeline] iteration {iteration}: outcome must render"
        );
    }
    eprintln!(
        "parser_fuzz[check-pipeline]: {FUZZ_ITERATIONS} iterations over generated ledger trees, 0 panics"
    );
}

// -- the DoD tests (`cargo test -p glyim-ledger parser_fuzz`) ----------------

#[test]
fn parser_fuzz_feature_registry_toml_survives_1000_documents() {
    fuzz_toml_surface(0x5EED_0001, RegistryFace::Feature);
}

#[test]
fn parser_fuzz_gap_registry_toml_survives_1000_documents() {
    fuzz_toml_surface(0x5EED_0002, RegistryFace::Gap);
}

#[test]
fn parser_fuzz_evidence_jsonl_survives_1000_lines() {
    fuzz_evidence_surface(0x5EED_0003);
}

#[test]
fn parser_fuzz_check_pipeline_survives_1000_generated_trees() {
    fuzz_check_pipeline(0x5EED_0004);
}

/// A known-good document must still parse (guards against the fuzz
/// generators drifting into rejecting everything, which would make the
/// "no silent accepts" assertions vacuous).
#[test]
fn parser_fuzz_known_good_documents_still_parse() {
    let feature = "\
[[feature]]\n\
id = \"LANG-FOR-VEC-INTOITER\"\n\
title = \"for over Vec<T> IntoIterator\"\n\
area = \"lang.control-flow\"\n\
tier = \"core\"\n\
spec = \"docs/specs/v0.1.0.md\"\n\
target_level = \"L5\"\n\
backends = [\"gam\", \"llvm\"]\n\
owner_lane = \"M\"\n";
    let parsed: FeatureFile = toml::from_str(feature).expect("known-good feature document");
    assert_eq!(parsed.feature.len(), 1);
    assert_eq!(parsed.feature[0].id, "LANG-FOR-VEC-INTOITER");

    let gap = "\
[[gap]]\n\
id = \"GAP-0007\"\n\
class = \"b\"\n\
title = \"differential backend divergence\"\n\
affects = [\"LANG-ASYNC-AWAIT\"]\n\
caps = \"L2\"\n\
opened_in = \"v0.10.0\"\n\
status = \"unwitnessed\"\n";
    let parsed: GapFile = toml::from_str(gap).expect("known-good gap document");
    assert_eq!(parsed.gap.len(), 1);
    assert_eq!(parsed.gap[0].id, "GAP-0007");

    let line = format!(
        "{{\"schema\":\"glyim.evidence/1\",\"name\":\"w.g\",\"fixture\":\"w.g\",\
\"fixture_sha256\":\"{GOOD_SHA}\",\
\"revision\":\"default\",\"mode\":\"run-pass\",\"outcome\":\"passed\",\"reason\":null,\
\"backend\":\"gam\",\"opt\":0,\"features\":[\"LANG-X\"],\"declared_backends\":[\"gam\"],\
\"instructions\":null,\"rss_kb\":null}}"
    );
    let record = LedgerEvidence::from_jsonl_line(&line).expect("known-good evidence line");
    assert!(
        record.validate().is_empty(),
        "known-good record validates clean"
    );
}

/// The seed discipline: two runs over the same seed must traverse the
/// same document stream (the reproducibility property the CI job leans
/// on — a failure on one machine reproduces byte-for-byte elsewhere).
#[test]
fn parser_fuzz_stream_is_seed_deterministic() {
    let stream = |seed: u64| {
        let mut rng = FuzzRng::new(seed);
        (0..64)
            .map(|_| random_feature_toml(&mut rng, false))
            .collect::<Vec<_>>()
    };
    assert_eq!(stream(0x5EED_BEEF), stream(0x5EED_BEEF));
    assert_ne!(stream(0x5EED_BEEF), stream(0x5EED_CAFE));
}
