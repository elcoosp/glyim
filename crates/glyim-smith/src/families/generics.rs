//! Generics family: generic identity/projection functions monomorphized
//! with `i32`.
//!
//! Shapes verified by the generic bench-corpus family plus the in-crate
//! probe (`fn first<T>(a: T, b: T) -> T { a }` compiles and runs):
//!
//! ```text
//! fn id<T>(x: T) -> T { x }
//! fn first<T>(a: T, b: T) -> T { a }
//! fn main() -> i32 {
//!     let v0 = id(c0);
//!     let v1 = first(id(v0), c1);
//!     let v2 = id(v1) + c2;
//!     …
//!     vn
//! }
//! ```
//!
//! `first` always returns its first argument, so the simulation threads
//! the first call-site argument exactly; `id` is the identity. Values
//! grow only through the additive tail, bounded like the scalars family.

use super::{arith_step, finish};
use crate::generator::{ProgramGenerator, SmithProgram};
use crate::profile::GeneratorProfile;
use crate::rng::SmithRng;

use std::fmt::Write as _;

/// Constant pool for generic call-site arguments and additive tails.
const CONSTANTS: [i32; 7] = [4, 9, 16, 25, 36, 49, 64];

/// Emits generic helpers and a main body of monomorphized call chains.
pub struct Generics;

impl ProgramGenerator for Generics {
    fn profile(&self) -> GeneratorProfile {
        GeneratorProfile::Generics
    }

    fn generate(&self, seed: u64, rng: &mut SmithRng, size: usize) -> SmithProgram {
        let mut items = String::new();
        let _ = writeln!(items, "fn id<T>(x: T) -> T {{ x }}");
        let _ = writeln!(items, "fn first<T>(a: T, b: T) -> T {{ a }}\n");

        let mut body = String::new();
        let mut value = *rng.choose(&CONSTANTS);
        let _ = writeln!(body, "    let v0 = id({value});");
        for index in 1..size {
            match rng.below(3) {
                0 => {
                    let c = *rng.choose(&CONSTANTS);
                    let _ = writeln!(body, "    let v{index} = id(v{}) + {c};", index - 1);
                    value += c;
                }
                1 => {
                    let c = *rng.choose(&CONSTANTS);
                    let _ = writeln!(body, "    let v{index} = first(id(v{0}), {c});", index - 1);
                }
                _ => {
                    let step = arith_step(rng, value, &CONSTANTS);
                    let _ = writeln!(body, "    let v{index} = {};", step.text);
                    value = step.value;
                    continue;
                }
            }
            if value >= super::VALUE_MOD {
                value %= super::VALUE_MOD;
                let _ = writeln!(body, "    let v{index} = v{index} % {};", super::VALUE_MOD);
            }
        }
        finish(
            self.profile(),
            seed,
            size,
            &format!("{items}\n"),
            &body,
            &format!("v{}", size - 1),
            value,
        )
    }
}
