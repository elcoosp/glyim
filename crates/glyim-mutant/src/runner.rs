use crate::config::MutationConfig;
use crate::engine::{MutationEngine, Mutation};

pub struct MutationRunner {
    #[allow(dead_code)]
    config: MutationConfig,
    engine: MutationEngine,
}

#[allow(dead_code)]
impl MutationRunner {
    pub fn new(config: MutationConfig) -> Self {
        let engine = MutationEngine::new(config.clone());
        Self { config, engine }
    }

    /// Run mutation testing on the given source code.
    /// Returns a vector of (mutation, killed: bool).
    pub fn run(&mut self, source: &str, _test_names: &[String]) -> Vec<(Mutation, bool)> {
        // Parse and type-check original source to get HIR
        let parse_out = glyim_parse::parse(source);
        if !parse_out.errors.is_empty() {
            return vec![];
        }
        let mut interner = parse_out.interner;
        let hir = glyim_hir::lower(&parse_out.ast, &mut interner);

        // Generate mutations
        let mutations = self.engine.generate_mutations(&hir);

        // Stub: mark all as survived (not killed)
        mutations.into_iter().map(|m| (m, false)).collect()
    }
}
