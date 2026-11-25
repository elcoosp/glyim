//! Metamorphic-suite throughput bench (plan M4.7 v1).
//!
//! Dependency-free (`harness = false`), mirroring the repo bench
//! convention. Measures cases/second for each relation over a fixed
//! smith-generated workload — the nightly-tier `DoD` ("relations run
//! nightly over fixtures + generated programs") budgets off these
//! numbers.
//!
//! Run with:
//!
//! ```text
//! cargo bench -p glyim-test --bench metamorphic
//! ```

use glyim_smith::{GeneratorProfile, generate};
use glyim_test::metamorphic::{MetamorphicRunner, all_relations};
use std::hint::black_box;
use std::time::Instant;

fn main() {
    let workdir = tempfile::tempdir().expect("tempdir");
    let runner = MetamorphicRunner::new(workdir.path().to_path_buf());
    let relations = all_relations();
    let programs: Vec<String> = GeneratorProfile::ALL
        .iter()
        .flat_map(|profile| {
            (0..4u64).map(move |seed| generate(*profile, seed, 16).expect("in-bounds").source)
        })
        .collect();

    for relation in &relations {
        let started = Instant::now();
        let mut cases = 0u32;
        for program in &programs {
            let outcome = relation.check(
                black_box(program),
                &mut glyim_smith::SmithRng::new(0),
                &runner,
            );
            debug_assert!(!matches!(
                outcome,
                glyim_test::metamorphic::RelationOutcome::Diverged { .. }
            ));
            cases += 1;
        }
        let per_second = f64::from(cases) / started.elapsed().as_secs_f64();
        println!("{:>22}: {per_second:>8.1} cases/second", relation.name());
    }
}
