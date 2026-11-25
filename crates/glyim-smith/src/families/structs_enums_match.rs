//! Structs/enums/match family: enum dispatch, struct fields, trait methods.
//!
//! Shapes verified by the match/trait bench-corpus families and the
//! `match_ref_struct_field.g` fixture:
//!
//! ```text
//! enum E0 { A0, A1, A2 }
//! fn classify(e: E0) -> i32 { match e { E0::A0 => k0, E0::A1 => k1, E0::A2 => k2 } }
//! struct S0 { r: i32 }
//! trait R0 { fn get(&mut self) -> i32; }
//! impl R0 for S0 { fn get(&mut self) -> i32 { self.r } }
//! fn main() -> i32 {
//!     let mut s = S0 { r: c };
//!     let v0 = classify(E0::A1);
//!     let v1 = v0 + s.get();
//!     let v2 = v1 + s.r;
//!     …
//!     vn
//! }
//! ```
//!
//! Variant count, per-arm constants, and the dispatch sequence are seeded;
//! the family simulates every `classify`/`get`/field read, so the checksum
//! is exact and every value stays below `~100_400`.

use super::{VALUE_MOD, finish};
use crate::generator::{ProgramGenerator, SmithProgram};
use crate::profile::GeneratorProfile;
use crate::rng::SmithRng;

use std::fmt::Write as _;

/// Per-arm constants: small, distinct values keep the match table honest
/// (equal arms would collapse dispatch diversity).
const ARM_VALUES: [i32; 6] = [3, 5, 11, 17, 23, 29];

/// Emits an enum + match classifier and a struct read two ways (field
/// access and trait method), interleaved for `size` steps.
pub struct StructsEnumsMatch;

impl ProgramGenerator for StructsEnumsMatch {
    fn profile(&self) -> GeneratorProfile {
        GeneratorProfile::StructsEnumsMatch
    }

    fn generate(&self, seed: u64, rng: &mut SmithRng, size: usize) -> SmithProgram {
        let variants = rng.range(2, 4) as usize;
        // Distinct per-arm constants: shuffle a slice of the pool by seed.
        let mut arms: Vec<i32> = ARM_VALUES.to_vec();
        for index in (1..arms.len()).rev() {
            let j = rng.below((index + 1) as u64) as usize;
            arms.swap(index, j);
        }
        let mut items = String::new();
        let _ = writeln!(items, "enum E0 {{ {} }}", {
            let names: Vec<String> = (0..variants).map(|v| format!("A{v}")).collect();
            names.join(", ")
        });
        let mut match_arms = String::new();
        for (v, value) in arms.iter().enumerate().take(variants) {
            let _ = writeln!(match_arms, "        E0::A{v} => {value},");
        }
        let _ = writeln!(
            items,
            "fn classify(e: E0) -> i32 {{\n    match e {{\n{match_arms}    }}\n}}"
        );
        let field_const = *rng.choose(&ARM_VALUES);
        let _ = writeln!(
            items,
            "\nstruct S0 {{ r: i32 }}\n\ntrait R0 {{\n    fn get(&mut self) -> i32;\n}}\n\nimpl R0 for S0 {{\n    fn get(&mut self) -> i32 {{ self.r }}\n}}"
        );

        let mut body = String::new();
        let _ = writeln!(body, "    let mut s = S0 {{ r: {field_const} }};");
        let mut value = 0;
        for index in 0..size {
            let variant = rng.below(variants as u64) as usize;
            let fold = if index == 0 {
                value = arms[variant] + field_const;
                format!("let v{index} = classify(E0::A{variant}) + s.r;")
            } else {
                let method_hit = rng.chance(50);
                let step = if method_hit {
                    value += field_const;
                    "s.get()".to_string()
                } else {
                    let v = rng.below(variants as u64) as usize;
                    value += arms[v];
                    format!("classify(E0::A{v})")
                };
                format!("let v{index} = v{} + {};", index - 1, step)
            };
            let _ = writeln!(body, "    {fold}");
            // Keep the accumulator in the positive low range so the final
            // fold matches the simulated remainder exactly.
            if value >= VALUE_MOD {
                value %= VALUE_MOD;
                let _ = writeln!(body, "    let v{index} = v{index} % {VALUE_MOD};");
            }
        }
        let items_and_body = format!("{items}\n");
        finish(
            self.profile(),
            seed,
            size,
            &items_and_body,
            &body,
            &format!("v{}", size - 1),
            value,
        )
    }
}
