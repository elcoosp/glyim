//! The [`ProgramGenerator`] seam and the dispatch entry point.
//!
//! Design (plan §5.3 CO-5, §6 M4.3):
//!
//! * **Typed synthesis.** Every family tracks the concrete `i32` value of
//!   each binding as it emits source text, so expressions are type-correct
//!   by construction and every arithmetic operation is bounds-checked
//!   *at generation time* — a value that would overflow `i32` (or divide by
//!   zero, or go negative through subtraction) is never emitted. This is
//!   the Csmith-lineage "well-typed, UB-free" contract.
//! * **Checksum printing.** Because the family simulates what it emits,
//!   the expected exit checksum is computed in lockstep with the source
//!   (§3.11 anti-gaming: generated programs always assert).
//! * **Determinism.** `generate(profile, seed, size)` is a pure function
//!   of its arguments (constitution C6): same triple, byte-identical
//!   source, on every machine.

use crate::profile::GeneratorProfile;
use crate::rng::SmithRng;

/// Upper bound of the `size` parameter; keeps synthesized programs inside
/// the interpreter's fuel budget and the `DoD` sample's compile-time budget.
pub const MAX_SIZE: usize = 96;

/// The checksum contract shared by every generated program: the harness
/// masks a process exit through `& 0xff` exactly like the OS does (see
/// `glyim_mir_interp::run`), so the expected value is compared through the
/// same mask.
#[must_use]
pub fn masked_exit(code: i32) -> i32 {
    code & 0xff
}

/// One generated program: the source text (fixture header included), the
/// checksum the program must exit with, and the `(profile, seed, size)`
/// identity that reproduces it byte-for-byte.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SmithProgram {
    /// The profile slice that generated the program.
    pub profile: GeneratorProfile,
    /// The seed the program was generated from (recorded per plan M4.3).
    pub seed: u64,
    /// The size budget the program was generated under.
    pub size: usize,
    /// Complete source text, `//@` fixture header included — droppable
    /// straight into `crates/glyim-test/tests/run-pass/`.
    pub source: String,
    /// The exit checksum the program must produce (unmasked; compare
    /// through [`masked_exit`]).
    pub expected_exit: i32,
}

/// Errors from [`generate`]: only the size bound is fallible; generation
/// itself cannot fail (the families never emit an invalid program).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SmithError {
    /// What rejected the request (size bound violations carry the value).
    pub detail: String,
}

impl std::fmt::Display for SmithError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "glyim-smith: {}", self.detail)
    }
}

impl std::error::Error for SmithError {}

/// The per-family generation seam (plan CO-5: "grows by feature" — a new
/// language slice is a new `ProgramGenerator` impl, not a rewrite).
pub trait ProgramGenerator {
    /// The profile slice this family implements.
    fn profile(&self) -> GeneratorProfile;

    /// Generates one program under `size` statement/item budget. The seed
    /// rides along so the family can stamp the returned program with its
    /// full `(profile, seed, size)` identity; the RNG stream stays the only
    /// generation state.
    fn generate(&self, seed: u64, rng: &mut SmithRng, size: usize) -> SmithProgram;
}

/// Generates one program: a pure function of `(profile, seed, size)`.
///
/// # Errors
///
/// [`SmithError`] when `size` is outside `1..=[MAX_SIZE]`.
pub fn generate(
    profile: GeneratorProfile,
    seed: u64,
    size: usize,
) -> Result<SmithProgram, SmithError> {
    if size == 0 || size > MAX_SIZE {
        return Err(SmithError {
            detail: format!("size must be within 1..={MAX_SIZE}, got {size}"),
        });
    }
    let mut rng = SmithRng::new(seed);
    let generator: Box<dyn ProgramGenerator> = match profile {
        GeneratorProfile::Scalars => Box::new(crate::families::Scalars),
        GeneratorProfile::ControlFlow => Box::new(crate::families::ControlFlow),
        GeneratorProfile::StructsEnumsMatch => Box::new(crate::families::StructsEnumsMatch),
        GeneratorProfile::Functions => Box::new(crate::families::Functions),
        GeneratorProfile::Generics => Box::new(crate::families::Generics),
    };
    debug_assert_eq!(generator.profile(), profile);
    Ok(generator.generate(seed, &mut rng, size))
}
