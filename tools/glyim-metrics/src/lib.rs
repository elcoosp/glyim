//! M0.6 repo-health metrics: deterministic source-tree measurements.
//!
//! The crate measures cheap, stable properties of the workspace's Rust
//! sources — code lines, test functions, documented public items, and the
//! inline-allow count that the M0 zero-allow policy pins at zero. The
//! binary emits `metrics/baseline.json` and CI's ratchet job compares
//! every future run against it (`--check`). Deliberately no parsing: all
//! metrics are lexical, order-independent, and gold-testable on fixture
//! strings so the contract cannot drift with the parser.

use std::path::Path;

/// A snapshot of the workspace's measurable health properties.
///
/// Every field is a plain count so the JSON baseline is diffable and the
/// ratchet's comparisons are trivially explainable in a CI log.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RepoMetrics {
    /// Number of `.rs` files under the scanned root (excluding `target/`).
    pub rust_files: usize,
    /// Non-blank source lines across all scanned `.rs` files.
    pub code_lines: usize,
    /// Public item declarations (`pub fn|struct|enum|trait|mod|const|type|use`).
    pub public_items: usize,
    /// Public items whose declaration is immediately preceded (possibly
    /// across attributes) by at least one `///` doc line.
    pub documented_items: usize,
    /// Inline `#[allow(..)]` / `#[expect(..)]` attributes. The M0 policy
    /// pins this at zero; the ratchet fails CI if it ever rises.
    pub inline_allows: usize,
    /// Test functions (`#[test]` / `#[tokio::test]` attributes).
    pub test_functions: usize,
    /// Workspace member count from the root manifest (informational only;
    /// excluded from the ratchet's directions).
    pub workspace_members: usize,
}

impl RepoMetrics {
    /// Serialize to a JSON object with one key per metric.
    #[must_use]
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "rust_files": self.rust_files,
            "code_lines": self.code_lines,
            "public_items": self.public_items,
            "documented_items": self.documented_items,
            "inline_allows": self.inline_allows,
            "test_functions": self.test_functions,
            "workspace_members": self.workspace_members,
        })
    }

    /// Parse a metrics object produced by [`RepoMetrics::to_json`].
    ///
    /// # Errors
    ///
    /// Returns the serde error when the value is not an object with the
    /// seven expected integer fields.
    pub fn from_json(v: &serde_json::Value) -> Result<Self, serde_json::Error> {
        Ok(Self {
            rust_files: serde_json::from_value(v["rust_files"].clone())?,
            code_lines: serde_json::from_value(v["code_lines"].clone())?,
            public_items: serde_json::from_value(v["public_items"].clone())?,
            documented_items: serde_json::from_value(v["documented_items"].clone())?,
            inline_allows: serde_json::from_value(v["inline_allows"].clone())?,
            test_functions: serde_json::from_value(v["test_functions"].clone())?,
            workspace_members: serde_json::from_value(v["workspace_members"].clone())?,
        })
    }
}

/// Count non-blank lines: lines whose trimmed content is non-empty.
#[must_use]
pub fn count_code_lines(source: &str) -> usize {
    source.lines().filter(|l| !l.trim().is_empty()).count()
}

/// Count test attributes: each `#[test]` or `#[tokio::test]` occurrence that
/// is not inside a line comment or a doc comment.
#[must_use]
pub fn count_test_functions(source: &str) -> usize {
    let code = strip_line_comments(source);
    count_word_attr(&code, "#[test]") + count_word_attr(&code, "#[tokio::test]")
}

/// Count inline suppression attributes (`#[allow(` / `#[expect(`) outside
/// line comments. The M0 gate keeps this at zero for the whole workspace.
#[must_use]
pub fn count_inline_allows(source: &str) -> usize {
    let code = strip_line_comments(source);
    count_word_attr(&code, "#[allow(") + count_word_attr(&code, "#[expect(")
}

/// Count public item declarations. Lexical approximation: occurrences of
/// `pub ` followed by an item keyword at a code position.
#[must_use]
pub fn count_public_items(source: &str) -> usize {
    let code = strip_line_comments(source);
    ITEM_KEYWORDS
        .iter()
        .map(|kw| count_word_attr(&code, &format!("pub {kw}")))
        .sum()
}

/// Count public items that carry a doc comment: a `///` line that is
/// (possibly across `#[...]` attribute lines) immediately followed by a
/// `pub <item>` declaration line.
#[must_use]
pub fn count_documented_items(source: &str) -> usize {
    // Operate on raw lines: `///` doc lines ARE the signal here, so they must
    // survive (unlike the other counters, which strip comments first).
    let lines: Vec<&str> = source.lines().collect();
    let mut count = 0;
    for (i, line) in lines.iter().enumerate() {
        let t = line.trim_start();
        if !t.starts_with("pub ") || !is_item_line(t) {
            continue;
        }
        // Walk upwards over attributes; a `///` line must sit directly above
        // the attribute run (or the item itself).
        let mut j = i;
        let mut saw_doc = false;
        while j > 0 {
            let prev = lines[j - 1].trim_start();
            if prev.starts_with("#[") {
                j -= 1;
            } else {
                saw_doc = prev.starts_with("///");
                break;
            }
        }
        if saw_doc {
            count += 1;
        }
    }
    count
}

/// Measure a source tree: every `.rs` file outside `target/` contributes to
/// the text metrics; `workspace_members` counts `[workspace.members]`? No —
/// it counts top-level member directories declared in the root `Cargo.toml`
/// (string-match on quoted paths under `members = [ ... ]`).
///
/// `root` should be the workspace root; the walk is deterministic (sorted
/// directory traversal) so repeated runs on an unchanged tree agree.
#[must_use]
pub fn scan_tree(root: &Path) -> RepoMetrics {
    let mut rust_files = 0usize;
    let mut code_lines = 0usize;
    let mut public_items = 0usize;
    let mut documented_items = 0usize;
    let mut inline_allows = 0usize;
    let mut test_functions = 0usize;
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        let mut entries: Vec<_> = entries.filter_map(Result::ok).collect();
        entries.sort_by_key(std::fs::DirEntry::file_name);
        for e in entries {
            let path = e.path();
            let name = e.file_name();
            let lossy = name.to_string_lossy();
            if path.is_dir() {
                if lossy == "target" || lossy.starts_with('.') {
                    continue;
                }
                stack.push(path);
            } else if lossy.ends_with(".rs") {
                rust_files += 1;
                if let Ok(src) = std::fs::read_to_string(&path) {
                    code_lines += count_code_lines(&src);
                    public_items += count_public_items(&src);
                    documented_items += count_documented_items(&src);
                    inline_allows += count_inline_allows(&src);
                    test_functions += count_test_functions(&src);
                }
            }
        }
    }
    RepoMetrics {
        rust_files,
        code_lines,
        public_items,
        documented_items,
        inline_allows,
        test_functions,
        workspace_members: count_workspace_members(root),
    }
}

/// Count quoted member paths in the root manifest's `members = [ ... ]`
/// table. Informational only.
#[must_use]
pub fn count_workspace_members(root: &Path) -> usize {
    let Ok(manifest) = std::fs::read_to_string(root.join("Cargo.toml")) else {
        return 0;
    };
    let Some(start) = manifest.find("members = [") else {
        return 0;
    };
    let rest = &manifest[start + "members = [".len()..];
    let Some(end) = rest.find(']') else {
        return 0;
    };
    rest[..end].matches('"').count() / 2
}

/// One metric regression against the baseline.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Regression {
    /// Metric name (matches the JSON key).
    pub metric: &'static str,
    /// Baseline value.
    pub baseline: usize,
    /// Current value.
    pub current: usize,
}

/// The M0.6 ratchet: compare current metrics against the baseline.
#[derive(Debug, Clone, Copy)]
pub struct Ratchet;

impl Ratchet {
    /// Compare `current` against `baseline`.
    ///
    /// Directions per metric:
    /// - higher-is-better (regression on decrease): `rust_files`,
    ///   `code_lines`, `public_items`, `documented_items`,
    ///   `test_functions`
    /// - lower-is-better (regression on increase): `inline_allows`
    /// - informational (never regresses): `workspace_members`
    #[must_use]
    pub fn check(current: &RepoMetrics, baseline: &RepoMetrics) -> Vec<Regression> {
        let mut out = Vec::new();
        let higher: [(&'static str, usize, usize); 5] = [
            ("rust_files", current.rust_files, baseline.rust_files),
            ("code_lines", current.code_lines, baseline.code_lines),
            ("public_items", current.public_items, baseline.public_items),
            (
                "documented_items",
                current.documented_items,
                baseline.documented_items,
            ),
            (
                "test_functions",
                current.test_functions,
                baseline.test_functions,
            ),
        ];
        for (metric, cur, base) in higher {
            if cur < base {
                out.push(Regression {
                    metric,
                    baseline: base,
                    current: cur,
                });
            }
        }
        if current.inline_allows > baseline.inline_allows {
            out.push(Regression {
                metric: "inline_allows",
                baseline: baseline.inline_allows,
                current: current.inline_allows,
            });
        }
        out
    }
}

const ITEM_KEYWORDS: [&str; 8] = [
    "fn", "struct", "enum", "trait", "mod", "const", "type", "use",
];

fn is_item_line(trimmed: &str) -> bool {
    let rest = trimmed.strip_prefix("pub ").unwrap_or(trimmed);
    // skip `pub(crate)`-style qualified visibility
    let rest = if rest.starts_with('(') {
        match rest.find(')') {
            Some(i) => rest[i + 1..].trim_start(),
            None => return false,
        }
    } else {
        rest
    };
    ITEM_KEYWORDS.iter().any(|kw| {
        rest.starts_with(kw)
            && rest
                .get(kw.len()..)
                .is_some_and(|after| after.starts_with(|c: char| c.is_whitespace() || c == '{'))
    })
}

/// Remove `//` line comments (including `///` doc lines) so prose mentions
/// of attributes are not counted. String literals are left untouched: the
/// counted markers (`#[allow(` etc.) never legitimately appear inside a
/// string literal in this workspace's sources.
fn strip_line_comments(source: &str) -> String {
    let mut out = String::with_capacity(source.len());
    for line in source.lines() {
        match line.find("//") {
            Some(i) => out.push_str(&line[..i]),
            None => out.push_str(line),
        }
        out.push('\n');
    }
    out
}

/// Count occurrences of `needle` in `code` where the character before the
/// occurrence is line-start-ish (only whitespace between the line start and
/// the needle), so prose embeddings do not match.
fn count_word_attr(code: &str, needle: &str) -> usize {
    let mut count = 0;
    for line in code.lines() {
        let t = line.trim_start();
        if t.starts_with(needle) {
            count += 1;
        }
    }
    count
}

#[cfg(test)]
mod tests {
    use super::*;

    // ------------------------------------------------------------------
    // count_code_lines
    // ------------------------------------------------------------------

    #[test]
    fn code_lines_counts_non_blank_lines_only() {
        assert_eq!(count_code_lines(""), 0);
        assert_eq!(count_code_lines("\n\n\n"), 0);
        assert_eq!(count_code_lines("   \n\t\n"), 0);
        assert_eq!(count_code_lines("fn main() {}\n"), 1);
        assert_eq!(count_code_lines("a\n\nb\n  \nc\n"), 3);
    }

    #[test]
    fn code_lines_without_trailing_newline_counts_last_line() {
        assert_eq!(count_code_lines("a\nb"), 2);
    }

    // ------------------------------------------------------------------
    // count_test_functions
    // ------------------------------------------------------------------

    #[test]
    fn test_functions_counts_test_and_tokio_test() {
        let src = "#[test]\nfn a() {}\n\n#[tokio::test]\nasync fn b() {}\n";
        assert_eq!(count_test_functions(src), 2);
    }

    #[test]
    fn test_functions_ignores_prose_mentions() {
        let src = "/// An `#[test]` helper doc.\nfn helper() {}\n";
        assert_eq!(count_test_functions(src), 0);
    }

    #[test]
    fn test_functions_counts_each_attribute_once() {
        let src = "#[test]\n#[should_panic]\nfn a() {}\n";
        assert_eq!(count_test_functions(src), 1);
    }

    // ------------------------------------------------------------------
    // count_inline_allows — the zero-allow policy's measurement
    // ------------------------------------------------------------------

    #[test]
    fn inline_allows_counts_allow_and_expect_attributes() {
        let src = "#[allow(dead_code)]\nfn a() {}\n#[expect(unused)]\nfn b() {}\n";
        assert_eq!(count_inline_allows(src), 2);
    }

    #[test]
    fn inline_allows_ignores_doc_comment_mentions() {
        let src = "/// Never write `#[allow(..)]` inline.\nfn a() {}\n";
        assert_eq!(count_inline_allows(src), 0);
    }

    #[test]
    fn inline_allows_ignores_manifest_style_lines_in_rs() {
        // A lint *name* in prose or a cfg_attr in a comment must not count.
        let src = "// the #[allow] here is inside a line comment\nfn a() {}\n";
        assert_eq!(count_inline_allows(src), 0);
    }

    // ------------------------------------------------------------------
    // count_public_items / count_documented_items
    // ------------------------------------------------------------------

    #[test]
    fn public_items_counts_item_keywords() {
        let src = concat!(
            "pub fn a() {}\n",
            "pub struct S;\n",
            "pub enum E { A }\n",
            "pub trait T {}\n",
            "pub mod m {}\n",
            "pub const C: u8 = 0;\n",
            "pub type Alias = u8;\n",
            "pub use crate::x;\n",
            "fn private() {}\n",
        );
        assert_eq!(count_public_items(src), 8);
    }

    #[test]
    fn documented_items_counts_doc_preceded_public_items() {
        let src = concat!("/// Documented.\npub fn a() {}\n", "pub fn b() {}\n");
        assert_eq!(count_public_items(src), 2);
        assert_eq!(count_documented_items(src), 1);
    }

    #[test]
    fn documented_items_requires_adjacent_doc_line() {
        // A doc comment separated by a blank line does NOT document the item.
        let src = "/// Detached.\n\npub fn a() {}\n";
        assert_eq!(count_documented_items(src), 0);
    }

    #[test]
    fn documented_items_skips_attribute_lines_between_doc_and_item() {
        let src = "/// Documented.\n#[must_use]\npub fn a() {}\n";
        assert_eq!(count_documented_items(src), 1);
    }

    // ------------------------------------------------------------------
    // RepoMetrics JSON round-trip
    // ------------------------------------------------------------------

    #[test]
    fn metrics_json_round_trips() {
        let m = RepoMetrics {
            rust_files: 3,
            code_lines: 100,
            public_items: 10,
            documented_items: 8,
            inline_allows: 0,
            test_functions: 5,
            workspace_members: 36,
        };
        let v = m.to_json();
        assert_eq!(v["inline_allows"], serde_json::json!(0));
        let back = RepoMetrics::from_json(&v).expect("round trip");
        assert_eq!(back, m);
    }

    // ------------------------------------------------------------------
    // Ratchet
    // ------------------------------------------------------------------

    #[test]
    fn ratchet_accepts_improvements_and_equal_values() {
        let base = RepoMetrics {
            rust_files: 10,
            code_lines: 1000,
            public_items: 100,
            documented_items: 90,
            inline_allows: 0,
            test_functions: 50,
            workspace_members: 36,
        };
        let better = RepoMetrics {
            rust_files: 11,
            code_lines: 1100,
            public_items: 101,
            documented_items: 95,
            inline_allows: 0,
            test_functions: 55,
            workspace_members: 36,
        };
        assert_eq!(Ratchet::check(&better, &base), [] as [Regression; 0]);
    }

    #[test]
    fn ratchet_flags_lower_is_better_regression() {
        let base = RepoMetrics {
            rust_files: 10,
            code_lines: 1000,
            public_items: 100,
            documented_items: 90,
            inline_allows: 2,
            test_functions: 50,
            workspace_members: 36,
        };
        let worse = RepoMetrics {
            inline_allows: 3,
            ..base
        };
        let regressions = Ratchet::check(&worse, &base);
        assert_eq!(regressions.len(), 1);
        assert_eq!(regressions[0].metric, "inline_allows");
        assert_eq!(regressions[0].baseline, 2);
        assert_eq!(regressions[0].current, 3);
    }

    #[test]
    fn ratchet_flags_higher_is_better_regression() {
        let base = RepoMetrics {
            rust_files: 10,
            code_lines: 1000,
            public_items: 100,
            documented_items: 90,
            inline_allows: 0,
            test_functions: 50,
            workspace_members: 36,
        };
        let worse = RepoMetrics {
            code_lines: 900,
            test_functions: 40,
            ..base
        };
        let regressions = Ratchet::check(&worse, &base);
        assert_eq!(regressions.len(), 2);
        let names: Vec<&str> = regressions.iter().map(|r| r.metric).collect();
        assert!(names.contains(&"code_lines"));
        assert!(names.contains(&"test_functions"));
    }

    #[test]
    fn ratchet_allows_workspace_members_to_grow_or_shrink() {
        // Members is informational: neither direction is a regression.
        let base = RepoMetrics {
            rust_files: 10,
            code_lines: 1000,
            public_items: 100,
            documented_items: 90,
            inline_allows: 0,
            test_functions: 50,
            workspace_members: 36,
        };
        let grown = RepoMetrics {
            workspace_members: 37,
            ..base
        };
        let shrunk = RepoMetrics {
            workspace_members: 35,
            ..base
        };
        assert_eq!(Ratchet::check(&grown, &base), [] as [Regression; 0]);
        assert_eq!(Ratchet::check(&shrunk, &base), [] as [Regression; 0]);
    }

    // ------------------------------------------------------------------
    // scan_tree on a synthetic tree
    // ------------------------------------------------------------------

    #[test]
    fn scan_tree_measures_a_synthetic_workspace() {
        let root = std::env::temp_dir().join(format!("glyim-metrics-test-{}", std::process::id()));
        let src_a = root.join("src");
        std::fs::create_dir_all(&src_a).unwrap();
        std::fs::write(root.join("Cargo.toml"), "[package]\nname = \"fake\"\n").unwrap();
        std::fs::write(
            src_a.join("lib.rs"),
            "//! Crate doc.\n/// Item doc.\npub fn a() -> u8 { 1 }\n#[test]\nfn t() {}\n",
        )
        .unwrap();
        // A non-rust file and a target/ dir must be skipped.
        std::fs::write(root.join("notes.md"), "pub fn not_rust() {}\n").unwrap();
        std::fs::create_dir_all(root.join("target")).unwrap();
        std::fs::write(
            root.join("target").join("junk.rs"),
            "#[allow(dead_code)]\nfn junk() {}\n",
        )
        .unwrap();

        let m = scan_tree(&root);
        assert_eq!(m.rust_files, 1);
        assert_eq!(m.public_items, 1);
        assert_eq!(m.documented_items, 1);
        assert_eq!(m.test_functions, 1);
        assert_eq!(m.inline_allows, 0);

        std::fs::remove_dir_all(&root).ok();
    }
}
