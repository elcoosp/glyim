use super::budget::TokenBudget;
use crate::config::types::PilotConfig;
use crate::error::PilotError;
use std::sync::Arc;

/// AssembledContext.
pub struct AssembledContext {
    /// Struct.
    pub prompt: String,
    /// Struct.
    pub total_tokens: usize,
    /// Struct.
    pub tier1_tokens: usize,
    /// Struct.
    pub tier2_tokens: usize,
    /// Struct.
    pub tier3_tokens: usize,
}

/// ContextAssembler.
pub struct ContextAssembler {
    project_root: std::path::PathBuf,
    config: Arc<PilotConfig>,
}

impl ContextAssembler {
    /// new.
    pub async fn new(project_root: std::path::PathBuf, config: Arc<PilotConfig>) -> Self {
        Self {
            project_root,
            config,
        }
    }
    /// assemble.
    pub async fn assemble(
        &self,
        _stream_id: &str,
        owned_files: &[String],
        _dependency_interfaces: &[String],
        ___test_files: &[String],
        provider_id: &str,
    ) -> Result<AssembledContext, PilotError> {
        let max_tokens = self
            .config
            .context
            .providers
            .get(provider_id)
            .map(|c| c.max_context_tokens)
            .unwrap_or(self.config.context.max_context_tokens);
        let mut budget = TokenBudget::new(max_tokens);
        let mut prompt = String::new();

        let tier1 = "# Glyim Compiler Development\n\n## File Operations Skill\n".to_string();
        let tier1_tokens = TokenBudget::estimate_tokens(&tier1);
        budget.force_allocate(tier1_tokens);
        prompt.push_str(&tier1);

        // Simple owned files inclusion
        for path in owned_files {
            let full = self.project_root.join(path);
            if let Ok(content) = tokio::fs::read_to_string(&full).await {
                let section = format!("\n### {path}\n```rust\n{}\n```\n", content);
                let tokens = TokenBudget::estimate_tokens(&section);
                if budget.try_allocate(tokens) {
                    prompt.push_str(&section);
                }
            }
        }
        let tier2_tokens = budget.used_tokens - tier1_tokens;
        Ok(AssembledContext {
            prompt,
            total_tokens: budget.used_tokens,
            tier1_tokens,
            tier2_tokens,
            tier3_tokens: 0,
        })
    }
}
