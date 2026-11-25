//! # glyim-smith
//!
//! Deterministic typed program generator — plan M4.3
//! (`docs/roadmaps/glyim-master-plan.md` §6 M4.3, design §5.3 CO-5).
//!
//! Every program is a **pure function of `(profile, seed, size)`**: the
//! same triple regenerates byte-identical source on any machine (no
//! clock, no OS entropy, no external `rand` — constitution C6). Each
//! family tracks the concrete `i32` value of every binding while it
//! emits, so expressions are type-correct by construction and every
//! arithmetic step is bounds-checked *at generation time* — the plan's
//! "well-typed, UB-free" contract. Because the families simulate what
//! they emit, every program ends in a checksum print whose expected exit
//! the harness can verify (§3.11 anti-gaming: generated programs always
//! assert).
//!
//! ## Example
//!
//! ```rust
//! use glyim_smith::{GeneratorProfile, generate, masked_exit};
//!
//! let program = generate(GeneratorProfile::Scalars, 42, 8).expect("size in-bounds");
//! assert_eq!(program.seed, 42);
//! assert_eq!(program.size, 8);
//! // Byte-identical regeneration from the recorded identity triple:
//! let again = generate(GeneratorProfile::Scalars, 42, 8).expect("size in-bounds");
//! assert_eq!(program.source, again.source);
//! // The checksum contract: the program exits with `expected_exit` masked
//! // through the OS exit-status low byte.
//! assert_eq!(masked_exit(program.expected_exit), program.expected_exit & 0xff);
//! println!("{}", program.source); // complete `//@`-headed fixture text
//! ```
//!
//! ## Profiles
//!
//! [`GeneratorProfile::ALL`] enumerates the v1 slices in plan order:
//! scalars, control flow, structs/enums/match, functions, generics. The
//! generated fixture headers tag the ledger features each slice
//! exercises, so a future campaign can slice evidence per profile
//! (plan §3.10 RG-01).

pub mod families;
pub mod generator;
pub mod profile;
pub mod rng;

pub use generator::{MAX_SIZE, ProgramGenerator, SmithError, SmithProgram, generate, masked_exit};
pub use profile::GeneratorProfile;
pub use rng::SmithRng;
