//! Architecture dependency gate.
//! Checks changed Cargo.toml files for forbidden dependencies.

use crate::domain_types::{default_architecture_rules, DependencyRule};
use crate::error::PilotError;
use crate::gates::types::GateContext;
use crate::gates::{Gate, GateResult};
use async_trait::async_trait;

/// ArchitectureGate.
pub struct ArchitectureGate {
    rules: Vec<DependencyRule>,
}

impl ArchitectureGate {
    /// new.
    pub fn new(rules: Vec<DependencyRule>) -> Self {
        Self {
            rules: if rules.is_empty() {
                default_architecture_rules()
            } else {
                rules
            },
        }
    }
    /// with_default_rules.
    pub fn with_default_rules() -> Self {
        Self::new(default_architecture_rules())
    }
}
impl Default for ArchitectureGate {
    fn default() -> Self {
        Self::with_default_rules()
    }
}

#[async_trait]
impl Gate for ArchitectureGate {
    fn name(&self) -> &str {
        "architecture"
    }

    async fn run(&self, ctx: &GateContext) -> Result<GateResult, PilotError> {
        let changed_files = ctx.changed_files.clone();
        let use_diff = !changed_files.is_empty();
        let dir = ctx.worktree_dir.clone();
        let rules = self.rules.clone();

        let result = tokio::task::spawn_blocking(move || {
            let mut violations = Vec::new();

            if use_diff {
                for rel_path in &changed_files {
                    if !rel_path.ends_with("Cargo.toml") {
                        continue;
                    }
                    let path = dir.join(rel_path);
                    if let Ok(content) = std::fs::read_to_string(&path) {
                        check_cargo_toml(&content, rel_path, &rules, &mut violations);
                    }
                }
            } else {
                let walker = ignore::WalkBuilder::new(&dir).hidden(false).build();
                for entry in walker.flatten() {
                    let path = entry.path();
                    if path.file_name().is_some_and(|n| n == "Cargo.toml") {
                        let rel = path.strip_prefix(&dir).unwrap_or(path);
                        let rel_str = rel.to_string_lossy();
                        if let Ok(content) = std::fs::read_to_string(path) {
                            check_cargo_toml(&content, &rel_str, &rules, &mut violations);
                        }
                    }
                }
            }

            if violations.is_empty() {
                GateResult::pass("architecture")
            } else {
                GateResult::fail_with_details(
                    "architecture",
                    format!("{} violation(s)", violations.len()),
                    violations.join("\n"),
                )
            }
        })
        .await
        .map_err(|e| PilotError::Gate {
            gate: "architecture".into(),
            message: format!("spawn_blocking: {e}"),
        })?;

        Ok(result)
    }
}

fn check_cargo_toml(
    content: &str,
    rel_path: &str,
    rules: &[DependencyRule],
    violations: &mut Vec<String>,
) {
    let crate_name = extract_crate_name(content);
    let deps = extract_dependencies(content);
    if let Some(name) = crate_name {
        for rule in rules {
            if name == rule.from_crate && deps.contains(&rule.forbidden_dep) {
                violations.push(format!(
                    "{}: {} depends on {} – {}",
                    rel_path, rule.from_crate, rule.forbidden_dep, rule.reason
                ));
            }
        }
    }
}

fn extract_crate_name(content: &str) -> Option<String> {
    let value: toml::Value = content.parse().ok()?;
    value
        .get("package")?
        .get("name")?
        .as_str()
        .map(|s| s.to_string())
}

fn extract_dependencies(content: &str) -> Vec<String> {
    let value: toml::Value = match content.parse() {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };
    let mut deps = Vec::new();
    if let Some(table) = value.get("dependencies").and_then(|v| v.as_table()) {
        for key in table.keys() {
            deps.push(key.clone());
        }
    }
    if let Some(table) = value.get("dev-dependencies").and_then(|v| v.as_table()) {
        for key in table.keys() {
            if !deps.contains(key) {
                deps.push(key.clone());
            }
        }
    }
    deps
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_crate_name() {
        let toml = r#"
[package]
name = "glyim-frontend"
version = "0.1.0"
"#;
        assert_eq!(extract_crate_name(toml), Some("glyim-frontend".to_string()));
    }

    #[test]
    fn test_extract_dependencies() {
        let toml = r#"
[dependencies]
glyim-ir = { path = "../ir" }
serde = "1"

[dev-dependencies]
tempfile = "3"
"#;
        let deps = extract_dependencies(toml);
        assert!(deps.contains(&"glyim-ir".to_string()));
        assert!(deps.contains(&"serde".to_string()));
        assert!(deps.contains(&"tempfile".to_string()));
    }

    #[test]
    fn test_architecture_rule_violation() {
        let rules = vec![DependencyRule {
            from_crate: "frontend".into(),
            forbidden_dep: "backend".into(),
            reason: "layering".into(),
        }];
        let mut violations = Vec::new();
        let cargo_content = r#"
[package]
name = "frontend"
[dependencies]
backend = { path = "../backend" }
"#;
        check_cargo_toml(cargo_content, "Cargo.toml", &rules, &mut violations);
        assert_eq!(violations.len(), 1);
        assert!(violations[0].contains("frontend depends on backend"));
    }
}
