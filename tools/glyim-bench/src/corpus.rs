//! Deterministic synthetic program corpus (plan M2.3, slice 1).
//!
//! Master plan M2.3 ("Corpora") calls for seven synthetic generator families
//! — *wide, deep, generic, trait, match, macro, closure* — plus real
//! programs and ≥12 runtime benchmark programs, "corpora committed with
//! hashes". This module is the generator substrate for that work item: given
//! a `(family, seed, size)` triple it emits a Glyim program whose **source
//! text, reference exit code, and compile flags are a pure function of the
//! triple** — no clock, no ambient PRNG, no filesystem. The same triple
//! regenerates the same bytes on any machine, which is what lets a manifest
//! record only an identity hash instead of the sources.
//!
//! # The manifest
//!
//! [`CorpusManifest`] is the committed artifact (`glyim.corpus/1` JSON,
//! `deny_unknown_fields`, byte-deterministic rendering — the same discipline
//! as the M1.5 evidence records and the M2.2 `glyim.bench/1` reports). It
//! stores per program: family, seed, size, reference exit code, compile
//! flags, and the [`fnv1a64`] identity of the source text.
//! [`CorpusManifest::verify`] regenerates every entry and fails with a typed
//! [`BenchError::CorpusIdentityMismatch`] when a template drifted after the
//! manifest was committed — the machine-checked "committed with hashes"
//! guarantee.
//!
//! # Families and shapes
//!
//! Every template mirrors a shape that already exists in the run-pass corpus
//! (the fixture is named in the template's comment), so the generated
//! programs reuse proven compiler paths; the M2.3 slice that lands the
//! committed corpora directory will run them through the harness and pin
//! them as fixtures. The exit code is derived *by construction* (a
//! documented sum of the generated constants), never by running a compiler:
//!
//! | Family | Shape (proven by) | Reference exit |
//! |---|---|---|
//! | [`CorpusFamily::Wide`] | straight-line block (`behavior/vec_capacity_ge_len.g` style) | Σ element constants |
//! | [`CorpusFamily::Deep`] | nested `if` blocks (`no-ice/deep_if.g` shape) | `70 + size` |
//! | [`CorpusFamily::Generic`] | `fn id<T>(x: T) -> T { x }` fan-out (`run-pass/generic_identity.g`) | Σ element constants |
//! | [`CorpusFamily::Trait`] | `&mut self` trait + impls (`behavior/trait_mut_self.g`) | Σ element constants |
//! | [`CorpusFamily::Match`] | fieldless-enum `match` dispatch (`run-pass/enum_match.g`) | Σ variant values |
//! | [`CorpusFamily::Macro`] | `format!`/`println!` checks (`behavior/format_multiarg.g`; `--with-stdlib`) | `0` (checks `return` 1..=size on mismatch) |
//! | [`CorpusFamily::Closure`] | single-capture closures (`behavior/closure_capture.g`) | Σ (capture + argument) |
//!
//! # Overflow safety
//!
//! Element constants are bounded to `[7, 67]` and closure arguments to
//! `[1, 23]`; with [`MAX_SIZE`] = 256 the worst-case accumulator is
//! 256 × (67 + 23) = 23 040, far inside `i32`. All template arithmetic is
//! `i32` by construction, so the generated programs cannot overflow under
//! the workspace's `overflow-checks = true` profile, and the interpreter and
//! LLVM agree on the reference exit.
//!
//! # Identity hash
//!
//! [`fnv1a64`] (FNV-1a, 64-bit) is the slice's identity hash — the same
//! construction the workspace already uses elsewhere (see the
//! `unreadable_literal` lint rationale). The M2.5 perf-history store
//! upgrades corpus identity to SHA-256 content addressing alongside the
//! evidence records; the manifest's `identity` column is the only field that
//! changes.

use crate::error::BenchError;

/// Schema tag pinned for the manifest shape.
pub const CORPUS_SCHEMA: &str = "glyim.corpus/1";

/// Upper bound for a program's `size` parameter. Chosen so the worst-case
/// reference exit (see the module's overflow-safety note) stays inside
/// `i32` with a wide margin.
pub const MAX_SIZE: usize = 256;

/// FNV-1a 64-bit offset basis.
pub const FNV_OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;

/// FNV-1a 64-bit prime.
pub const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// The seven M2.3 synthetic generator families.
#[derive(
    Clone,
    Copy,
    Debug,
    PartialEq,
    Eq,
    Hash,
    serde::Serialize,
    serde::Deserialize,
)]
#[serde(rename_all = "kebab-case")]
pub enum CorpusFamily {
    /// Wide straight-line blocks (many independent statements).
    Wide,
    /// Deeply nested control flow.
    Deep,
    /// Generic-function fan-out (monomorphization pressure).
    Generic,
    /// Trait + impl dispatch (`&mut self` methods).
    Trait,
    /// Enum `match` dispatch tables.
    Match,
    /// Builtin-macro (`format!`/`println!`) expansion chains.
    Macro,
    /// Closure capture + call chains.
    Closure,
}

impl CorpusFamily {
    /// Every family, in the canonical order used by [`CorpusManifest::build`]
    /// and the benches/examples.
    pub const ALL: [CorpusFamily; 7] = [
        CorpusFamily::Wide,
        CorpusFamily::Deep,
        CorpusFamily::Generic,
        CorpusFamily::Trait,
        CorpusFamily::Match,
        CorpusFamily::Macro,
        CorpusFamily::Closure,
    ];

    /// The family's kebab-case spelling (the manifest's serialised form).
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            CorpusFamily::Wide => "wide",
            CorpusFamily::Deep => "deep",
            CorpusFamily::Generic => "generic",
            CorpusFamily::Trait => "trait",
            CorpusFamily::Match => "match",
            CorpusFamily::Macro => "macro",
            CorpusFamily::Closure => "closure",
        }
    }

    /// The inverse of [`CorpusFamily::as_str`] (case-sensitive; `None` for
    /// anything the manifest schema would also reject).
    #[must_use]
    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "wide" => Some(Self::Wide),
            "deep" => Some(Self::Deep),
            "generic" => Some(Self::Generic),
            "trait" => Some(Self::Trait),
            "match" => Some(Self::Match),
            "macro" => Some(Self::Macro),
            "closure" => Some(Self::Closure),
            _ => None,
        }
    }
}

/// One generated program: source text plus the metadata a consumer needs to
/// compile and check it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CorpusProgram {
    /// The family that generated this program.
    pub family: CorpusFamily,
    /// The seed the program regenerates from.
    pub seed: u64,
    /// The size parameter (family-dependent element count).
    pub size: usize,
    /// The program's Glyim source, header included.
    pub source: String,
    /// The exit code the program must produce (derived by construction).
    pub reference_exit: i32,
    /// Compile flags the program requires (e.g. `--with-stdlib`).
    pub compile_flags: Vec<String>,
    /// [`fnv1a64`] of [`CorpusProgram::source`] (the manifest's identity).
    pub identity: u64,
}

/// One manifest row: everything needed to regenerate and re-verify a
/// program without storing its source.
#[derive(
    Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize,
)]
#[serde(deny_unknown_fields)]
pub struct CorpusEntry {
    /// The family that generated the program.
    pub family: CorpusFamily,
    /// The seed the program regenerates from.
    pub seed: u64,
    /// The size parameter passed to the generator.
    pub size: usize,
    /// The exit code the program must produce.
    pub reference_exit: i32,
    /// Compile flags the program requires.
    pub compile_flags: Vec<String>,
    /// [`fnv1a64`] of the program's source text.
    pub identity: u64,
}

/// The committed corpus artifact: identity, rows, and the verify contract.
///
/// Rows are stored in build order (families outer, seeds inner — both in the
/// caller's order), so two builds from the same inputs serialise to
/// identical bytes.
#[derive(
    Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize,
)]
#[serde(deny_unknown_fields)]
pub struct CorpusManifest {
    /// Schema tag, always [`CORPUS_SCHEMA`].
    pub schema: String,
    /// One row per program, in build order.
    pub programs: Vec<CorpusEntry>,
}

/// The splitmix64 finalizer: a stateless, clock-free mix of one `u64`.
fn mix64(z: u64) -> u64 {
    let mut z = z.wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// Deterministic per-element constant in `[7, 67]`, derived from
/// `(seed, index)`.
///
/// This is the shared building block of every template's data plane, and the
/// exit-code contracts are sums over it — tests recompute the sums from this
/// function rather than hard-coding literals.
#[must_use]
pub fn element_constant(seed: u64, index: usize) -> i32 {
    let mixed = mix64(seed ^ mix64(index as u64));
    ((mixed % 61) as i32) + 7
}

/// Deterministic closure argument in `[1, 23]`, derived from
/// `(seed, index)` (an independent stream from [`element_constant`]).
fn argument_constant(seed: u64, index: usize) -> i32 {
    let mixed = mix64(seed ^ mix64((index as u64) + 0xFFFD));
    ((mixed % 23) as i32) + 1
}

/// FNV-1a (64-bit) over the UTF-8 bytes of `data`.
///
/// The slice's identity hash: not cryptographic, but the corpus is
/// generator-owned text (not adversary-controlled), and the M2.5 perf-history
/// store's SHA-256 migration replaces this column wholesale.
#[must_use]
pub fn fnv1a64(data: &str) -> u64 {
    let mut hash = FNV_OFFSET_BASIS;
    for byte in data.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    hash
}

/// Generate one program: a pure function of `(family, seed, size)`.
///
/// # Errors
///
/// [`BenchError::CorpusPlan`] when `size` is outside `1..=[MAX_SIZE]`.
pub fn generate(
    family: CorpusFamily,
    seed: u64,
    size: usize,
) -> Result<CorpusProgram, BenchError> {
    if size == 0 || size > MAX_SIZE {
        return Err(BenchError::CorpusPlan {
            detail: format!("size must be within 1..={MAX_SIZE}, got {size}"),
        });
    }
    let _span = tracing::info_span!(
        "corpus_generate",
        family = family.as_str(),
        seed = seed,
        size = size,
    )
    .entered();

    let template: fn(u64, usize) -> (String, i32) = match family {
        CorpusFamily::Wide => gen_wide,
        CorpusFamily::Deep => gen_deep,
        CorpusFamily::Generic => gen_generic,
        CorpusFamily::Trait => gen_trait,
        CorpusFamily::Match => gen_match,
        CorpusFamily::Macro => gen_macro,
        CorpusFamily::Closure => gen_closure,
    };
    let (source, reference_exit) = template(seed, size);
    let compile_flags = match family {
        CorpusFamily::Macro => vec!["--with-stdlib".to_string()],
        _ => Vec::new(),
    };
    let identity = fnv1a64(&source);
    tracing::debug!(identity = format_args!("{identity:016x}"), "corpus program generated");
    Ok(CorpusProgram {
        family,
        seed,
        size,
        source,
        reference_exit,
        compile_flags,
        identity,
    })
}

/// The `//@` fixture header shared by every generated program (M1.4 grammar):
/// explicit mode, the by-construction exit code, the registry features the
/// corpus exercises, and the verified backend set.
fn header(reference_exit: i32, extra_flags: &[&str]) -> String {
    let mut text = format!(
        "//@ mode: run-pass\n//@ expect-exit: {reference_exit}\n"
    );
    for flag in extra_flags {
        text.push_str(&format!("//@ compile-flags: {flag}\n"));
    }
    text.push_str("//@ feature: TOOL-TEST-RUN-PASS, TOOL-BENCH-CORPUS-GEN\n");
    text.push_str("//@ backends: gam, llvm\n");
    text
}

/// Wide: one straight-line block of `size` dependent additions.
///
/// Exit = Σ element constants (all adds, no overflow within [`MAX_SIZE`]).
fn gen_wide(seed: u64, size: usize) -> (String, i32) {
    let mut body = String::new();
    let mut exit: i32 = 0;
    let mut prev = String::from("a0");
    for index in 0..size {
        let constant = element_constant(seed, index);
        exit = exit.wrapping_add(constant);
        if index == 0 {
            body.push_str(&format!("    let a0 = {constant};\n"));
        } else {
            body.push_str(&format!("    let a{index} = {prev} + {constant};\n"));
            prev = format!("a{index}");
        }
    }
    let source = format!("{}fn main() -> i32 {{\n{body}    {prev}\n}}\n", header(exit, &[]));
    (source, exit)
}

/// Deep: `size`-level nested `if` blocks assigning at the innermost level.
///
/// Exit = `70 + size` (the innermost assignment's constant), so the exit
/// doubles as a depth witness for the M2.4 scaling analyzer.
fn gen_deep(_seed: u64, size: usize) -> (String, i32) {
    let exit = 70 + size as i32;
    let mut body = String::from("    let mut acc = 0;\n");
    for _ in 0..size {
        body.push_str("    if acc == 0 {\n");
    }
    body.push_str(&format!("        acc = {exit};\n"));
    for _ in 0..size {
        body.push_str("    }\n");
    }
    let source = format!("{}fn main() -> i32 {{\n{body}    acc\n}}\n", header(exit, &[]));
    (source, exit)
}

/// Generic: a single `id<T>` definition fanned out over `size` call sites.
///
/// Mirrors `run-pass/generic_identity.g`; exit = Σ element constants (each
/// `id` call returns its argument unchanged).
fn gen_generic(seed: u64, size: usize) -> (String, i32) {
    let mut body = String::new();
    let mut exit: i32 = 0;
    for index in 0..size {
        let constant = element_constant(seed, index);
        exit = exit.wrapping_add(constant);
        if index == 0 {
            body.push_str(&format!("    let v0 = id({constant});\n"));
        } else {
            body.push_str(&format!("    let v{index} = id(v{}) + {constant};\n", index - 1));
        }
    }
    let source = format!(
        "{}fn id<T>(x: T) -> T {{ x }}\n\nfn main() -> i32 {{\n{body}    v{}\n}}\n",
        header(exit, &[]),
        size - 1,
    );
    (source, exit)
}

/// Trait: one `&mut self` trait, `size` impls, straight-line dispatch.
///
/// Mirrors `behavior/trait_mut_self.g`; exit = Σ element constants.
fn gen_trait(seed: u64, size: usize) -> (String, i32) {
    let mut items = String::from("trait Read {\n    fn get(&mut self) -> i32;\n}\n\n");
    let mut body = String::new();
    let mut exit: i32 = 0;
    for index in 0..size {
        let constant = element_constant(seed, index);
        exit = exit.wrapping_add(constant);
        items.push_str(&format!(
            "struct W{index} {{ r: i32 }}\n\nimpl Read for W{index} {{\n    fn get(&mut self) -> i32 {{ self.r }}\n}}\n\n"
        ));
        if index == 0 {
            body.push_str(&format!("    let mut w0 = W0 {{ r: {constant} }};\n"));
            body.push_str("    let s0 = w0.get();\n");
        } else {
            body.push_str(&format!("    let mut w{index} = W{index} {{ r: {constant} }};\n"));
            body.push_str(&format!("    let s{index} = s{} + w{index}.get();\n", index - 1));
        }
    }
    let source = format!(
        "{}{items}fn main() -> i32 {{\n{body}    s{}\n}}\n",
        header(exit, &[]),
        size - 1,
    );
    (source, exit)
}

/// Match: a fieldless enum, one `match` classifier, `size` dispatch calls.
///
/// Mirrors `run-pass/enum_match.g`; the seed rotates the variant sequence and
/// exit = Σ variant values (Add = 3, Sub = 5, Mul = 11).
fn gen_match(seed: u64, size: usize) -> (String, i32) {
    const VALUES: [i32; 3] = [3, 5, 11];
    let phase = (seed % 3) as usize;
    let mut body = String::new();
    let mut exit: i32 = 0;
    for index in 0..size {
        let position = (index + phase) % 3;
        exit = exit.wrapping_add(VALUES[position]);
        if index == 0 {
            body.push_str(&format!(
                "    let r0 = classify(Op::{});\n",
                variant_name(position)
            ));
        } else {
            body.push_str(&format!(
                "    let r{index} = r{} + classify(Op::{});\n",
                index - 1,
                variant_name(position),
            ));
        }
    }
    let source = format!(
        "{}enum Op {{ Add, Sub, Mul }}\n\nfn classify(op: Op) -> i32 {{ match op {{ Op::Add => 3, Op::Sub => 5, Op::Mul => 11 }} }}\n\nfn main() -> i32 {{\n{body}    r{}\n}}\n",
        header(exit, &[]),
        size - 1,
    );
    (source, exit)
}

/// The enum variant at a rotated position (`0..3`).
fn variant_name(position: usize) -> &'static str {
    match position {
        0 => "Add",
        1 => "Sub",
        _ => "Mul",
    }
}

/// Macro: `format!` substitution checks with pinned expected lengths.
///
/// Mirrors `behavior/format_multiarg.g` (`--with-stdlib`); each mismatching
/// check returns `index + 1`, so success is exit 0 and every failure names
/// its row. Expected lengths are computed Rust-side from the same
/// decimal substitution the builtin macro performs.
fn gen_macro(seed: u64, size: usize) -> (String, i32) {
    let mut body = String::new();
    for index in 0..size {
        // Single-substitution check: `format!("k{i}=<c>")` and its pinned
        // decimal length (expected is the exact string the builtin macro
        // produces; constants are positive, so no sign ambiguity).
        let first = element_constant(seed, index);
        let expected_single = format!("k{index}={first}");
        body.push_str(&format!(
            "    let s{index} = format!(\"k{index}={first}\");\n"
        ));
        body.push_str(&format!(
            "    if s{index}.as_str().len() != {} {{ return {}; }}\n",
            expected_single.len(),
            index + 1,
        ));
        if index % 3 == 0 {
            // Two-substitution check: `format!("<a> + <b>")` (the multi-arg
            // path format_multiarg.g pins). `{{{a}}}` renders one literal
            // brace around the substituted value.
            let a = element_constant(seed.wrapping_add(1), index);
            let b = element_constant(seed.wrapping_add(2), index);
            let expected_pair = format!("{a} + {b}");
            body.push_str(&format!(
                "    let p{index} = format!(\"{{{a}}} + {{{b}}}\");\n"
            ));
            body.push_str(&format!(
                "    if p{index}.as_str().len() != {} {{ return {}; }}\n",
                expected_pair.len(),
                index + 1,
            ));
        }
    }
    body.push_str("    println!(\"done\");\n    0\n");
    let source = format!("{}fn main() -> i32 {{\n{body}}}\n", header(0, &["--with-stdlib"]));
    (source, 0)
}

/// Closure: `size` single-capture closures, results chained additively.
///
/// Mirrors `behavior/closure_capture.g`; exit = Σ (capture + argument).
fn gen_closure(seed: u64, size: usize) -> (String, i32) {
    let mut body = String::new();
    let mut exit: i32 = 0;
    for index in 0..size {
        let capture = element_constant(seed, index);
        let argument = argument_constant(seed, index);
        exit = exit.wrapping_add(capture).wrapping_add(argument);
        body.push_str(&format!("    let n{index} = {capture};\n"));
        body.push_str(&format!(
            "    let f{index} = |x: i32| -> i32 {{ x + n{index} }};\n"
        ));
        if index == 0 {
            body.push_str(&format!("    let r0 = f0({argument});\n"));
        } else {
            body.push_str(&format!(
                "    let r{index} = r{} + f{index}({argument});\n",
                index - 1,
            ));
        }
    }
    let source = format!("{}fn main() -> i32 {{\n{body}    r{}\n}}\n", header(exit, &[]), size - 1);
    (source, exit)
}

impl CorpusEntry {
    /// The manifest row for a generated program.
    #[must_use]
    pub fn from_program(program: &CorpusProgram) -> Self {
        Self {
            family: program.family,
            seed: program.seed,
            size: program.size,
            reference_exit: program.reference_exit,
            compile_flags: program.compile_flags.clone(),
            identity: program.identity,
        }
    }
}

impl CorpusManifest {
    /// Build a manifest over `families × seeds` at one `size`.
    ///
    /// Row order: families outer, seeds inner, both in the caller's order —
    /// deterministic by construction.
    ///
    /// # Errors
    ///
    /// [`BenchError::CorpusPlan`] for an empty family/seed list or an
    /// out-of-range `size` (surfaced by the first [`generate`] call).
    pub fn build(
        families: &[CorpusFamily],
        seeds: &[u64],
        size: usize,
    ) -> Result<Self, BenchError> {
        if families.is_empty() || seeds.is_empty() {
            return Err(BenchError::CorpusPlan {
                detail: "families and seeds must be non-empty".to_string(),
            });
        }
        let _span = tracing::info_span!(
            "corpus_manifest_build",
            families = families.len(),
            seeds = seeds.len(),
            size = size,
        )
        .entered();
        let mut programs = Vec::with_capacity(families.len() * seeds.len());
        for family in families {
            for seed in seeds {
                let program = generate(*family, *seed, size)?;
                programs.push(CorpusEntry::from_program(&program));
            }
        }
        Ok(Self {
            schema: CORPUS_SCHEMA.to_string(),
            programs,
        })
    }

    /// Regenerate every row and check it against the recorded identity,
    /// exit code, and flags.
    ///
    /// This is the "corpora committed with hashes" guarantee: a template
    /// edit after the manifest was committed fails here instead of silently
    /// invalidating a benchmark's baseline.
    ///
    /// # Errors
    ///
    /// [`BenchError::CorpusPlan`] if a row's `size` is now out of range, or
    /// [`BenchError::CorpusIdentityMismatch`] on the first drifted row.
    pub fn verify(&self) -> Result<(), BenchError> {
        for entry in &self.programs {
            let program = generate(entry.family, entry.seed, entry.size)?;
            if program.identity != entry.identity
                || program.reference_exit != entry.reference_exit
                || program.compile_flags != entry.compile_flags
            {
                return Err(BenchError::CorpusIdentityMismatch {
                    family: entry.family.as_str().to_string(),
                    seed: entry.seed,
                    expected: entry.identity,
                    actual: program.identity,
                });
            }
        }
        Ok(())
    }

    /// Parse a `glyim.corpus/1` document (strict: `deny_unknown_fields`).
    ///
    /// # Errors
    ///
    /// [`BenchError::CorpusManifestParse`] with the deserialiser's detail.
    pub fn from_json(text: &str) -> Result<Self, BenchError> {
        serde_json::from_str(text).map_err(|err| BenchError::CorpusManifestParse {
            detail: err.to_string(),
        })
    }

    /// Pretty-printed JSON (deterministic field order via the struct shape).
    ///
    /// # Errors
    ///
    /// [`BenchError::CorpusManifestParse`] if serialisation fails (cannot
    /// happen for in-memory values, but kept for the `Result`-shaped seam).
    pub fn to_json_pretty(&self) -> Result<String, BenchError> {
        serde_json::to_string_pretty(self)
            .map(|mut text| {
                text.push('\n');
                text
            })
            .map_err(|err| BenchError::CorpusManifestParse {
                detail: err.to_string(),
            })
    }

    /// Fixed-width human-readable table (deterministic for a manifest).
    #[must_use]
    pub fn render_text(&self) -> String {
        let mut out = String::new();
        out.push_str(&format!(
            "glyim.corpus/1 manifest\n  programs: {}\n",
            self.programs.len()
        ));
        out.push_str("  family   seed  size  exit  identity            flags\n");
        for entry in &self.programs {
            let flags = if entry.compile_flags.is_empty() {
                "-".to_string()
            } else {
                entry.compile_flags.join(",")
            };
            out.push_str(&format!(
                "  {:<8} {:>4} {:>5} {:>5}  {:016x}  {}\n",
                entry.family.as_str(),
                entry.seed,
                entry.size,
                entry.reference_exit,
                entry.identity,
                flags,
            ));
        }
        out
    }
}
