//! Functions family: top-level helper functions composed into call chains.
//!
//! Shape (helper chains and `if`-expression bodies verified by the probes
//! recorded in `docs/` and the wide bench-corpus family):
//!
//! ```text
//! fn f0(x: i32) -> i32 { x + c0 }
//! fn f1(x: i32) -> i32 { if x == 0 { c1 } else { f0(x) } }
//! fn f2(x: i32) -> i32 { f1(f0(x)) + c2 }
//! fn main() -> i32 {
//!     let v0 = f0(seed_const);
//!     let v1 = f1(v0);
//!     …
//!     vn
//! }
//! ```
//!
//! Every helper body shape is simulated: `x + c` is a function of the
//! argument the call site passes, so the family threads the concrete
//! argument value through the chain and the checksum is exact.

use super::{VALUE_MOD, finish};
use crate::generator::{ProgramGenerator, SmithProgram};
use crate::profile::GeneratorProfile;
use crate::rng::SmithRng;

use std::fmt::Write as _;
use std::rc::Rc;

/// Helper-function additive constants.
const CONSTANTS: [i32; 6] = [2, 5, 9, 17, 31, 63];

/// One helper function's pure `i32 -> i32` simulation, shared with the
/// composed bodies (`Arc` because a helper's simulation captures earlier
/// helpers' simulations).
struct Helper {
    apply: Rc<dyn Fn(i32) -> i32>,
}

/// Emits helper functions in call order, then a main body that walks the
/// chain `size` steps deep.
pub struct Functions;

impl ProgramGenerator for Functions {
    fn profile(&self) -> GeneratorProfile {
        GeneratorProfile::Functions
    }

    fn generate(&self, seed: u64, rng: &mut SmithRng, size: usize) -> SmithProgram {
        let count = size.clamp(1, 8);
        let mut items = String::new();
        let mut helpers: Vec<Helper> = Vec::new();
        for index in 0..count {
            let c = *rng.choose(&CONSTANTS);
            let (text, apply): (String, Rc<dyn Fn(i32) -> i32>) = match helpers.len() {
                0 => (format!("fn f{index}(x: i32) -> i32 {{ x + {c} }}"), {
                    Rc::new(move |x| x + c)
                }),
                // From the second helper on, draw a body shape that calls
                // earlier helpers: a guarded constant (exercises `if` as an
                // expression with both branches over the seed space) or a
                // two-call composition.
                _ if rng.chance(50) => (
                    format!(
                        "fn f{index}(x: i32) -> i32 {{ if x == 0 {{ {c} }} else {{ f{}(x) + {c} }} }}",
                        index - 1
                    ),
                    {
                        let prev =
                            Rc::clone(&helpers.last().expect("non-empty by the guard").apply);
                        Rc::new(move |x| if x == 0 { c } else { prev(x) + c })
                    },
                ),
                _ => (
                    format!(
                        "fn f{index}(x: i32) -> i32 {{ f{}(f{}(x)) + {c} }}",
                        index - 1,
                        { if index >= 2 { index - 2 } else { index - 1 } }
                    ),
                    {
                        let inner = Rc::clone(
                            &helpers[if index >= 2 { index - 2 } else { index - 1 }].apply,
                        );
                        let outer =
                            Rc::clone(&helpers.last().expect("non-empty by the guard").apply);
                        Rc::new(move |x| outer(inner(x)) + c)
                    },
                ),
            };
            let _ = writeln!(items, "{text}\n");
            helpers.push(Helper { apply });
        }

        let mut body = String::new();
        let mut value = *rng.choose(&CONSTANTS);
        let _ = writeln!(body, "    let v0 = f0({value});");
        value = (helpers[0].apply)(value);
        for index in 1..count {
            let which = rng.below(helpers.len() as u64) as usize;
            let _ = writeln!(body, "    let v{index} = f{which}(v{});", index - 1);
            value = (helpers[which].apply)(value);
            if value >= VALUE_MOD {
                value %= VALUE_MOD;
                let _ = writeln!(body, "    let v{index} = v{index} % {VALUE_MOD};");
            }
        }
        // Statement budget beyond the helper chain: extra folded rewrites.
        for index in count..size {
            let c = *rng.choose(&CONSTANTS);
            let pre = value;
            value = (pre + c) % VALUE_MOD;
            let _ = writeln!(body, "    let v{index} = v{} + {c};", index - 1);
            if pre + c >= VALUE_MOD {
                let _ = writeln!(body, "    let v{index} = v{index} % {VALUE_MOD};");
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
