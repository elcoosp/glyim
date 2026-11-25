//! Scalars family: straight-line `i32` arithmetic chains.
//!
//! The program is one chain of `let vN = <expr>;` where each expression
//! combines the previous binding with a random constant through a bounds-
//! guarded operator draw (see [`crate::families::arith_step`]). The
//! generator simulates every step, so the final binding's value is the
//! checksum, and every intermediate value stays below `~100_257`.

use super::{arith_step, finish};
use crate::generator::{ProgramGenerator, SmithProgram};
use crate::profile::GeneratorProfile;
use crate::rng::SmithRng;

use std::fmt::Write as _;

/// Constant pool: small positive values keep `*` factors small and `-`
/// results non-negative.
const CONSTANTS: [i32; 8] = [3, 7, 11, 19, 41, 73, 127, 255];

/// Emits arithmetic chains over a single evolving `i32` binding.
pub struct Scalars;

impl ProgramGenerator for Scalars {
    fn profile(&self) -> GeneratorProfile {
        GeneratorProfile::Scalars
    }

    fn generate(&self, seed: u64, rng: &mut SmithRng, size: usize) -> SmithProgram {
        let mut body = String::new();
        let first = *rng.choose(&CONSTANTS);
        let mut value = first;
        let _ = writeln!(body, "    let v0 = {first};");
        for index in 1..size {
            let step = arith_step(rng, value, &CONSTANTS);
            value = step.value;
            let _ = writeln!(body, "    let v{index} = {};", step.text);
        }
        finish(
            self.profile(),
            seed,
            size,
            "",
            &body,
            &format!("v{}", size - 1),
            value,
        )
    }
}
