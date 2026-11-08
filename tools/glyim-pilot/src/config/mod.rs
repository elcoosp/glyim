/// types.
pub mod types;
use crate::error::PilotError;
use std::path::Path;
pub use types::*;

/// `load_config`.
pub fn load_config(project_root: &Path) -> Result<PilotConfig, PilotError> {
    let config_path = project_root.join(".glyim-pilot.toml");
    let content = std::fs::read_to_string(&config_path)
        .map_err(|e| PilotError::Config(format!("failed to read config: {e}")))?;
    toml::from_str(&content).map_err(|e| PilotError::Config(format!("failed to parse config: {e}")))
}
