//! Content-addressed compilation cache.
//!
//! # Why
//!
//! Every invocation of the compiler currently re-runs the entire pipeline —
//! parse → expand → def-map → HIR → typeck → lower → mono → codegen — even
//! when the input and build configuration are byte-for-byte identical to a
//! previous run. For iterative work (`glyip build` after a one-line edit to a
//! *different* file, `glyip test`, editor save-hooks) that is the dominant
//! cost.
//!
//! # What
//!
//! A **whole-crate, coarse-grained** cache: the produced object file is stored
//! under a key derived from the *complete* inputs — the flattened crate source
//! (which already includes every external module's contents, see
//! `glyim_pipeline::mod_loader`), the target triple, the optimization level,
//! and whether a C-ABI `main` wrapper was requested. Any change to any of
//! those changes the key, so a stale object is never served.
//!
//! This is the same granularity as Cargo's per-crate incremental build: it
//! skips the *entire* crate's compilation when nothing that affects its output
//! has changed. Finer-grained (per-item / per-query) caching is a follow-up;
//! this first rung already eliminates the "nothing changed, recompile
//! everything" case.
//!
//! # Layout
//!
//! ```text
//! <root>/
//!   objects/<key>.o     the produced object file, byte-identical to a fresh compile
//! ```
//!
//! # Location
//!
//! The root is chosen once per process, in priority order:
//!   1. `$GLYIM_CACHE_DIR` — explicit override (tests set this).
//!   2. `$HOME/.glyip/cache/glyim` — shared per-user cache.
//!   3. `<system temp>/glyim-cache` — best-effort fallback when `$HOME` is
//!      unavailable (no persistence across reboots, but the compiler still
//!      functions).
//!
//! # Correctness
//!
//! - **Only successful compiles are stored.** A failed pipeline never writes
//!   an object, so a cache hit always means "this exact input compiled
//!   cleanly and produced this exact object".
//! - **The object file is written atomically** (temp file + rename) so a
//!   concurrent reader never observes a partial write.
//! - **Cache misses are silent.** If the cache directory is unwritable, the
//!   compiler still compiles from scratch — a cache is an optimisation, never
//!   a correctness dependency.

use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

/// A content-addressed object-file cache.
pub struct CompileCache {
    root: PathBuf,
}

impl CompileCache {
    /// Open (and lazily create) a cache rooted at `root`.
    pub fn open(root: impl Into<PathBuf>) -> Self {
        let root = root.into();
        // Best effort: a failure to create the directory means `lookup` will
        // miss and `store` will no-op, which is exactly the "no cache"
        // behaviour. Never fatal.
        let _ = std::fs::create_dir_all(root.join("objects"));
        Self { root }
    }

    /// Open the cache at the process-wide default location.
    ///
    /// See the module docs for the priority order.
    pub fn open_default() -> Self {
        if let Ok(dir) = std::env::var("GLYIM_CACHE_DIR") {
            return Self::open(dir);
        }
        if let Some(home) = std::env::var_os("HOME") {
            return Self::open(PathBuf::from(home).join(".glyip").join("cache").join("glyim"));
        }
        Self::open(std::env::temp_dir().join("glyim-cache"))
    }

    /// Directory holding the cached objects.
    pub fn objects_dir(&self) -> PathBuf {
        self.root.join("objects")
    }

    /// Compute the cache key for a compilation.
    ///
    /// All inputs that can change the produced object MUST be included. The
    /// `source` is the **flattened** crate source (external modules already
    /// spliced in), so a change to any module's contents changes the key.
    pub fn key(source: &str, target: &str, opt_level: u8, entry_main: bool) -> String {
        let mut h = Sha256::new();
        // Field separator so `("ab", "c")` and `("a", "bc")` cannot collide.
        h.update(b"source\x1f");
        h.update(source.as_bytes());
        h.update(b"\x1ftarget\x1f");
        h.update(target.as_bytes());
        h.update(b"\x1fopt\x1f");
        h.update([opt_level]);
        h.update(b"\x1fentry\x1f");
        h.update([entry_main as u8]);
        let digest = h.finalize();
        // Lowercase hex without pulling in `hex` as a dependency.
        let mut s = String::with_capacity(digest.len() * 2);
        for b in digest {
            use std::fmt::Write as _;
            let _ = write!(s, "{b:02x}");
        }
        s
    }

    /// Path an object for `key` would occupy, whether or not it exists.
    pub fn object_path(&self, key: &str) -> PathBuf {
        self.objects_dir().join(format!("{key}.o"))
    }

    /// Look up a cached object. Returns its path if present and non-empty.
    ///
    /// "Non-empty" guards against a partially-written file left by a previous
    /// crash (the writer is atomic, but a foreign process could still corrupt
    /// the directory; an empty file is never a valid object).
    pub fn lookup(&self, key: &str) -> Option<PathBuf> {
        let path = self.object_path(key);
        match std::fs::metadata(&path) {
            Ok(m) if m.len() > 0 => Some(path),
            _ => None,
        }
    }

    /// Store `object` under `key`, atomically.
    ///
    /// Best effort: any I/O failure is silently ignored (the compile already
    /// succeeded; not caching only costs time next run).
    pub fn store(&self, key: &str, object: &Path) {
        let dest = self.object_path(key);
        // Unique temp name so concurrent compiles of different crates do not
        // race on a shared `.tmp`.
        let tmp = self
            .objects_dir()
            .join(format!(".{key}.{}.tmp", std::process::id()));
        if std::fs::copy(object, &tmp).is_ok() {
            // Rename is atomic on every supported platform; a reader sees
            // either the old (absent) or the new (complete) file, never a
            // partial one.
            let _ = std::fs::rename(&tmp, &dest);
        }
        let _ = std::fs::remove_file(&tmp);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_is_stable_and_input_sensitive() {
        let a = CompileCache::key("fn main() {}", "x86_64-unknown-linux-gnu", 0, false);
        let b = CompileCache::key("fn main() {}", "x86_64-unknown-linux-gnu", 0, false);
        assert_eq!(a, b, "identical inputs must produce identical keys");

        let c = CompileCache::key("fn main() {}", "x86_64-unknown-linux-gnu", 0, true);
        assert_ne!(a, c, "entry_main must affect the key");

        let d = CompileCache::key("fn main() {}", "aarch64-apple-darwin", 0, false);
        assert_ne!(a, d, "target must affect the key");

        let e = CompileCache::key("fn main() {}", "x86_64-unknown-linux-gnu", 2, false);
        assert_ne!(a, e, "opt level must affect the key");

        let f = CompileCache::key("fn main() { }", "x86_64-unknown-linux-gnu", 0, false);
        assert_ne!(a, f, "source must affect the key");
    }

    #[test]
    fn separator_prevents_field_collision() {
        // Without an explicit separator, ("ab", "c") would hash the same as
        // ("a", "bc").
        let a = CompileCache::key("ab", "c", 0, false);
        let b = CompileCache::key("a", "bc", 0, false);
        assert_ne!(a, b);
    }

    #[test]
    fn store_then_lookup_roundtrips() {
        let dir = tempfile::tempdir().unwrap();
        let cache = CompileCache::open(dir.path());
        let key = "deadbeef";
        assert!(cache.lookup(key).is_none(), "empty cache must miss");

        let src = dir.path().join("fake.o");
        std::fs::write(&src, b"\x7fELF...").unwrap();
        cache.store(key, &src);

        let hit = cache.lookup(key).expect("store must make the object visible");
        assert_eq!(std::fs::read(hit).unwrap(), b"\x7fELF...");
    }
}
