//! The five v1 generator families (plan M4.3 scope, in plan order).
//!
//! Every family shares the same shape: it emits Glyim source text and
//! simulates the emitted statements in lockstep so the checksum is exact
//! and every intermediate value stays inside `0..1_000_000` (no overflow,
//! no negative values where the grammar's subtraction is used, no
//! division — the plan's "well-typed, UB-free" contract at generation
//! time). Constants and shapes are drawn from the seeded [`SmithRng`],
//! never from a clock or the environment.

use crate::generator::SmithProgram;
use crate::profile::GeneratorProfile;
use crate::rng::SmithRng;

pub mod control_flow;
pub mod functions;
pub mod generics;
pub mod scalars;
pub mod structs_enums_match;

pub use control_flow::ControlFlow;
pub use functions::Functions;
pub use generics::Generics;
pub use scalars::Scalars;
pub use structs_enums_match::StructsEnumsMatch;

/// Prime modulus folded in after multiplications so every materialized
/// value stays below ~1e6 (well clear of `i32` overflow) without making
/// the program trivial: each `%` re-binds the value, and the generator's
/// simulation applies the identical fold.
const VALUE_MOD: i32 = 100_003;

/// Builds the shared `//@` fixture header (the M1.4 grammar) for a
/// generated program: explicit mode, the by-construction checksum exit,
/// the registry features the profile exercises, and the honest backend
/// set (v1 is interpreter-verified; LLVM coverage arrives with the M4.4
/// differential driver).
fn header(expected_exit: i32, profile: GeneratorProfile) -> String {
    format!(
        "//@ mode: run-pass\n//@ expect-exit: {}\n//@ feature: {}\n//@ backends: gam\n",
        crate::generator::masked_exit(expected_exit),
        profile.feature_tags(),
    )
}

/// Emits the `fn main() -> i32 { … }` wrapper around an indented body.
fn main_fn(body: &str, final_expr: &str) -> String {
    format!("fn main() -> i32 {{\n{body}    {final_expr}\n}}\n")
}

/// Wraps a family's item prelude and main body into a complete
/// [`SmithProgram`]. `prelude` carries top-level items the family declares
/// ahead of `main` (empty for statement-only families).
fn finish(
    profile: GeneratorProfile,
    seed: u64,
    size: usize,
    prelude: &str,
    body: &str,
    final_expr: &str,
    expected_exit: i32,
) -> SmithProgram {
    SmithProgram {
        profile,
        seed,
        size,
        source: format!(
            "{}{prelude}{}",
            header(expected_exit, profile),
            main_fn(body, final_expr)
        ),
        expected_exit,
    }
}

/// One arithmetic step shared by the scalar/control-flow families: returns
/// the source text for `v OP c` (with the `%` fold after `*`) and the
/// simulated result. Subtraction only when it stays positive; multiplication
/// only when both factors stay small; both guarded by the caller's choice.
pub(crate) struct ArithStep {
    /// Emitted source text for the right-hand side (not yet parenthesized).
    pub text: String,
    /// Simulated value of `v OP c` after the `%` fold (if any).
    pub value: i32,
}

/// Picks a random safe arithmetic combination of `v` and a constant,
/// returning the emitted expression text and its simulated value.
pub(crate) fn arith_step(rng: &mut SmithRng, v: i32, constant_pool: &[i32]) -> ArithStep {
    let c = *rng.choose(constant_pool);
    // Operator order is a fixed weighted menu; the guards keep every
    // outcome inside bounds, so the menu is safe to draw from blindly.
    match rng.below(3) {
        0 => ArithStep {
            text: format!("{v} + {c}"),
            value: v + c,
        },
        1 if v > c => ArithStep {
            text: format!("{v} - {c}"),
            value: v - c,
        },
        _ => {
            // Multiplication is only safe when both factors are small;
            // when `v` is large, fall back to an addition that stays small
            // by wrapping through the prime modulus.
            if v <= 1_000 && c <= 1_000 {
                ArithStep {
                    text: format!("({v} * {c}) % {VALUE_MOD}"),
                    value: (v * c) % VALUE_MOD,
                }
            } else {
                ArithStep {
                    text: format!("({v} + {c}) % {VALUE_MOD}"),
                    value: (v + c) % VALUE_MOD,
                }
            }
        }
    }
}
