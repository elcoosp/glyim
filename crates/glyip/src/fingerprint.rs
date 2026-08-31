//! SHA-256 based fingerprinting for incremental compilation.

use sha2::{Digest, Sha256};

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

/// A fingerprint representing the state of a file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fingerprint {
    /// SHA-256 hex digest of the file content.
    pub hash: String,
    /// Last modification time (nanoseconds since epoch).
    pub mtime: u128,
    /// File size in bytes.
    pub size: u64,
}

impl Fingerprint {
    /// Compute a fingerprint by reading a file from disk.
    pub fn from_file(path: &Path) -> crate::error::GlyipResult<Self> {
        let content = fs::read(path)?;
        let metadata = fs::metadata(path)?;
        let mut hasher = Sha256::new();
        hasher.update(&content);
        let hash = hex::encode(hasher.finalize());
        Ok(Self {
            hash,
            mtime: metadata
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_nanos())
                .unwrap_or(0),
            size: metadata.len(),
        })
    }

    /// Compute a fingerprint from in-memory content.
    pub fn from_content(content: &[u8]) -> Self {
        let mut hasher = Sha256::new();
        hasher.update(content);
        let hash = hex::encode(hasher.finalize());
        Self {
            hash,
            mtime: 0,
            size: content.len() as u64,
        }
    }

    /// Check whether this fingerprint matches another (by hash).
    pub fn matches(&self, other: &Fingerprint) -> bool {
        self.hash == other.hash
    }

    /// Cheap metadata-only equality check (plan §1.5): compares `(size, mtime)`
    /// against the file currently on disk without reading its contents. This is
    /// the fast path used by incremental builds — files whose size and mtime
    /// are unchanged since the last fingerprint are assumed unchanged, avoiding
    /// a full `fs::read` + SHA-256 on every no-op build. Returns `true` when the
    /// on-disk metadata matches this fingerprint (so an expensive re-hash is
    /// unnecessary). Clock-skew / touch-without-edit cases are handled by the
    /// full `from_file` + `matches` comparison done by `has_changed`.
    pub fn metadata_matches_path(&self, path: &Path) -> bool {
        match fs::metadata(path) {
            Ok(m) => m.len() == self.size && {
                m.modified()
                    .ok()
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|d| d.as_nanos())
                    .unwrap_or(0)
                    == self.mtime
            },
            Err(_) => false,
        }
    }
}

/// Persistent store of file fingerprints for incremental compilation.
#[derive(Debug, Clone, Default)]
pub struct FingerprintStore {
    fingerprints: HashMap<PathBuf, Fingerprint>,
    /// Fingerprint of the build configuration recorded at the last successful
    /// build (opt-level, target, backend, release, LTO). Loaded from / saved to
    /// disk. When the *current* build config (set via [`FingerprintStore::
    /// set_build_config`]) differs from this recorded value, the whole
    /// incremental cache is invalidated (plan §4.2) — a flag change must force a
    /// rebuild even if no source changed.
    recorded_config_hash: Option<String>,
    /// Fingerprint of the active build configuration for the build currently in
    /// progress. Set by [`FingerprintStore::set_build_config`]; compared against
    /// `recorded_config_hash` during change checks and persisted on save.
    current_config_hash: Option<String>,
}

impl FingerprintStore {
    /// Create a new empty fingerprint store.
    pub fn new() -> Self {
        Self::default()
    }

    /// Record the active build configuration so subsequent change checks can
    /// detect a flag change (plan §4.2). Computed as a SHA-256 of the canonical
    /// config string.
    pub fn set_build_config(&mut self, opts: &crate::config::BuildOptions) {
        self.current_config_hash = Some(hash_build_config(opts));
    }

    /// PUBLIC: compute the build-config fingerprint for `opts` (plan §4.2).
    pub fn build_config_hash(opts: &crate::config::BuildOptions) -> String {
        hash_build_config(opts)
    }

    /// Recorded build-config fingerprint (from the last successful build).
    pub fn recorded_config_hash(&self) -> Option<&str> {
        self.recorded_config_hash.as_deref()
    }

    /// Active build-config fingerprint for the build in progress, if set.
    pub fn current_config_hash(&self) -> Option<&str> {
        self.current_config_hash.as_deref()
    }

    /// Whether the recorded build config matches `current`. Returns `true` when
    /// no config has been recorded yet (nothing to invalidate against) or when
    /// the hashes are equal; `false` when they differ (a flag change).
    pub fn config_matches(&self, current: &str) -> bool {
        match &self.recorded_config_hash {
            Some(recorded) => recorded == current,
            None => true,
        }
    }

    /// Load fingerprints from a `.fingerprint` directory inside `dir`.
    pub fn load_from_dir(dir: &Path) -> crate::error::GlyipResult<Self> {
        let fp_dir = dir.join(".fingerprint");
        if !fp_dir.exists() {
            return Ok(Self::new());
        }
        let mut store = Self::new();
        for entry in fs::read_dir(&fp_dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().is_some_and(|e| e == "fp") {
                let content = fs::read_to_string(&path)?;
                if path.file_name().is_some_and(|n| n == "config.fp") {
                    // Plan §4.2: restore the recorded build-config fingerprint.
                    if let Some(hash) = content.strip_prefix("config_hash=") {
                        store.recorded_config_hash = Some(hash.trim_end().to_string());
                    }
                    continue;
                }
                if let Some((rel_path, fp)) = parse_fingerprint_file(&content) {
                    store.fingerprints.insert(PathBuf::from(rel_path), fp);
                }
            }
        }
        Ok(store)
    }

    /// Save all fingerprints to a `.fingerprint` directory inside `dir`.
    pub fn save_to_dir(&self, dir: &Path) -> crate::error::GlyipResult<()> {
        let fp_dir = dir.join(".fingerprint");
        fs::create_dir_all(&fp_dir)?;
        // Persist the active build-config fingerprint (plan §4.2) under a
        // reserved key so a flag change can be detected on the next build.
        if let Some(cfg_hash) = self
            .current_config_hash
            .clone()
            .or_else(|| self.recorded_config_hash.clone())
        {
            let cfg_path = fp_dir.join("config.fp");
            fs::write(cfg_path, format!("config_hash={cfg_hash}\n"))?;
        }
        for (path, fp) in &self.fingerprints {
            let mut hasher = Sha256::new();
            hasher.update(path.to_string_lossy().as_bytes());
            let file_name = format!("{}.fp", hex::encode(hasher.finalize()));
            let fp_path = fp_dir.join(file_name);
            let content = format!(
                "path={}\nhash={}\nmtime={}\nsize={}\n",
                path.display(),
                fp.hash,
                fp.mtime,
                fp.size
            );
            fs::write(fp_path, content)?;
        }
        Ok(())
    }

    /// Return `true` if the file on disk differs from the stored fingerprint.
    /// Uses a cheap `(size, mtime)` stat first (plan §1.5); only falls back to a
    /// full content SHA-256 when the metadata disagrees (handles clock skew /
    /// touch-without-edit), so no-op incremental builds stay O(files-changed)
    /// instead of O(total source bytes).
    pub fn has_changed(&self, path: &Path) -> crate::error::GlyipResult<bool> {
        match self.fingerprints.get(path) {
            Some(stored) => {
                // Fast path: metadata unchanged ⇒ content unchanged (no read/hash).
                if stored.metadata_matches_path(path) {
                    return Ok(false);
                }
                let current = Fingerprint::from_file(path)?;
                Ok(!stored.matches(&current))
            }
            None => Ok(true),
        }
    }

    /// Update (or insert) the fingerprint for a single file.
    pub fn update(&mut self, path: &Path) -> crate::error::GlyipResult<()> {
        let fp = Fingerprint::from_file(path)?;
        self.fingerprints.insert(path.to_path_buf(), fp);
        Ok(())
    }

    /// Return `true` if any `.g` file under `dir` has changed, *or* if the
    /// project manifest / build scripts have changed (plan §23.3 — a manifest
    /// or dependency edit must invalidate incremental state even though those
    /// files do not themselves carry `extension`), *or* if the active build
    /// configuration differs from the one recorded at the last successful build
    /// (plan §4.2 — a flag change must force a rebuild even when no source
    /// changed).
    pub fn has_any_changed(
        &self,
        dir: &Path,
        extension: &str,
        current_config_hash: &str,
    ) -> crate::error::GlyipResult<bool> {
        // Plan §4.2: a build-config flag change invalidates the whole cache.
        if !self.config_matches(current_config_hash) {
            return Ok(true);
        }
        let files = collect_files_with_extension(dir, extension);
        for path in &files {
            if self.has_changed(path)? {
                return Ok(true);
            }
        }
        for cfg in config_files(dir) {
            if !cfg.exists() {
                continue;
            }
            if self.has_changed(&cfg)? {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// Update fingerprints for every file with `extension` under `dir`, plus
    /// the project manifest / build scripts (plan §23.3).
    pub fn update_all(&mut self, dir: &Path, extension: &str) -> crate::error::GlyipResult<()> {
        let files = collect_files_with_extension(dir, extension);
        for path in &files {
            self.update(path)?;
        }
        for cfg in config_files(dir) {
            if cfg.exists() {
                self.update(&cfg)?;
            }
        }
        Ok(())
    }

    /// Number of stored fingerprints.
    pub fn len(&self) -> usize {
        self.fingerprints.len()
    }

    /// Whether the store is empty.
    pub fn is_empty(&self) -> bool {
        self.fingerprints.is_empty()
    }
}

/// Collect all files with the given extension under a directory (non-recursive closure).
fn collect_files_with_extension(dir: &Path, extension: &str) -> Vec<PathBuf> {
    let mut result = Vec::new();
    if !dir.exists() {
        return result;
    }
    collect_files_recursive(dir, extension, &mut result);
    result
}

/// Plan §23.3: project-wide inputs that must also invalidate incremental state
/// but are not source files carrying `extension` (e.g. `.g`): the manifest and
/// any build scripts. Returns their (possibly non-existent) paths.
fn config_files(dir: &Path) -> Vec<PathBuf> {
    let mut files = vec![
        dir.join("glyim.toml"),
        dir.join("glyim.lock"),
        dir.join("build.g"),
        dir.join("build.rs"),
    ];
    // Also pick up any `*.toml` build-config files alongside the manifest.
    if dir.exists() {
        if let Ok(entries) = fs::read_dir(dir) {
            for entry in entries.flatten() {
                let p = entry.path();
                if p.extension().is_some_and(|e| e == "toml") && p.file_name().is_some_and(|n| n != "glyim.toml") {
                    files.push(p);
                }
            }
        }
    }
    files
}

/// Recursive helper to collect files.
fn collect_files_recursive(dir: &Path, extension: &str, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_files_recursive(&path, extension, out);
        } else if path.extension().is_some_and(|e| e == extension) {
            out.push(path);
        }
    }
}

/// Parse a simple key=value fingerprint file.
fn parse_fingerprint_file(content: &str) -> Option<(String, Fingerprint)> {
    let mut path = None;
    let mut hash = None;
    let mut mtime = None;
    let mut size = None;
    for line in content.lines() {
        if let Some(val) = line.strip_prefix("path=") {
            path = Some(val.to_string());
        } else if let Some(val) = line.strip_prefix("hash=") {
            hash = Some(val.to_string());
        } else if let Some(val) = line.strip_prefix("mtime=") {
            mtime = Some(val.parse::<u128>().ok()?);
        } else if let Some(val) = line.strip_prefix("size=") {
            size = Some(val.parse().ok()?);
        }
    }
    Some((
        path?,
        Fingerprint {
            hash: hash?,
            mtime: mtime?,
            size: size?,
        },
    ))
}

/// Canonical string form of a build configuration, used as the basis for its
/// fingerprint (plan §4.2). Any change to opt-level, target triple, backend,
/// release flag, or LTO strategy changes this string and therefore the hash.
fn canonical_build_config(opts: &crate::config::BuildOptions) -> String {
    let lto = match opts.lto {
        Some(glyim_codegen_llvm::passes::LtoKind::None) => "none",
        Some(glyim_codegen_llvm::passes::LtoKind::Thin) => "thin",
        Some(glyim_codegen_llvm::passes::LtoKind::Fat) => "fat",
        None => "none",
    };
    format!(
        "compiler={};release={};target={};backend={};opt_level={};lto={}",
        env!("CARGO_PKG_VERSION"),
        opts.release,
        opts.target.as_deref().unwrap_or(""),
        opts.backend,
        opts.opt_level,
        lto,
    )
}

/// SHA-256 fingerprint of the active build configuration (plan §4.2).
fn hash_build_config(opts: &crate::config::BuildOptions) -> String {
    let mut hasher = Sha256::new();
    hasher.update(canonical_build_config(opts).as_bytes());
    hex::encode(hasher.finalize())
}
