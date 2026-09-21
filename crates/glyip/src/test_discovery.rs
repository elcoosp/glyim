//! Test function discovery within Glyim source files.
//!
//! Scans `.g` source files for test function declarations using simple
//! text-based pattern matching. Supports `#[test]` attribute detection
//! and `fn test_*` naming conventions.

use std::path::{Path, PathBuf};

/// Exact comment forms that mark the following `#[test]` function as ignored.
///
/// Glyim source does not parse `#[...]` attribute syntax, so `#[ignore]` is
/// expressed as a comment. These are matched by exact (case-insensitive)
/// equality so that ordinary prose comments mentioning "ignore" do not
/// accidentally mark a test ignored.
const IGNORE_MARKERS: &[&str] = &["// #[ignore]", "//#[ignore]", "// ignore", "//#ignore"];

/// A single discovered test function.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveredTest {
    /// Function name (e.g. `test_foo`).
    pub name: String,
    /// Source file containing the test.
    pub file: PathBuf,
    /// 1-based line number where the function is declared.
    pub line: usize,
    /// Whether the test is annotated with `#[ignore]` and should be skipped
    /// unless the caller explicitly opts into running ignored tests.
    pub ignored: bool,
}

/// Result of scanning a file for test functions.
#[derive(Debug, Clone)]
pub struct FileTestDiscovery {
    /// The source file that was scanned.
    pub file: PathBuf,
    /// Discovered test functions within the file.
    pub tests: Vec<DiscoveredTest>,
}

impl FileTestDiscovery {
    /// Scan a single source file for test functions.
    ///
    /// Detects two patterns:
    /// - `#[test]` attribute followed by `fn <name>(`
    /// - `fn test_<name>(` naming convention
    pub fn scan(file: &Path) -> crate::error::GlyipResult<Self> {
        let content = std::fs::read_to_string(file)?;
        let mut tests = Vec::new();
        let mut pending_test_attr = false;
        let mut pending_ignore_attr = false;

        for (line_idx, line) in content.lines().enumerate() {
            let line_number = line_idx.saturating_add(1);
            let trimmed = line.trim();

            // Detect #[test] / #[ignore] attributes on their own line or inline.
            if trimmed == "#[test]" {
                pending_test_attr = true;
                continue;
            }
            if trimmed == "#[ignore]" {
                pending_ignore_attr = true;
                continue;
            }
            // glyim source does not parse `#[...]` attributes, so an `#[ignore]`
            // marker must be written as a comment (`// #[ignore]`, `//#[ignore]`,
            // or `// ignore` on the line immediately preceding the function).
            // Use an exact allow-list rather than a substring search so ordinary
            // prose comments that merely mention "ignore" (e.g. `// we ignore
            // errors here`) do not accidentally mark a test as ignored.
            if IGNORE_MARKERS
                .iter()
                .any(|m| trimmed.eq_ignore_ascii_case(m))
            {
                pending_ignore_attr = true;
            } else if let Some(rest) = trimmed
                .strip_prefix("// #[ignore")
                .or_else(|| trimmed.strip_prefix("//#[ignore"))
            {
                // Accept the `#[ignore = "reason"]` form (mirrors real Rust's
                // `#[ignore = "reason"]` for glyim-level comments), so a
                // commented-out attributed ignore with a reason still counts.
                let rest = rest.trim_start();
                if rest.starts_with('=') || rest.starts_with(']') {
                    pending_ignore_attr = true;
                }
            }

            // Detect fn declaration.
            if let Some(fn_name) = extract_fn_name(trimmed) {
                let is_test_fn = pending_test_attr || fn_name.starts_with("test_");
                if is_test_fn {
                    tests.push(DiscoveredTest {
                        name: fn_name.to_string(),
                        file: file.to_path_buf(),
                        line: line_number,
                        ignored: pending_ignore_attr,
                    });
                }
                pending_test_attr = false;
                pending_ignore_attr = false;
            } else {
                // If there was a #[test]/#[ignore] but the next non-empty line
                // isn't a fn, keep looking (but reset for safety if it's clearly
                // not a fn).
                if !trimmed.is_empty() && !trimmed.starts_with('#') && !trimmed.starts_with("//") {
                    pending_test_attr = false;
                    pending_ignore_attr = false;
                }
            }
        }

        Ok(FileTestDiscovery {
            file: file.to_path_buf(),
            tests,
        })
    }

    /// Number of discovered tests in this file.
    pub fn count(&self) -> usize {
        self.tests.len()
    }
}

/// Extract the function name from a line like `fn foo(` or `pub fn foo(`.
fn extract_fn_name(line: &str) -> Option<&str> {
    // Strip modifiers like pub, async, unsafe, const, extern, etc.
    let mut line = line.trim_start();
    let prefixes = ["pub", "const", "unsafe", "async", "extern"];
    let mut changed = true;
    while changed {
        changed = false;
        for &prefix in &prefixes {
            if line.starts_with(prefix) && line[prefix.len()..].starts_with(' ') {
                line = line[prefix.len()..].trim_start();
                changed = true;
                break;
            }
        }
    }
    // Now find "fn "
    let after_fn = line.strip_prefix("fn ")?;
    let after_fn = after_fn.trim_start();

    let name_end = after_fn
        .find(|c: char| c == '(' || c == '<' || c == '{' || c.is_whitespace())
        .unwrap_or(after_fn.len());
    let name = &after_fn[..name_end];

    if name.is_empty() { None } else { Some(name) }
}
