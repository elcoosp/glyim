pub mod keys;

/// A dependency recorded during comptime evaluation for invalidation.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Dependency {
    TraitImpl(String, String),
    TypeFields(String),
    Query(u64),
}

/// Configuration for the query system integration.
pub struct QueryConfig {
    pub enabled: bool,
    pub persist_state: bool,
    pub state_path: Option<std::path::PathBuf>,
}

impl Default for QueryConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            persist_state: false,
            state_path: None,
        }
    }
}
