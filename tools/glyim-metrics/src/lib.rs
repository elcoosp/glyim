//! M0.6 repo-health metrics: deterministic source-tree measurements.
//!
//! The crate measures cheap, stable properties of the workspace's Rust
//! sources — code lines, test functions, documented public items, the
//! inline-allow count that the M0 zero-allow policy pins at zero, and the
//! five panic/unsafe/IR-hygiene counters the full M0.6 ratchet set adds
//! (non-test `unwrap`/`expect` calls, `unsafe` items, `from_raw` call
//! sites, public fields on IR structs, `HashMap` sites in compiler crates).
//! The binary emits `metrics/baseline.json` and CI's ratchet jobs compare
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
    /// Non-test `.unwrap()` / `.expect(` call sites (plan M0.6 "unwrap/
    /// expect/panic counts, non-test"; the M3.5 panic ratchet burns this
    /// down). Test code — `#[cfg(test)]` regions, `#[test]` functions, and
    /// files under a `tests/` directory — is excluded lexically.
    pub unwrap_count: usize,
    /// Production `unsafe` items (`unsafe fn` / `unsafe impl` /
    /// `unsafe trait` / `unsafe {`), test regions excluded (plan M0.6
    /// "unsafe counts"; the M3.6 unsafe policy drives this to the
    /// allow-list).
    pub unsafe_count: usize,
    /// `from_raw` call sites across the workspace (plan M0.6 "`from_raw`
    /// call sites"; M3.1's CT-1 codemod drives this to zero outside arena
    /// modules). Exact `from_raw(` calls only — `from_raw_parts` and other
    /// suffixed names are different APIs and not counted.
    pub from_raw_count: usize,
    /// Public fields on public structs in the IR crates (`glyim-mir`,
    /// `glyim-hir`): the M5.4 "public-field count on IR structs = 0" ratchet
    /// measures exactly this.
    pub public_ir_fields: usize,
    /// `HashMap` type-usage sites in compiler crates (paths under
    /// `crates/`): the M3.9 determinism ratchet drives std-`HashMap` out of
    /// the compiler in favor of `FxIndexMap`/`FxHashMap`. Tools are not
    /// compiler crates and are not counted.
    pub hash_map_hot_sites: usize,
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
            "unwrap_count": self.unwrap_count,
            "unsafe_count": self.unsafe_count,
            "from_raw_count": self.from_raw_count,
            "public_IR_fields": self.public_ir_fields,
            "HashMap_hot_sites": self.hash_map_hot_sites,
        })
    }

    /// Parse a metrics object produced by [`RepoMetrics::to_json`].
    ///
    /// # Errors
    ///
    /// Returns the serde error when the value is not an object with the
    /// twelve expected integer fields.
    pub fn from_json(v: &serde_json::Value) -> Result<Self, serde_json::Error> {
        Ok(Self {
            rust_files: serde_json::from_value(v["rust_files"].clone())?,
            code_lines: serde_json::from_value(v["code_lines"].clone())?,
            public_items: serde_json::from_value(v["public_items"].clone())?,
            documented_items: serde_json::from_value(v["documented_items"].clone())?,
            inline_allows: serde_json::from_value(v["inline_allows"].clone())?,
            test_functions: serde_json::from_value(v["test_functions"].clone())?,
            workspace_members: serde_json::from_value(v["workspace_members"].clone())?,
            unwrap_count: serde_json::from_value(v["unwrap_count"].clone())?,
            unsafe_count: serde_json::from_value(v["unsafe_count"].clone())?,
            from_raw_count: serde_json::from_value(v["from_raw_count"].clone())?,
            public_ir_fields: serde_json::from_value(v["public_IR_fields"].clone())?,
            hash_map_hot_sites: serde_json::from_value(v["HashMap_hot_sites"].clone())?,
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
    let mut unwrap_count = 0usize;
    let mut unsafe_count = 0usize;
    let mut from_raw_count = 0usize;
    let mut public_ir_fields = 0usize;
    let mut hashmap_sites = 0usize;
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
                    // The non-test counters exclude test-only files
                    // entirely (`tests/` components); inside mixed files
                    // the counters strip `#[cfg(test)]` regions
                    // themselves.
                    if !is_test_file(&path) {
                        unwrap_count += count_unwrap_calls(&src);
                        unsafe_count += count_unsafe_items(&src);
                        from_raw_count += count_from_raw_calls(&src);
                        if is_compiler_crate_file(&path) {
                            hashmap_sites += count_hashmap_sites(&src);
                        }
                        if is_ir_file(&path) {
                            public_ir_fields += count_public_fields(&strip_test_regions(&src));
                        }
                    }
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
        unwrap_count,
        unsafe_count,
        from_raw_count,
        public_ir_fields,
        hash_map_hot_sites: hashmap_sites,
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

/// A file whose path carries a `tests` component is test-only (`tests/`
/// integration targets and the `src/tests/` unit-test layout this repo
/// uses): the non-test panic/unsafe ratchets exclude it entirely.
fn is_test_file(path: &Path) -> bool {
    path.components().any(|c| c.as_os_str() == "tests")
}

/// The IR crates whose public-struct fields the M5.4 ratchet counts.
fn is_ir_file(path: &Path) -> bool {
    path.components()
        .any(|c| c.as_os_str() == "glyim-mir" || c.as_os_str() == "glyim-hir")
}

/// Compiler crates for the `HashMap` determinism ratchet: everything under
/// `crates/`. Tools are infrastructure, not compiler hot paths.
fn is_compiler_crate_file(path: &Path) -> bool {
    path.components().any(|c| c.as_os_str() == "crates")
}

/// Net brace delta of one line, ignoring braces inside string literals so
/// format strings cannot unbalance the region tracking, plus the number of
/// braces OPENED on the line (a single-line `fn f() { .. }` has net zero
/// but did open a region). `in_string` persists across the lines of one
/// file (a string may span lines).
fn brace_delta(line: &str, in_string: &mut bool) -> (i32, i32) {
    let mut delta = 0;
    let mut opens = 0;
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        if *in_string {
            match c {
                '\\' => {
                    chars.next();
                }
                '"' => *in_string = false,
                _ => {}
            }
            continue;
        }
        match c {
            '"' => *in_string = true,
            '{' => {
                delta += 1;
                opens += 1;
            }
            '}' => delta -= 1,
            _ => {}
        }
    }
    (delta, opens)
}

/// Remove test-only regions from `source` so the non-test counters see
/// production code: `#[cfg(test)]`-gated items (`mod tests` and friends)
/// and `#[test]` / `#[tokio::test]`-annotated functions. Lexical, not a
/// parse: the region extent is the brace block opened by the gated
/// declaration, tracked string-aware, so the result is deterministic for a
/// given input and testable on fixture strings.
fn strip_test_regions(source: &str) -> String {
    let mut out = String::with_capacity(source.len());
    let mut in_string = false;
    let mut depth = 0i32;
    // When set, lines are inside a skipped region until brace depth returns
    // to the recorded value AND the region's opening brace was seen (a
    // multi-line declaration signature delays the opening brace).
    let mut skip_until: Option<i32> = None;
    let mut region_opened = false;
    // Saw `#[cfg(test)]` / `#[test]` on a previous line; the next
    // non-attribute line decides whether it gates a region.
    let mut pending_cfg_test = false;
    let mut pending_test_fn = false;

    for line in source.lines() {
        // `strip_line_comments` re-terminates each line with `\n`, so trim
        // BOTH ends here: a trailing `\n` would break `ends_with(';')`
        // below and leave a body-less `mod tests;` gating a phantom region
        // that never closes (silently dropping all code after it).
        let code = strip_line_comments(line);
        let trimmed = code.trim();

        if let Some(target) = skip_until {
            let (delta, opens) = brace_delta(&code, &mut in_string);
            depth += delta;
            if opens > 0 {
                region_opened = true;
            }
            if region_opened && depth <= target {
                skip_until = None;
                region_opened = false;
            }
            continue;
        }

        // Attribute/decorator triggers only fire when the line does not
        // start inside a string literal carried over from a previous line:
        // fixture templates in test-writer crates embed `#[test]`-shaped
        // text inside string constants, and treating that as a gate would
        // silently skip real production code below it.
        let at_code_start = !in_string;
        if at_code_start && trimmed.starts_with("#[cfg(test)]") {
            pending_cfg_test = true;
            continue;
        }
        if at_code_start
            && (trimmed.starts_with("#[test]") || trimmed.starts_with("#[tokio::test]"))
        {
            pending_test_fn = true;
            continue;
        }
        if trimmed.is_empty() {
            // Keep any pending gate across blank lines.
            continue;
        }
        if at_code_start && trimmed.starts_with("#[") {
            // Another attribute (e.g. `#[should_panic]` between `#[test]`
            // and the fn): pass through, keep the pending gate.
            out.push_str(line);
            out.push('\n');
            continue;
        }

        let decl = pending_cfg_test
            && (trimmed.starts_with("mod ")
                || trimmed.starts_with("pub mod ")
                || trimmed.starts_with("fn ")
                || trimmed.starts_with("async fn ")
                || trimmed.starts_with("pub fn ")
                || trimmed.starts_with("pub async fn "));
        let test_fn = pending_test_fn
            && (trimmed.starts_with("fn ")
                || trimmed.starts_with("async fn ")
                || trimmed.starts_with("pub fn ")
                || trimmed.starts_with("pub async fn "));
        pending_cfg_test = false;
        pending_test_fn = false;

        if (decl || test_fn) && trimmed.ends_with(';') {
            // Body-less gated declaration (`#[cfg(test)] mod tests;`): the
            // region is exactly this line.
            continue;
        }
        if decl || test_fn {
            skip_until = Some(depth);
            region_opened = false;
            let (delta, opens) = brace_delta(&code, &mut in_string);
            depth += delta;
            if opens > 0 {
                region_opened = true;
            }
            if region_opened && depth <= skip_until.unwrap_or(i32::MAX) {
                skip_until = None;
                region_opened = false;
            }
            continue;
        }

        out.push_str(line);
        out.push('\n');
        depth += brace_delta(&code, &mut in_string).0;
    }
    out
}

/// Count non-overlapping occurrences of `needle` in `code` where the
/// character before is not a word character (so `FxHashMap` does not match
/// `HashMap`), and — when the needle itself ends on a word character — the
/// character after is not a word character either (so `HashMapExt` does
/// not match either).
fn count_word(code: &str, needle: &str) -> usize {
    let ends_on_word = needle
        .chars()
        .next_back()
        .is_some_and(|c| c.is_alphanumeric() || c == '_');
    let mut count = 0;
    let mut start = 0;
    while let Some(pos) = code[start..].find(needle) {
        let abs = start + pos;
        let end = abs + needle.len();
        let before_ok = abs == 0
            || !code[..abs]
                .chars()
                .next_back()
                .is_some_and(|c| c.is_alphanumeric() || c == '_');
        let after_ok = !ends_on_word
            || code[end..]
                .chars()
                .next()
                .is_none_or(|c| !(c.is_alphanumeric() || c == '_'));
        if before_ok && after_ok {
            count += 1;
        }
        start = end;
    }
    count
}

/// Count non-test `.unwrap()` / `.expect(` call sites: test regions
/// (`#[cfg(test)]` items, `#[test]` functions, `tests/` files are handled
/// by the caller) are stripped first, then the two call shapes are counted
/// outside comments. `.unwrap_or(..)` and friends do not match.
#[must_use]
pub fn count_unwrap_calls(source: &str) -> usize {
    let code = strip_line_comments(&strip_test_regions(source));
    let unwraps = code.matches(".unwrap()").count();
    let expects = code.matches(".expect(").count();
    unwraps + expects
}

/// Count production `unsafe` items: `unsafe fn` / `unsafe impl` /
/// `unsafe trait` declarations and `unsafe {` blocks, outside comments and
/// test regions. The word-boundary prefix check keeps prose like
/// `is_unsafe` from matching.
#[must_use]
pub fn count_unsafe_items(source: &str) -> usize {
    let code = strip_line_comments(&strip_test_regions(source));
    ["unsafe fn", "unsafe impl", "unsafe trait", "unsafe {"]
        .iter()
        .map(|kw| count_word(&code, kw))
        .sum()
}

/// Count `from_raw(` call sites outside comments and test regions. Only
/// the exact name matches — `from_raw_parts` is a different API and is not
/// a CT-1 codemod site.
#[must_use]
pub fn count_from_raw_calls(source: &str) -> usize {
    let code = strip_line_comments(&strip_test_regions(source));
    let mut count = 0;
    let mut start = 0;
    while let Some(pos) = code[start..].find("from_raw") {
        let abs = start + pos;
        let end = abs + "from_raw".len();
        let before_ok = abs == 0
            || !code[..abs]
                .chars()
                .next_back()
                .is_some_and(|c| c.is_alphanumeric() || c == '_');
        let call = code[end..].trim_start().starts_with('(');
        if before_ok && call {
            count += 1;
        }
        start = end;
    }
    count
}

/// Count `pub ` fields on public structs in one source file. A struct
/// header (`pub struct`, also qualified `pub(crate)` / `pub(super)`) opens
/// a body at its `{`; every `pub`/`pub(crate)`/`pub(super)`-prefixed line
/// inside the body until it closes is a public field, and fields declared
/// on the header line itself count too (one-line structs). Lexical
/// approximation: a multi-line struct header (rare in this codebase) and
/// braces inside field types are not tracked, and both only ever
/// under-count.
#[must_use]
pub fn count_public_fields(source: &str) -> usize {
    let is_field_line = |t: &str| {
        t.starts_with("pub ") || t.starts_with("pub(crate) ") || t.starts_with("pub(super) ")
    };
    let code = strip_line_comments(source);
    let mut count = 0;
    let mut in_struct = false;
    let mut depth = 0i32;
    let mut in_string = false;
    for line in code.lines() {
        let t = line.trim_start();
        let (delta, _opens) = brace_delta(line, &mut in_string);
        if in_struct {
            depth += delta;
            if depth > 0 && is_field_line(t) {
                count += 1;
            }
            if depth <= 0 {
                in_struct = false;
                depth = 0;
            }
            continue;
        }
        let is_header = t.starts_with("pub struct")
            || t.starts_with("pub(crate) struct")
            || t.starts_with("pub(super) struct");
        if !is_header {
            continue;
        }
        let Some(open) = line.find('{') else {
            // Multi-line header: the body opens on a later line, which is
            // not tracked (under-count, never over-count).
            continue;
        };
        // Fields on the header line itself (`pub struct S { pub a: u8 }`).
        count += line[open..].matches("pub ").count();
        depth += delta;
        if depth > 0 {
            in_struct = true;
        }
    }
    count
}

/// Count `HashMap` type-usage sites (imports, type positions, method-less
/// type mentions) outside comments. `FxHashMap` and `HashMapExt`-style
/// names do not match (word-boundary prefix check).
#[must_use]
pub fn count_hashmap_sites(source: &str) -> usize {
    let code = strip_line_comments(source);
    count_word(&code, "HashMap")
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
    /// - lower-is-better (regression on increase): `inline_allows`,
    ///   `unwrap_count`, `unsafe_count`, `from_raw_count`,
    ///   `public_IR_fields`, `HashMap_hot_sites`
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
        let lower: [(&'static str, usize, usize); 6] = [
            (
                "inline_allows",
                current.inline_allows,
                baseline.inline_allows,
            ),
            ("unwrap_count", current.unwrap_count, baseline.unwrap_count),
            ("unsafe_count", current.unsafe_count, baseline.unsafe_count),
            (
                "from_raw_count",
                current.from_raw_count,
                baseline.from_raw_count,
            ),
            (
                "public_IR_fields",
                current.public_ir_fields,
                baseline.public_ir_fields,
            ),
            (
                "HashMap_hot_sites",
                current.hash_map_hot_sites,
                baseline.hash_map_hot_sites,
            ),
        ];
        for (metric, cur, base) in lower {
            if cur > base {
                out.push(Regression {
                    metric,
                    baseline: base,
                    current: cur,
                });
            }
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
            unwrap_count: 7,
            unsafe_count: 1,
            from_raw_count: 2,
            public_ir_fields: 4,
            hash_map_hot_sites: 3,
        };
        let v = m.to_json();
        assert_eq!(v["inline_allows"], serde_json::json!(0));
        assert_eq!(v["public_IR_fields"], serde_json::json!(4));
        assert_eq!(v["HashMap_hot_sites"], serde_json::json!(3));
        let back = RepoMetrics::from_json(&v).expect("round trip");
        assert_eq!(back, m);
    }

    #[test]
    fn from_json_rejects_the_old_seven_key_shape() {
        // A baseline written before the M0.6 expansion must fail loudly at
        // load time, not silently parse as zeros — the ratchet would
        // otherwise compare real counts against a fiction.
        let old = serde_json::json!({
            "rust_files": 3, "code_lines": 100, "public_items": 10,
            "documented_items": 8, "inline_allows": 0, "test_functions": 5,
            "workspace_members": 36,
        });
        assert!(
            RepoMetrics::from_json(&old).is_err(),
            "the seven-key pre-expansion baseline shape must not parse"
        );
    }

    // ------------------------------------------------------------------
    // Ratchet
    // ------------------------------------------------------------------

    #[test]
    fn ratchet_accepts_improvements_and_equal_values() {
        let base = full_metrics_fixture();
        let mut better = base;
        better.rust_files += 1;
        better.code_lines += 100;
        better.public_items += 1;
        better.documented_items += 5;
        better.test_functions += 5;
        better.unwrap_count = base.unwrap_count.saturating_sub(1);
        better.unsafe_count = base.unsafe_count.saturating_sub(1);
        better.from_raw_count = base.from_raw_count.saturating_sub(1);
        better.public_ir_fields = base.public_ir_fields.saturating_sub(1);
        better.hash_map_hot_sites = base.hash_map_hot_sites.saturating_sub(1);
        assert_eq!(Ratchet::check(&better, &base), [] as [Regression; 0]);
    }

    /// A representative non-zero metrics fixture for the ratchet tests.
    fn full_metrics_fixture() -> RepoMetrics {
        RepoMetrics {
            rust_files: 10,
            code_lines: 1000,
            public_items: 100,
            documented_items: 90,
            inline_allows: 0,
            test_functions: 50,
            workspace_members: 36,
            unwrap_count: 20,
            unsafe_count: 4,
            from_raw_count: 8,
            public_ir_fields: 6,
            hash_map_hot_sites: 12,
        }
    }

    #[test]
    fn ratchet_flags_lower_is_better_regression() {
        let base = full_metrics_fixture();
        let worse = RepoMetrics {
            inline_allows: 1,
            ..base
        };
        let regressions = Ratchet::check(&worse, &base);
        assert_eq!(regressions.len(), 1);
        assert_eq!(regressions[0].metric, "inline_allows");
        assert_eq!(regressions[0].baseline, 0);
        assert_eq!(regressions[0].current, 1);
    }

    #[test]
    fn ratchet_flags_higher_is_better_regression() {
        let base = full_metrics_fixture();
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
    fn ratchet_flags_every_new_lower_is_better_metric() {
        // Each of the five M0.6-expansion counters must fail the ratchet
        // when it increases — that is the "must-not-increase" contract.
        let base = full_metrics_fixture();
        let cases: [(&str, fn(&mut RepoMetrics)); 5] = [
            ("unwrap_count", |m| m.unwrap_count += 1),
            ("unsafe_count", |m| m.unsafe_count += 1),
            ("from_raw_count", |m| m.from_raw_count += 1),
            ("public_IR_fields", |m| m.public_ir_fields += 1),
            ("HashMap_hot_sites", |m| m.hash_map_hot_sites += 1),
        ];
        for (metric, bump) in cases {
            let mut worse = base;
            bump(&mut worse);
            let regressions = Ratchet::check(&worse, &base);
            assert_eq!(regressions.len(), 1, "{metric} must be ratcheted");
            assert_eq!(regressions[0].metric, metric);
        }
    }

    #[test]
    fn ratchet_allows_workspace_members_to_grow_or_shrink() {
        // Members is informational: neither direction is a regression.
        let base = full_metrics_fixture();
        let grown = RepoMetrics {
            workspace_members: base.workspace_members + 1,
            ..base
        };
        let shrunk = RepoMetrics {
            workspace_members: base.workspace_members - 1,
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
        assert_eq!(m.unwrap_count, 0);
        assert_eq!(m.unsafe_count, 0);
        assert_eq!(m.from_raw_count, 0);
        assert_eq!(m.public_ir_fields, 0);
        assert_eq!(m.hash_map_hot_sites, 0);

        std::fs::remove_dir_all(&root).ok();
    }

    // ------------------------------------------------------------------
    // strip_test_regions — the non-test counters' foundation
    // ------------------------------------------------------------------

    #[test]
    fn test_regions_strip_cfg_test_mods_and_test_fns() {
        let src = concat!(
            "pub fn prod() { let x = Option::<u8>::None.unwrap(); }\n",
            "#[cfg(test)]\n",
            "mod tests {\n",
            "    #[test]\n",
            "    fn t() { let y = x.unwrap(); }\n",
            "}\n",
            "#[test]\n",
            "fn t2() { assert_eq!(a.expect(\"x\"), 1); }\n",
            "pub fn prod2() { y.unwrap(); }\n",
        );
        assert_eq!(count_unwrap_calls(src), 2);
    }

    #[test]
    fn test_regions_handle_multiline_signatures_and_strings() {
        // A fn whose signature spans lines must still close its region at
        // the right brace, and braces inside string literals must not
        // unbalance the tracking.
        let src = concat!(
            "#[cfg(test)]\n",
            "mod tests {\n",
            "    #[test]\n",
            "    fn t(\n",
            "        a: u8,\n",
            "    ) {\n",
            "        let s = \"}{ weird {\";\n",
            "        a.unwrap();\n",
            "    }\n",
            "}\n",
            "pub fn after() { b.unwrap(); }\n",
        );
        assert_eq!(count_unwrap_calls(src), 1);
    }

    #[test]
    fn test_regions_keep_code_after_the_skipped_region() {
        // The classic strip bug: everything after `mod tests` silently
        // dropped, under-counting production code. The region must end
        // exactly at its closing brace.
        let src = concat!(
            "#[cfg(test)]\n",
            "mod tests {\n",
            "    fn a() { x.unwrap(); }\n",
            "}\n",
            "pub struct S { pub f: u8 }\n",
            "pub fn b() { y.unwrap(); }\n",
            "pub fn c() { z.expect(\"m\"); }\n",
        );
        assert_eq!(count_unwrap_calls(src), 2);
        assert_eq!(count_public_fields(src), 1);
    }

    #[test]
    fn bodyless_cfg_test_mod_declaration_is_dropped_alone() {
        let src = "#[cfg(test)]\nmod tests;\npub fn prod() { x.unwrap(); }\n";
        assert_eq!(count_unwrap_calls(src), 1);
    }

    // ------------------------------------------------------------------
    // count_unwrap_calls / count_unsafe_items / count_from_raw_calls
    // ------------------------------------------------------------------

    #[test]
    fn unwrap_calls_count_unwrap_and_expect_but_not_unwrap_or() {
        let src = concat!(
            "let a = x.unwrap();\n",
            "let b = y.unwrap_or(0);\n",
            "let c = z.expect(\"message\");\n",
            "fn helper() -> &str { \"nothing here\" }\n",
        );
        assert_eq!(count_unwrap_calls(src), 2);
    }

    #[test]
    fn unsafe_items_count_fn_impl_trait_block_not_prose() {
        let src = concat!(
            "unsafe fn raw() {}\n",
            "unsafe impl Send for T {}\n",
            "unsafe trait Marker {}\n",
            "let v = unsafe { *ptr };\n",
            "fn safe(x: bool) -> bool { x.is_unsafe() || unsafe_fn_marker() }\n",
            "// prose: unsafe code lives here\n",
        );
        assert_eq!(count_unsafe_items(src), 4);
    }

    #[test]
    fn from_raw_counts_exact_calls_not_from_raw_parts() {
        let src = concat!(
            "let p = Box::from_raw(raw);\n",
            "let q = Vec::from_raw_parts(p, 1, 1);\n",
            "fn from_raw_elsewhere() {}\n",
            "let r = from_raw(ptr2);\n",
            "// doc: Box::from_raw is dangerous\n",
        );
        assert_eq!(count_from_raw_calls(src), 2);
    }

    // ------------------------------------------------------------------
    // count_public_fields / count_hashmap_sites
    // ------------------------------------------------------------------

    #[test]
    fn public_fields_count_pub_fields_on_public_structs_only() {
        let src = concat!(
            "pub struct I {\n",
            "    pub visible: u8,\n",
            "    hidden: u8,\n",
            "    /// documented\n",
            "    pub other: Vec<u8>,\n",
            "}\n",
            "struct Private {\n",
            "    pub invisible: u8,\n",
            "}\n",
            "pub(crate) struct Q {\n",
            "    pub(crate) flagged: u8,\n",
            "}\n",
        );
        assert_eq!(count_public_fields(src), 3);
    }

    #[test]
    fn public_fields_handle_one_line_structs() {
        let src = "pub struct P { pub a: u8, b: u8 }\nfn main() {}\n";
        assert_eq!(count_public_fields(src), 1);
    }

    #[test]
    fn hashmap_sites_respect_word_boundaries() {
        let src = concat!(
            "use std::collections::HashMap;\n",
            "let m: HashMap<u8, u8> = HashMap::new();\n",
            "type Fast = FxHashMap<u8, u8>;\n",
            "fn take(m: &HashMap<u8, u8>) {}\n",
        );
        assert_eq!(count_hashmap_sites(src), 4);
    }
}
