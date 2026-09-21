use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone)]
/// GateContext.
pub struct GateContext {
    /// Struct.
    pub worktree_dir: PathBuf,
    /// Struct.
    pub project_root: PathBuf,
    /// Struct.
    pub default_branch: String,
    /// Struct.
    pub branch_version: String,
    /// Struct.
    pub timeout_secs: u64,
    /// Struct.
    pub changed_files: Vec<String>,
}

impl GateContext {
    /// new.
    pub fn new(
        worktree_dir: PathBuf,
        project_root: PathBuf,
        default_branch: String,
        branch_version: String,
        timeout_secs: u64,
        changed_files: Vec<String>,
    ) -> Self {
        Self {
            worktree_dir,
            project_root,
            default_branch,
            branch_version,
            timeout_secs,
            changed_files,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
/// GateSideEffect.
pub struct GateSideEffect {
    /// Struct.
    pub description: String,
    /// Struct.
    pub affected_files: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
/// GateResult.
pub struct GateResult {
    /// Struct.
    pub gate_name: String,
    /// Struct.
    pub passed: bool,
    /// Struct.
    pub message: String,
    /// Struct.
    pub details: Option<String>,
    /// Struct.
    pub side_effects: Vec<GateSideEffect>,
}

impl GateResult {
    /// pass.
    pub fn pass(name: impl Into<String>) -> Self {
        Self {
            gate_name: name.into(),
            passed: true,
            message: "passed".into(),
            details: None,
            side_effects: Vec::new(),
        }
    }
    /// pass_with_note.
    pub fn pass_with_note(name: impl Into<String>, note: impl Into<String>) -> Self {
        Self {
            gate_name: name.into(),
            passed: true,
            message: note.into(),
            details: None,
            side_effects: Vec::new(),
        }
    }
    /// pass_with_side_effects.
    pub fn pass_with_side_effects(
        name: impl Into<String>,
        note: impl Into<String>,
        details: impl Into<String>,
        side_effects: Vec<GateSideEffect>,
    ) -> Self {
        Self {
            gate_name: name.into(),
            passed: true,
            message: note.into(),
            details: Some(details.into()),
            side_effects,
        }
    }
    /// fail.
    pub fn fail(name: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            gate_name: name.into(),
            passed: false,
            message: message.into(),
            details: None,
            side_effects: Vec::new(),
        }
    }
    /// fail_with_details.
    pub fn fail_with_details(
        name: impl Into<String>,
        message: impl Into<String>,
        details: impl Into<String>,
    ) -> Self {
        Self {
            gate_name: name.into(),
            passed: false,
            message: message.into(),
            details: Some(details.into()),
            side_effects: Vec::new(),
        }
    }
    /// has_side_effects.
    pub fn has_side_effects(&self) -> bool {
        !self.side_effects.is_empty()
    }
}

#[derive(Debug, Clone)]
/// PipelineResult.
pub struct PipelineResult {
    /// Struct.
    pub gates: Vec<GateResult>,
    /// Struct.
    pub passed: bool,
}

impl PipelineResult {
    /// from_gates.
    pub fn from_gates(gates: Vec<GateResult>) -> Self {
        let passed = gates.iter().all(|g| g.passed);
        Self { gates, passed }
    }
    /// first_failure.
    pub fn first_failure(&self) -> Option<&GateResult> {
        self.gates.iter().find(|g| !g.passed)
    }
    /// failure_message.
    pub fn failure_message(&self) -> String {
        if let Some(fail) = self.first_failure() {
            let mut msg = format!("**{} failed**: {}", fail.gate_name, fail.message);
            if let Some(details) = &fail.details {
                msg = format!("{msg}\n\n```\n{details}\n```");
            }
            let side_effects: Vec<&GateSideEffect> = self
                .gates
                .iter()
                .filter(|g| g.passed && g.has_side_effects())
                .flat_map(|g| &g.side_effects)
                .collect();
            if !side_effects.is_empty() {
                msg.push_str("\n\n**Note: auto-fixes were applied before this failure:**\n");
                for se in side_effects {
                    msg.push_str(&format!("- {}\n", se.description));
                    if !se.affected_files.is_empty() {
                        msg.push_str(&format!("  Files: {}\n", se.affected_files.join(", ")));
                    }
                }
            }
            msg
        } else {
            String::new()
        }
    }
}
