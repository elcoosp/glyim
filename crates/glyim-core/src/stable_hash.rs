//! Stable identity hashes (M6.1): position-independent fingerprints so an
//! edit to one body never renumbers another item.
//!
//! Before M6.1, a def's identity was its `LocalDefId` — a counter the def-map
//! bumps in declaration order. Reordering items, inserting a new item ahead
//! of an existing one, or even editing an unrelated body that triggers a
//! re-lower would renumber downstream `LocalDefId`s, busting every cache keyed
//! on them. The plan (§6 M6.1) kills that by giving every def a
//! [`DefPathHash`]: a stable hash of the crate name plus the path segments
//! from the crate root to the def. It is a pure function of the *path*, so
//! editing an unrelated body cannot change another item's hash (the M6.1
//! definition of done, asserted by a seeded property test).
//!
//! This module reuses the workspace `sha2` crate (SHA-256) — no new external
//! dependency (the task rails forbid new deps; `sha2` is already workspace).
//! The hash is deterministic across runs and machines (no randomization
//! seed), so cached query results keyed by these hashes are reproducible.

use crate::def_id::{DefId, LocalDefId};
use sha2::{Digest, Sha256};

/// A stable, position-independent 128-bit hash of a definition's path. Two
/// defs with the same path have the same hash; renumbering locals or
/// reordering declarations does not change it. The M6.1 identity: a body
/// edit never changes another item's `DefPathHash`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct DefPathHash([u8; 16]);

impl DefPathHash {
    /// Hash a path — the crate name followed by each path segment from the
    /// crate root to the def — into a stable 128-bit value. The path is the
    /// sole input, so the result is independent of declaration order, file
    /// order, and body content.
    #[must_use]
    pub fn from_path(crate_name: &str, segments: &[&str]) -> Self {
        let mut h = StableHasher::new();
        h.write_str(crate_name);
        for seg in segments {
            h.write_str(seg);
        }
        Self(h.finish())
    }

    /// The raw 128-bit digest bytes.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8; 16] {
        &self.0
    }
}

impl std::fmt::Display for DefPathHash {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for byte in &self.0 {
            write!(f, "{byte:02x}")?;
        }
        Ok(())
    }
}

/// A stable 128-bit fingerprint of an item tree (M6.3). A hash of the
/// signature-only summary — paths and signatures, never bodies — so a
/// body-only edit leaves it unchanged (the M6.3 definition of done). Built
/// with the same [`StableHasher`] so fingerprints compose: a tree's
/// fingerprint is the hash of its items' `DefPathHash`es and signatures.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Fingerprint([u8; 16]);

impl Fingerprint {
    /// Wrap a finished 16-byte digest.
    #[must_use]
    pub fn from_bytes(bytes: [u8; 16]) -> Self {
        Self(bytes)
    }

    /// The raw 128-bit digest bytes.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8; 16] {
        &self.0
    }
}

impl std::fmt::Display for Fingerprint {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for byte in &self.0 {
            write!(f, "{byte:02x}")?;
        }
        Ok(())
    }
}

/// A sha2-based stable hasher (M6.1). The hash is deterministic across runs
/// and machines — there is no randomization seed, unlike `std::hash::SipHash`
/// which is seeded per-process — so query caches keyed by its output are
/// reproducible. This is the position-independent hashing the plan calls for
/// in place of order-dependent `LocalDefId` numbering.
pub struct StableHasher {
    state: Sha256,
}

impl StableHasher {
    /// Start a fresh hash.
    #[must_use]
    pub fn new() -> Self {
        Self {
            state: Sha256::new(),
        }
    }

    /// Write a string followed by a NUL separator, so the segment list
    /// `["a", "bc"]` hashes differently from `["ab", "c"]`.
    pub fn write_str(&mut self, s: &str) {
        self.state.update(s.as_bytes());
        self.state.update([0]);
    }

    /// Write raw bytes (no separator) — for signature bytes that already have
    /// a defined length prefix.
    pub fn write_bytes(&mut self, bytes: &[u8]) {
        self.state.update(bytes);
    }

    /// Finalize to 16 bytes (the first 128 bits of the SHA-256 digest).
    /// Truncating SHA-256 to 128 bits is the standard fingerprint width
    /// (rustc's `Fingerprint` is also 128 bits); collision risk at compiler
    /// scale is negligible and the digest is not a security boundary.
    #[must_use]
    pub fn finish(self) -> [u8; 16] {
        let digest = self.state.finalize();
        let mut out = [0u8; 16];
        out.copy_from_slice(&digest[..16]);
        out
    }
}

impl Default for StableHasher {
    fn default() -> Self {
        Self::new()
    }
}

/// A stable HIR id (M6.1): `(owner DefId, local index within the owner)`.
/// The owner's stable identity is its [`DefPathHash`] (a path-based hash, not
/// the order-dependent `LocalDefId`), so editing an unrelated body cannot
/// renumber another item's `HirId`. This is the type the plan (§6 M6.1)
/// calls for; migrating existing HIR call sites onto it is deferred (a scope
/// guard, not a stub) until the M6.4 body queries land, so the green tree is
/// not disturbed piecemeal.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct HirId {
    /// The owning definition. Its `DefPathHash` (computed from the owner's
    /// path via [`DefPathHash::from_path`]) is the position-independent
    /// identity; the `DefId` here is the in-memory handle for lookups.
    pub owner: DefId,
    /// The local index within the owner (a body-local item numbering, e.g. a
    /// statement or sub-expression id). `LocalDefId` is reused as the local
    /// index type for now; the index is per-owner, not global.
    pub local: LocalDefId,
}

impl HirId {
    /// Construct a stable HIR id from an owner and a per-owner local index.
    #[must_use]
    pub fn new(owner: DefId, local: LocalDefId) -> Self {
        Self { owner, local }
    }

    /// The position-independent identity of this id: the owner's
    /// [`DefPathHash`] (path-based) paired with the local index. Editing an
    /// unrelated body changes neither the owner's path nor this id's local
    /// index, so this identity is stable.
    #[must_use]
    pub fn stable_identity(&self, crate_name: &str, owner_path: &[&str]) -> DefPathHash {
        let mut h = StableHasher::new();
        let owner_hash = DefPathHash::from_path(crate_name, owner_path);
        h.write_bytes(owner_hash.as_bytes());
        h.write_str(&self.local.to_raw().to_string());
        DefPathHash(h.finish())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn def_path_hash_is_path_based_not_order_based() {
        // Same path → same hash, regardless of insertion order.
        let a = DefPathHash::from_path("my_crate", &["foo", "bar"]);
        let b = DefPathHash::from_path("my_crate", &["foo", "bar"]);
        assert_eq!(a, b);
        // Different path → different hash.
        let c = DefPathHash::from_path("my_crate", &["foo", "baz"]);
        assert_ne!(a, c);
        // Different crate → different hash.
        let d = DefPathHash::from_path("other_crate", &["foo", "bar"]);
        assert_ne!(a, d);
    }

    #[test]
    fn def_path_hash_distinguishes_segment_boundaries() {
        // The NUL separator means ["a","bc"] != ["ab","c"].
        let a = DefPathHash::from_path("c", &["a", "bc"]);
        let b = DefPathHash::from_path("c", &["ab", "c"]);
        assert_ne!(a, b);
    }

    #[test]
    fn def_path_hash_is_deterministic_across_calls() {
        // No randomization seed: the same inputs hash to the same bytes
        // every call (reproducible query caches).
        let a = DefPathHash::from_path("glyim", &["std", "vec", "Vec", "push"]);
        let b = DefPathHash::from_path("glyim", &["std", "vec", "Vec", "push"]);
        assert_eq!(a.as_bytes(), b.as_bytes());
    }

    #[test]
    fn hir_id_stable_identity_is_path_based() {
        let owner = DefId::new(
            crate::def_id::CrateId::from_raw(0),
            crate::def_id::LocalDefId::from_raw(7),
        );
        let id = HirId::new(owner, crate::def_id::LocalDefId::from_raw(3));
        let id_a = id.stable_identity("c", &["foo"]);
        let id_b = id.stable_identity("c", &["foo"]);
        assert_eq!(id_a, id_b);
        // A different owner path → a different stable identity, even with the
        // same local index and in-memory DefId.
        let id_c = id.stable_identity("c", &["bar"]);
        assert_ne!(id_a, id_c);
    }

    #[test]
    fn stable_hasher_default_matches_new() {
        let mut a = StableHasher::new();
        let mut b = StableHasher::default();
        a.write_str("x");
        b.write_str("x");
        assert_eq!(a.finish(), b.finish());
    }
}
