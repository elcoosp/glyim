//! Banned pattern gate with full string literal stripping (raw strings, byte strings, etc.)

use crate::domain_types::{default_banned_patterns, BannedPattern};
use crate::error::PilotError;
use crate::gates::types::GateContext;
use crate::gates::{Gate, GateResult};
use async_trait::async_trait;

/// BannedPatternGate.
pub struct BannedPatternGate {
    patterns: Vec<BannedPattern>,
}

impl BannedPatternGate {
    /// new.
    pub fn new(patterns: Vec<BannedPattern>) -> Self {
        Self {
            patterns: if patterns.is_empty() {
                default_banned_patterns()
            } else {
                patterns
            },
        }
    }
    /// with_defaults.
    pub fn with_defaults() -> Self {
        Self::new(default_banned_patterns())
    }
}
impl Default for BannedPatternGate {
    fn default() -> Self {
        Self::with_defaults()
    }
}

#[async_trait]
impl Gate for BannedPatternGate {
    fn name(&self) -> &str {
        "banned_patterns"
    }

    async fn run(&self, ctx: &GateContext) -> Result<GateResult, PilotError> {
        let changed_files = ctx.changed_files.clone();
        let use_diff = !changed_files.is_empty();
        let dir = ctx.worktree_dir.clone();
        let patterns = self.patterns.clone();

        let result = tokio::task::spawn_blocking(move || {
            let mut violations = Vec::new();

            if use_diff {
                for rel_path in &changed_files {
                    if !rel_path.ends_with(".rs") {
                        continue;
                    }
                    if rel_path.contains("/tests/") || rel_path.contains("\\tests\\") {
                        continue;
                    }
                    let path = dir.join(rel_path);
                    if let Ok(content) = std::fs::read_to_string(&path) {
                        check_content_for_violations(
                            &content,
                            rel_path,
                            &patterns,
                            &mut violations,
                        );
                    }
                }
            } else {
                let walker = ignore::WalkBuilder::new(&dir).hidden(false).build();
                for entry in walker.flatten() {
                    let path = entry.path();
                    if path.extension().is_some_and(|e| e == "rs") {
                        let path_str = path.to_string_lossy();
                        if path_str.contains("/tests/") || path_str.contains("\\tests\\") {
                            continue;
                        }
                        let rel = path.strip_prefix(&dir).unwrap_or(path);
                        let rel_str = rel.to_string_lossy().to_string();
                        if let Ok(content) = std::fs::read_to_string(path) {
                            check_content_for_violations(
                                &content,
                                &rel_str,
                                &patterns,
                                &mut violations,
                            );
                        }
                    }
                }
            }

            if violations.is_empty() {
                GateResult::pass("banned_patterns")
            } else {
                GateResult::fail_with_details(
                    "banned_patterns",
                    format!("{} banned pattern(s) found", violations.len()),
                    violations.join("\n"),
                )
            }
        })
        .await
        .map_err(|e| PilotError::Gate {
            gate: "banned_patterns".into(),
            message: format!("spawn_blocking: {e}"),
        })?;

        Ok(result)
    }
}

fn check_content_for_violations(
    content: &str,
    rel_path: &str,
    patterns: &[BannedPattern],
    violations: &mut Vec<String>,
) {
    for (i, line) in content.lines().enumerate() {
        if line.trim().starts_with("//") {
            continue;
        }
        let stripped = strip_string_literals(line);
        for pat in patterns {
            if stripped.contains(&pat.pattern) {
                violations.push(format!("{}:{}: {}", rel_path, i + 1, pat.description));
            }
        }
    }
}

fn strip_string_literals(line: &str) -> String {
    let mut result = String::with_capacity(line.len());
    let mut chars = line.chars().peekable();

    'outer: while let Some(c) = chars.next() {
        if c == '/' && chars.peek() == Some(&'/') {
            break;
        }
        if c == 'b' {
            let next = chars.peek().copied();
            if next == Some('"') {
                chars.next();
                while let Some(nc) = chars.next() {
                    if nc == '\\' {
                        chars.next();
                        continue;
                    }
                    if nc == '"' {
                        continue 'outer;
                    }
                }
                continue;
            }
            if next == Some('r') {
                chars.next();
                consume_raw_string(&mut chars);
                continue;
            }
            result.push(c);
            continue;
        }
        if c == 'r' {
            let next = chars.peek().copied();
            if next == Some('"') || next == Some('#') {
                consume_raw_string(&mut chars);
                continue;
            }
            result.push(c);
            continue;
        }
        if c == '"' {
            while let Some(nc) = chars.next() {
                if nc == '\\' {
                    chars.next();
                    continue;
                }
                if nc == '"' {
                    continue 'outer;
                }
            }
            continue;
        }
        if c == '\'' {
            while let Some(nc) = chars.next() {
                if nc == '\\' {
                    chars.next();
                    continue;
                }
                if nc == '\'' {
                    continue 'outer;
                }
            }
            continue;
        }
        result.push(c);
    }
    result
}

fn consume_raw_string(chars: &mut std::iter::Peekable<std::str::Chars<'_>>) {
    let mut hash_count = 0usize;
    while chars.peek() == Some(&'#') {
        chars.next();
        hash_count += 1;
    }
    if chars.peek() != Some(&'"') {
        return;
    }
    chars.next(); // consume opening "
    if hash_count == 0 {
        // Simple raw string without hash: skip until the next unescaped "
        while let Some(nc) = chars.next() {
            if nc == '\\' {
                chars.next();
                continue;
            }
            if nc == '"' {
                break;
            }
        }
    } else {
        let mut prev_quote = false;
        let mut matched = 0;
        loop {
            match chars.next() {
                None => break,
                Some('"') => {
                    prev_quote = true;
                    matched = 0;
                }
                Some('#') if prev_quote => {
                    matched += 1;
                    if matched == hash_count {
                        break;
                    }
                }
                Some(_) => {
                    prev_quote = false;
                    matched = 0;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_strip_regular_string() {
        assert_eq!(
            strip_string_literals(r#"let x = "hello { world }"; let y = foo;"#),
            "let x = ; let y = foo;"
        );
    }

    #[test]
    fn test_strip_raw_string() {
        let input = r#"let s = r"contains { braces }"; let y = foo;"#;
        let stripped = strip_string_literals(input);
        assert!(stripped.contains("let s = "));
        assert!(stripped.contains("let y = foo;"));
        assert!(!stripped.contains("contains { braces }"));
    }

    #[test]
    fn test_strip_raw_string_hash() {
        let input = "let s = r#\"contains { braces }\"#; let y = foo;";
        let stripped = strip_string_literals(input);
        assert!(stripped.contains("let s = "));
        assert!(stripped.contains("let y = foo;"));
        assert!(!stripped.contains("contains { braces }"));
    }

    #[test]
    fn test_strip_byte_string() {
        let input = r#"let b = b"hello {"; let y = foo;"#;
        assert_eq!(strip_string_literals(input), "let b = ; let y = foo;");
    }

    #[test]
    fn test_strip_byte_raw_string() {
        let input = "let b = br#\"contains { braces }\"#; let y = foo;";
        let stripped = strip_string_literals(input);
        assert!(stripped.contains("let b = "));
        assert!(stripped.contains("let y = foo;"));
        assert!(!stripped.contains("contains { braces }"));
    }

    #[test]
    fn test_banned_pattern_not_in_string_literal() {
        let line = r#"let msg = r"call .unwrap() here"; let y = z;"#;
        let stripped = strip_string_literals(line);
        assert!(!stripped.contains("unwrap"));
    }

    #[test]
    fn test_comments_skipped() {
        let content = "// todo!() should not be flagged\nfn main() {}";
        let patterns = default_banned_patterns();
        let mut violations = Vec::new();
        check_content_for_violations(content, "test.rs", &patterns, &mut violations);
        assert!(violations.is_empty());
    }

    #[test]
    fn test_banned_pattern_detection() {
        let patterns = vec![BannedPattern::new("unwrap", "no unwrap")];
        let mut violations = Vec::new();
        let content = "let x = unwrap(); // comment\nfn main() {}";
        check_content_for_violations(content, "test.rs", &patterns, &mut violations);
        assert_eq!(violations.len(), 1);
        assert!(violations[0].contains("test.rs:1:"));
    }

    #[test]
    fn test_banned_pattern_ignores_strings() {
        let patterns = vec![BannedPattern::new("unwrap", "no unwrap")];
        let mut violations = Vec::new();
        let content = r#"let msg = "unwrap is not called";"#;
        check_content_for_violations(content, "test.rs", &patterns, &mut violations);
        assert!(violations.is_empty());
    }
}
