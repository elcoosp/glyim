//! Control-flow family: bounded `while` loops plus `if`/`else` trees.
//!
//! Shape (all statements verified by `crates/glyim-test` fixtures
//! `baseline_while.g` and the deep bench-corpus family):
//!
//! ```text
//! let mut acc = c0;
//! let mut i = 0;
//! while i < n { acc = acc + s; i = i + 1; }
//! if acc == k { acc = acc + d1; } else { acc = acc + d2; }
//! acc = acc % mod;
//! acc
//! ```
//!
//! Both branch arms add (different constants), so every intermediate value
//! stays positive and the `%` remainder never sees a negative operand —
//! the simulation then matches the program exactly. The loop is simulated
//! for its full iteration count, so `acc` entering the branches is exact;
//! the `if` conditions hit the `then` branch with 40% probability across
//! the seed space and provably fall to `else` otherwise.

use super::{VALUE_MOD, finish};
use crate::generator::{ProgramGenerator, SmithProgram};
use crate::profile::GeneratorProfile;
use crate::rng::SmithRng;

use std::fmt::Write as _;

/// Loop-step and branch increments drawn from a small positive pool.
const ADDS: [i32; 6] = [2, 3, 5, 7, 11, 13];

/// Emits bounded loops and guarded branches over one `mut` accumulator.
pub struct ControlFlow;

impl ProgramGenerator for ControlFlow {
    fn profile(&self) -> GeneratorProfile {
        GeneratorProfile::ControlFlow
    }

    fn generate(&self, seed: u64, rng: &mut SmithRng, size: usize) -> SmithProgram {
        let mut body = String::new();
        let mut acc = *rng.choose(&ADDS);
        let iterations = rng.range(1, 8);
        let step = *rng.choose(&ADDS);
        let _ = writeln!(body, "    let mut acc = {acc};");
        let _ = writeln!(body, "    let mut i = 0;");
        let _ = writeln!(
            body,
            "    while i < {iterations} {{ acc = acc + {step}; i = i + 1; }}"
        );
        acc += step * iterations;

        // Guarded branch tree; `size` scales the depth.
        let depth = (size / 16).clamp(1, 4);
        for _ in 0..depth {
            let equal_hit = rng.chance(40);
            let k = if equal_hit {
                acc
            } else {
                acc + rng.range(1, 99)
            };
            let then_delta = *rng.choose(&ADDS);
            let else_delta = *rng.choose(&ADDS);
            if equal_hit {
                acc += then_delta;
            } else {
                acc += else_delta;
            }
            let _ = writeln!(
                body,
                "    if acc == {k} {{ acc = acc + {then_delta}; }} else {{ acc = acc + {else_delta}; }}"
            );
        }
        let _ = writeln!(body, "    acc = acc % {VALUE_MOD};");
        acc %= VALUE_MOD;
        finish(self.profile(), seed, size, "", &body, "acc", acc)
    }
}
