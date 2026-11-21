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
use std::fmt::Write as _;

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
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
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
    /// Lowercase hex SHA-256 of [`CorpusProgram::source`] — the committed
    /// corpus's second identity column (M2.3 "corpora committed with
    /// hashes"); 64 hex chars, computed by [`sha256_hex`].
    pub sha256: String,
}

/// One manifest row: everything needed to regenerate and re-verify a
/// program without storing its source.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
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
    /// Lowercase hex SHA-256 of the program's source text.
    pub sha256: String,
}

/// The committed corpus artifact: identity, rows, and the verify contract.
///
/// Rows are stored in build order (families outer, seeds inner — both in the
/// caller's order), so two builds from the same inputs serialise to
/// identical bytes.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
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

/// The SHA-256 initial hash state (FIPS 180-4, §5.3.3).
const SHA256_H0: [u32; 8] = [
    0x6a09_e667,
    0xbb67_ae85,
    0x3c6e_f372,
    0xa54f_f53a,
    0x510e_527f,
    0x9b05_688c,
    0x1f83_d9ab,
    0x5be0_cd19,
];

/// The SHA-256 round constants (FIPS 180-4, §4.2.3, the first 64 primes).
const SHA256_K: [u32; 64] = [
    0x428a_2f98,
    0x7137_4491,
    0xb5c0_fbcf,
    0xe9b5_dba5,
    0x3956_c25b,
    0x59f1_11f1,
    0x923f_82a4,
    0xab1c_5ed5,
    0xd807_aa98,
    0x1283_5b01,
    0x2431_85be,
    0x550c_7dc3,
    0x72be_5d74,
    0x80de_b1fe,
    0x9bdc_06a7,
    0xc19b_f174,
    0xe49b_69c1,
    0xefbe_4786,
    0x0fc1_9dc6,
    0x240c_a1cc,
    0x2de9_2c6f,
    0x4a74_84aa,
    0x5cb0_a9dc,
    0x76f9_88da,
    0x983e_5152,
    0xa831_c66d,
    0xb003_27c8,
    0xbf59_7fc7,
    0xc6e0_0bf3,
    0xd5a7_9147,
    0x06ca_6351,
    0x1429_2967,
    0x27b7_0a85,
    0x2e1b_2138,
    0x4d2c_6dfc,
    0x5338_0d13,
    0x650a_7354,
    0x766a_0abb,
    0x81c2_c92e,
    0x9272_2c85,
    0xa2bf_e8a1,
    0xa81a_664b,
    0xc24b_8b70,
    0xc76c_51a3,
    0xd192_e819,
    0xd699_0624,
    0xf40e_3585,
    0x106a_a070,
    0x19a4_c116,
    0x1e37_6c08,
    0x2748_774c,
    0x34b0_bcb5,
    0x391c_0cb3,
    0x4ed8_aa4a,
    0x5b9c_ca4f,
    0x682e_6ff3,
    0x748f_82ee,
    0x78a5_636f,
    0x84c8_7814,
    0x8cc7_0208,
    0x90be_fffa,
    0xa450_6ceb,
    0xbef9_a3f7,
    0xc671_78f2,
];

/// SHA-256 over `data` (FIPS 180-4). Hand-rolled rather than pulled from a
/// crate so the manifest's second identity column adds no dependency: the
/// corpus is generator-owned text and the digest is a content address, and
/// the standard vectors in the crate's tests pin correctness.
fn sha256(data: &[u8]) -> [u8; 32] {
    let bit_len = (data.len() as u64).wrapping_mul(8);
    let mut msg = Vec::with_capacity(data.len() + 72);
    msg.extend_from_slice(data);
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&bit_len.to_be_bytes());

    let mut h = SHA256_H0;
    for chunk in msg.as_chunks::<64>().0 {
        let mut w = [0u32; 64];
        for (i, word) in chunk.as_chunks::<4>().0.iter().enumerate() {
            w[i] = u32::from_be_bytes(*word);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }
        let (mut a, mut b, mut c, mut d) = (h[0], h[1], h[2], h[3]);
        let (mut e, mut f, mut g, mut hh) = (h[4], h[5], h[6], h[7]);
        for (i, k) in SHA256_K.iter().enumerate() {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ (!e & g);
            let t1 = hh
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(*k)
                .wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let t2 = s0.wrapping_add(maj);
            hh = g;
            g = f;
            f = e;
            e = d.wrapping_add(t1);
            d = c;
            c = b;
            b = a;
            a = t1.wrapping_add(t2);
        }
        h[0] = h[0].wrapping_add(a);
        h[1] = h[1].wrapping_add(b);
        h[2] = h[2].wrapping_add(c);
        h[3] = h[3].wrapping_add(d);
        h[4] = h[4].wrapping_add(e);
        h[5] = h[5].wrapping_add(f);
        h[6] = h[6].wrapping_add(g);
        h[7] = h[7].wrapping_add(hh);
    }
    let mut digest = [0u8; 32];
    for (i, word) in h.iter().enumerate() {
        digest[i * 4..i * 4 + 4].copy_from_slice(&word.to_be_bytes());
    }
    digest
}

/// Lowercase hex SHA-256 of `data`'s UTF-8 bytes — the manifest's
/// `sha256` identity column (64 chars).
#[must_use]
pub fn sha256_hex(data: &str) -> String {
    let digest = sha256(data.as_bytes());
    let mut hex = String::with_capacity(64);
    for byte in digest {
        let _ = write!(hex, "{byte:02x}");
    }
    hex
}

/// Generate one program: a pure function of `(family, seed, size)`.
///
/// # Errors
///
/// [`BenchError::CorpusPlan`] when `size` is outside `1..=[MAX_SIZE]`.
pub fn generate(family: CorpusFamily, seed: u64, size: usize) -> Result<CorpusProgram, BenchError> {
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
    let sha256 = sha256_hex(&source);
    tracing::debug!(
        identity = format_args!("{identity:016x}"),
        "corpus program generated"
    );
    Ok(CorpusProgram {
        family,
        seed,
        size,
        source,
        reference_exit,
        compile_flags,
        identity,
        sha256,
    })
}

/// The `//@` fixture header shared by every generated program (M1.4 grammar):
/// explicit mode, the by-construction exit code, the registry features the
/// corpus exercises, and the verified backend set.
fn header(reference_exit: i32, extra_flags: &[&str]) -> String {
    let mut text = format!("//@ mode: run-pass\n//@ expect-exit: {reference_exit}\n");
    for flag in extra_flags {
        let _ = writeln!(text, "//@ compile-flags: {flag}");
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
            let _ = writeln!(body, "    let a0 = {constant};");
        } else {
            let _ = writeln!(body, "    let a{index} = {prev} + {constant};");
            prev = format!("a{index}");
        }
    }
    let source = format!(
        "{}fn main() -> i32 {{\n{body}    {prev}\n}}\n",
        header(exit, &[])
    );
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
    let _ = writeln!(body, "        acc = {exit};");
    for _ in 0..size {
        body.push_str("    }\n");
    }
    let source = format!(
        "{}fn main() -> i32 {{\n{body}    acc\n}}\n",
        header(exit, &[])
    );
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
            let _ = writeln!(body, "    let v0 = id({constant});");
        } else {
            let _ = writeln!(body, "    let v{index} = id(v{}) + {constant};", index - 1);
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
        let _ = writeln!(
            items,
            "struct W{index} {{ r: i32 }}\n\nimpl Read for W{index} {{\n    fn get(&mut self) -> i32 {{ self.r }}\n}}\n"
        );
        if index == 0 {
            let _ = writeln!(body, "    let mut w0 = W0 {{ r: {constant} }};");
            body.push_str("    let s0 = w0.get();\n");
        } else {
            let _ = writeln!(body, "    let mut w{index} = W{index} {{ r: {constant} }};");
            let _ = writeln!(body, "    let s{index} = s{} + w{index}.get();", index - 1);
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
            let _ = writeln!(
                body,
                "    let r0 = classify(Op::{});",
                variant_name(position)
            );
        } else {
            let _ = writeln!(
                body,
                "    let r{index} = r{} + classify(Op::{});",
                index - 1,
                variant_name(position),
            );
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
        let _ = writeln!(body, "    let s{index} = format!(\"k{index}={first}\");");
        let _ = writeln!(
            body,
            "    if s{index}.as_str().len() != {} {{ return {}; }}",
            expected_single.len(),
            index + 1,
        );
        if index % 3 == 0 {
            // Two-substitution check: `format!("<a> + <b>")` (the multi-arg
            // path format_multiarg.g pins). `{{{a}}}` renders one literal
            // brace around the substituted value.
            let a = element_constant(seed.wrapping_add(1), index);
            let b = element_constant(seed.wrapping_add(2), index);
            let expected_pair = format!("{a} + {b}");
            let _ = writeln!(body, "    let p{index} = format!(\"{{{a}}} + {{{b}}}\");");
            let _ = writeln!(
                body,
                "    if p{index}.as_str().len() != {} {{ return {}; }}",
                expected_pair.len(),
                index + 1,
            );
        }
    }
    body.push_str("    println!(\"done\");\n    0\n");
    let source = format!(
        "{}fn main() -> i32 {{\n{body}}}\n",
        header(0, &["--with-stdlib"])
    );
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
        let _ = writeln!(body, "    let n{index} = {capture};");
        let _ = writeln!(
            body,
            "    let f{index} = |x: i32| -> i32 {{ x + n{index} }};"
        );
        if index == 0 {
            let _ = writeln!(body, "    let r0 = f0({argument});");
        } else {
            let _ = writeln!(
                body,
                "    let r{index} = r{} + f{index}({argument});",
                index - 1,
            );
        }
    }
    let source = format!(
        "{}fn main() -> i32 {{\n{body}    r{}\n}}\n",
        header(exit, &[]),
        size - 1
    );
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
            sha256: program.sha256.clone(),
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
                || program.sha256 != entry.sha256
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
        let _ = writeln!(
            out,
            "glyim.corpus/1 manifest\n  programs: {}",
            self.programs.len()
        );
        out.push_str("  family   seed  size  exit  identity            sha256           flags\n");
        for entry in &self.programs {
            let flags = if entry.compile_flags.is_empty() {
                "-".to_string()
            } else {
                entry.compile_flags.join(",")
            };
            let _ = writeln!(
                out,
                "  {:<8} {:>4} {:>5} {:>5}  {:016x}  {:<16}  {}",
                entry.family.as_str(),
                entry.seed,
                entry.size,
                entry.reference_exit,
                entry.identity,
                entry.sha256.get(..16).unwrap_or(&entry.sha256),
                flags,
            );
        }
        out
    }
}

/// The committed corpora directory, relative to the repository root (plan
/// M2.3 remainder: "corpora committed with hashes"). The directory holds one
/// `<family>-<seed>-<size>.g` source per manifest row plus the
/// `glyim.corpus/1` `corpus.json`, materialized by
/// `glyim-bench corpus write --dir tools/glyim-bench/corpus --seeds 0,1
/// --size 32` (all seven families) and re-verified by
/// `glyim-bench corpus check --dir tools/glyim-bench/corpus`.
pub const COMMITTED_CORPUS_DIR: &str = "tools/glyim-bench/corpus";

/// The committed corpus's seeds: two per family.
pub const COMMITTED_SEEDS: [u64; 2] = [0, 1];

/// The committed corpus's `size` parameter (every family shares it).
pub const COMMITTED_SIZE: usize = 32;

/// The number of committed programs: `families × seeds` = 14, clearing the
/// master plan's `M2.3` "≥ 12 runtime benchmark programs" floor.
pub const COMMITTED_PROGRAM_COUNT: usize = CorpusFamily::ALL.len() * COMMITTED_SEEDS.len();

/// The reference stdout a committed corpus program must produce.
///
/// Only the `macro` family prints (`println!("done")` closing its
/// `format!` check chain); every other family communicates through its exit
/// code alone, so its pinned stdout is empty. The exit half of the reference
/// output lives on [`CorpusEntry::reference_exit`].
#[must_use]
pub fn reference_stdout(family: CorpusFamily) -> &'static str {
    match family {
        CorpusFamily::Macro => "done\n",
        _ => "",
    }
}

#[cfg(test)]
mod committed_corpus_tests {
    use super::{
        COMMITTED_PROGRAM_COUNT, COMMITTED_SEEDS, COMMITTED_SIZE, CorpusEntry, CorpusFamily,
        CorpusManifest, reference_stdout,
    };
    use std::path::Path;

    /// The committed directory must satisfy the manifest's own verify
    /// contract, every committed byte must hash to its row (both identity
    /// columns), and the manifest must cover the committed plan.
    #[test]
    fn committed_corpus_matches_the_manifest() {
        // COMMITTED_CORPUS_DIR is repo-root-relative; this crate's manifest
        // dir is the repo's `tools/glyim-bench`, so the committed directory
        // is `corpus/` beside the manifest — assert the two spellings agree.
        assert_eq!(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .file_name()
                .and_then(|name| name.to_str()),
            Some("glyim-bench")
        );
        let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("corpus")
            .canonicalize()
            .expect("the committed corpus directory exists");
        let manifest = CorpusManifest::from_json(
            &std::fs::read_to_string(dir.join("corpus.json")).expect("committed manifest"),
        )
        .expect("the committed manifest parses");
        assert_eq!(
            manifest.programs.len(),
            COMMITTED_PROGRAM_COUNT,
            "the committed manifest must cover the committed plan"
        );
        manifest
            .verify()
            .expect("the templates must regenerate every committed row");
        for entry in &manifest.programs {
            let source = std::fs::read_to_string(dir.join(format!(
                "{}-{}-{}.g",
                entry.family.as_str(),
                entry.seed,
                entry.size
            )))
            .expect("every committed program file exists");
            assert_eq!(
                super::fnv1a64(&source),
                entry.identity,
                "committed bytes must match the manifest's fnv identity"
            );
            assert_eq!(
                super::sha256_hex(&source),
                entry.sha256,
                "committed bytes must match the manifest's sha256 identity"
            );
        }
    }

    /// The committed plan clears the M2.3 floor and covers every family.
    #[test]
    fn committed_plan_covers_every_family_twice() {
        // Compile-time plan checks (const-evaluable, so runtime asserts
        // would be constant-value warnings): the floor and the product.
        const {
            assert!(
                COMMITTED_PROGRAM_COUNT >= 12,
                "M2.3 DoD: >= 12 runtime benchmark programs"
            );
            assert!(
                CorpusFamily::ALL.len() * COMMITTED_SEEDS.len() == COMMITTED_PROGRAM_COUNT,
                "the committed plan must be families × seeds"
            );
            assert!(COMMITTED_SIZE > 0);
        }
    }

    /// Only the macro family prints; every family's exit is pinned on the
    /// entry.
    #[test]
    fn reference_stdout_is_pinned_per_family() {
        for family in CorpusFamily::ALL {
            let expected = if family == CorpusFamily::Macro {
                "done\n"
            } else {
                ""
            };
            assert_eq!(reference_stdout(family), expected);
        }
    }

    /// SHA-256 correctness: the FIPS 180-4 standard vectors.
    #[test]
    fn sha256_matches_the_standard_vectors() {
        assert_eq!(
            super::sha256_hex(""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            super::sha256_hex("abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(
            super::sha256_hex("The quick brown fox jumps over the lazy dog"),
            "d7a8fbb307d7809469ca9abcb0082e4f8d5651e46d3cdb762d02d0bf37c9e592"
        );
    }

    /// The entry row's sha256 column must agree with the program's.
    #[test]
    fn entry_rows_carry_the_sha256_identity() {
        let program =
            super::generate(CorpusFamily::Wide, 0, COMMITTED_SIZE).expect("size in range");
        let entry = CorpusEntry::from_program(&program);
        assert_eq!(entry.sha256, super::sha256_hex(&program.source));
    }
}
