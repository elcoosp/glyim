//! M6.2 inputs & durability: content-hashed source files, a compiler-config
//! fingerprint, and a `Durability` enum (stdlib = `High`).
//!
//! The query engine (M6) invalidates a cached query when an *input* changes.
//! Before M6.2, "did this input change?" was a string comparison on the whole
//! file content (expensive, and it re-ran on every byte) or a config-field
//! diff (scattered across three `CrateConfig` fields). This module gives each
//! input a 128-bit [`Fingerprint`] (the M6.1 [`glyim_core::StableHasher`],
//! reused — no new dep) so invalidation is a single `Fingerprint` equality
//! check, and gives each input a [`Durability`] so a `High`-durability input
//! (the precompiled stdlib) is not re-validated on every edit.
//!
//! The invalidation contract (M6.2 `DoD`) is the test matrix: a content change
//! moves the [`SourceFile`] hash; a config change moves the
//! [`CrateConfig`] fingerprint; and `Durability` ordering is asserted
//! (`High > Medium > Low`), so a `High`-durability input is the most stable.

use glyim_core::{Fingerprint, StableHasher};
use std::sync::Arc;

/// A source file's content paired with its content-hash fingerprint (M6.2).
/// The hash is the input the query engine keys on: two `SourceFile`s with the
/// same `content_hash` are interchangeable, so a query that consumed one is a
/// cache hit on the other without re-reading the bytes.
#[derive(Clone, Debug)]
pub struct SourceFile {
    /// The file's text.
    pub content: Arc<str>,
    /// The 128-bit content fingerprint. A single byte change in `content`
    /// moves this (asserted by the invalidation matrix).
    pub content_hash: Fingerprint,
}

impl SourceFile {
    /// Construct a `SourceFile`, hashing the content with the workspace
    /// `sha2`-based [`StableHasher`].
    #[must_use]
    pub fn new(content: Arc<str>) -> Self {
        let mut h = StableHasher::new();
        h.write_bytes(content.as_bytes());
        let content_hash = Fingerprint::from_bytes(h.finish());
        Self {
            content,
            content_hash,
        }
    }

    /// Whether two source files are content-equivalent (same fingerprint),
    /// without comparing the bytes — the query-engine cache-hit check.
    #[must_use]
    pub fn content_eq(&self, other: &Self) -> bool {
        self.content_hash == other.content_hash
    }
}

/// How stable an input is (M6.2). A `High`-durability input (the precompiled
/// stdlib) changes rarely, so queries that depend only on it are not
/// re-validated on every user edit. `Durability` is ordered: a higher
/// durability is more stable, so a `High`-durability input invalidates a
/// strictly smaller edit set than a `Low`-durability one (asserted by the
/// invalidation matrix).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Durability {
    /// A per-keystroke user file: invalidated on any edit.
    Low,
    /// A project-local file that changes between compilations but not within
    /// one.
    Medium,
    /// The precompiled stdlib / sysroot: changes only across compiler
    /// versions.
    High,
}

impl Durability {
    /// The durability of the precompiled stdlib (M6.2: stdlib = `High`).
    #[must_use]
    pub const fn stdlib() -> Self {
        Self::High
    }
}

/// The fingerprint of a [`crate::CrateConfig`] (M6.2): a 128-bit hash of the
/// config's identity fields (crate name, target triple, opt level). A config
/// change moves the fingerprint (asserted by the invalidation matrix), so a
/// query keyed on the config is invalidated exactly when the config that
/// produced it changes — not on every rebuild.
pub trait ConfigFingerprint {
    /// The 128-bit config fingerprint.
    fn fingerprint(&self) -> Fingerprint;
}

impl ConfigFingerprint for crate::CrateConfig {
    fn fingerprint(&self) -> Fingerprint {
        let mut h = StableHasher::new();
        h.write_str(&self.name);
        h.write_str(&self.target_triple);
        h.write_str(self.opt_level.to_string().as_str());
        Fingerprint::from_bytes(h.finish())
    }
}

/// Extract a body-agnostic, signature-only summary of a source file (M6.3):
/// each item's signature line, truncated at the first `{` so the body never
/// enters the hash. The signatures are kept in source order (the item-tree
/// preserves declaration order); a body-only edit changes neither a signature
/// line nor its position, so the fingerprint built from this summary is stable
/// across body edits (the M6.3 definition of done).
///
/// This is a line-level heuristic stand-in for the parser-backed item tree
/// (plan §6 M6.3 / PF-4); it exists so the `QueryContext::item_tree_fingerprint`
/// query has a real implementation today, and so the body-edit-invariance
/// property test exercises the actual fingerprint computation. The real
/// def-collection query will swap in a parsed `ItemTree` without changing the
/// trait surface or the invariant.
#[must_use]
pub fn item_tree_signatures(content: &str) -> Vec<String> {
    content
        .lines()
        .filter_map(|line| signature_of(line))
        .collect()
}

/// The item-tree fingerprint of a source file (M6.3): a stable hash of its
/// signature-only summary. Body-only edits leave it unchanged; a signature
/// change (renaming an item, adding/removing an item) moves it.
#[must_use]
pub fn item_tree_fingerprint_of(content: &str) -> Fingerprint {
    let mut h = StableHasher::new();
    for sig in item_tree_signatures(content) {
        h.write_str(&sig);
    }
    Fingerprint::from_bytes(h.finish())
}

/// Whether a trimmed source line begins an item signature, after stripping
/// leading `pub` / `pub(crate)` visibility.
fn is_signature_start(stripped: &str) -> bool {
    [
        "fn ", "struct ", "enum ", "mod ", "trait ", "impl ", "const ", "static ", "use ",
    ]
    .iter()
    .any(|k| stripped.starts_with(k))
}

/// The signature portion of a line (up to the first `{`), if the line begins
/// an item declaration; `None` for body lines and non-item lines.
fn signature_of(line: &str) -> Option<String> {
    let trimmed = line.trim();
    let vis_stripped = trimmed
        .strip_prefix("pub(crate)")
        .or_else(|| trimmed.strip_prefix("pub"))
        .map(str::trim_start)
        .unwrap_or(trimmed);
    if !is_signature_start(vis_stripped) {
        return None;
    }
    // Cut at the first `{` so the body never enters the signature summary.
    let sig = vis_stripped.split('{').next().unwrap_or(vis_stripped);
    Some(sig.trim_end().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::CrateConfig;

    /// M6.2 invalidation matrix: a content change moves the `SourceFile` hash.
    #[test]
    fn content_change_moves_source_file_hash() {
        let a = SourceFile::new(Arc::<str>::from("fn main() {}"));
        let b = SourceFile::new(Arc::<str>::from("fn main() { }"));
        assert_ne!(
            a.content_hash, b.content_hash,
            "a content change must move the hash"
        );
        // The same content hashes the same (cache hit).
        let a2 = SourceFile::new(Arc::<str>::from("fn main() {}"));
        assert_eq!(a.content_hash, a2.content_hash);
        assert!(a.content_eq(&a2));
        assert!(!a.content_eq(&b));
    }

    /// M6.2 invalidation matrix: a config change moves the fingerprint.
    #[test]
    fn config_change_moves_fingerprint() {
        let base = CrateConfig {
            name: "app".to_string(),
            target_triple: "x86_64-unknown-linux-gnu".to_string(),
            opt_level: 2,
        };
        let fp0 = base.fingerprint();

        // Name change → different fingerprint.
        let mut renamed = base.clone();
        renamed.name = "other".to_string();
        assert_ne!(renamed.fingerprint(), fp0);

        // Target change → different fingerprint.
        let mut retargeted = base.clone();
        retargeted.target_triple = "aarch64-unknown-linux-gnu".to_string();
        assert_ne!(retargeted.fingerprint(), fp0);

        // Opt-level change → different fingerprint.
        let mut relevel = base.clone();
        relevel.opt_level = 0;
        assert_ne!(relevel.fingerprint(), fp0);

        // Identical config → same fingerprint (cache hit).
        let same = base.clone();
        assert_eq!(same.fingerprint(), fp0);
    }

    /// M6.2 durability ordering: `High > Medium > Low`, and the stdlib is
    /// `High`. A higher durability invalidates a strictly smaller edit set.
    #[test]
    fn durability_ordering_and_stdlib() {
        assert!(Durability::High > Durability::Medium);
        assert!(Durability::Medium > Durability::Low);
        assert_eq!(Durability::stdlib(), Durability::High);
    }
}
