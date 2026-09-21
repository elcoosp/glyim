use crate::error::PilotError;
use crate::gates::types::GateContext;
use crate::gates::{Gate, GateResult};
use crate::git_ops::diff_main;
use async_trait::async_trait;

/// ContractGate.
pub struct ContractGate;

#[async_trait]
impl Gate for ContractGate {
    fn name(&self) -> &str {
        "contracts"
    }

    async fn run(&self, ctx: &GateContext) -> Result<GateResult, PilotError> {
        let contracts_path = ctx.project_root.join("CONTRACTS_LOCKED.md");
        if !contracts_path.exists() {
            return Ok(GateResult::pass_with_note(
                "contracts",
                "no CONTRACTS_LOCKED.md found",
            ));
        }

        let content = tokio::fs::read_to_string(&contracts_path)
            .await
            .map_err(|e| PilotError::Gate {
                gate: "contracts".into(),
                message: format!("failed to read CONTRACTS_LOCKED.md: {e}"),
            })?;

        let locked_names = extract_locked_names(&content);
        if locked_names.is_empty() {
            return Ok(GateResult::pass("contracts"));
        }

        let diff = diff_main(&ctx.worktree_dir, &ctx.default_branch, ctx.timeout_secs).await?;
        if diff.is_empty() {
            return Ok(GateResult::pass("contracts"));
        }

        let mut violations = Vec::new();
        for line in diff.lines() {
            if line.starts_with('-') && !line.starts_with("---") {
                for name in &locked_names {
                    if line.contains(name.as_str()) {
                        violations.push(format!(
                            "locked interface '{}' in removed line: {}",
                            name,
                            line.trim_start_matches('-').trim()
                        ));
                    }
                }
            }
        }

        if violations.is_empty() {
            Ok(GateResult::pass("contracts"))
        } else {
            Ok(GateResult::fail_with_details(
                "contracts",
                format!("{} locked interface(s) removed", violations.len()),
                violations.join("\n"),
            ))
        }
    }
}

fn extract_locked_names(content: &str) -> Vec<String> {
    let mut names = Vec::new();
    let mut in_code = false;
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("```") {
            in_code = !in_code;
            continue;
        }
        if !in_code {
            continue;
        }
        if let Some(name) = extract_pub_name(trimmed) {
            names.push(name);
        }
    }
    names
}

fn extract_pub_name(line: &str) -> Option<String> {
    // Remove any leading whitespace
    let trimmed = line.trim();
    let after_pub = if let Some(r) = trimmed.strip_prefix("pub async fn ") {
        ("fn", r)
    } else if let Some(r) = trimmed.strip_prefix("pub const fn ") {
        ("fn", r)
    } else if let Some(r) = trimmed.strip_prefix("pub(crate) async fn ") {
        ("fn", r)
    } else if let Some(r) = trimmed.strip_prefix("pub(crate) fn ") {
        ("fn", r)
    } else if let Some(r) = trimmed.strip_prefix("pub fn ") {
        ("fn", r)
    } else if let Some(r) = trimmed.strip_prefix("pub struct ") {
        ("struct", r)
    } else if let Some(r) = trimmed.strip_prefix("pub enum ") {
        ("enum", r)
    } else {
        let r = trimmed.strip_prefix("pub trait ")?;
        ("trait", r)
    };

    match after_pub.0 {
        "fn" => after_pub.1.split('(').next().map(|s| s.trim().to_string()),
        "struct" | "enum" => after_pub
            .1
            .split(['<', '{', ' ', ';'])
            .next()
            .map(|s| s.trim().to_string()),
        "trait" => after_pub
            .1
            .split(['<', '{', ':'])
            .next()
            .map(|s| s.trim().to_string()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_locked_names_from_markdown() {
        let content = r#"
## Locked Contracts

```rust
pub fn parse_input() -> Result<()>;
pub struct Config { ... }
```
"#;
        let names = extract_locked_names(content);
        assert!(names.contains(&"parse_input".to_string()));
        assert!(names.contains(&"Config".to_string()));
    }

    #[test]
    fn test_extract_pub_name() {
        assert_eq!(
            extract_pub_name("pub fn foo() -> i32"),
            Some("foo".to_string())
        );
        assert_eq!(
            extract_pub_name("pub struct Bar<T>"),
            Some("Bar".to_string())
        );
        assert_eq!(extract_pub_name("pub enum Baz"), Some("Baz".to_string()));
        assert_eq!(extract_pub_name("pub trait Qux"), Some("Qux".to_string()));
        assert_eq!(
            extract_pub_name("pub async fn async_func()"),
            Some("async_func".to_string())
        );
        assert_eq!(extract_pub_name("fn not_pub()"), None);
    }
}
