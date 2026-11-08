//! Build cache and target directory management.

use crate::error::GlyipResult;
use crate::fingerprint::FingerprintStore;
use std::fs;
use std::path::{Path, PathBuf};

/// Manages the build cache, target directory, and incremental state.
#[derive(Debug)]
pub struct Cache {
    /// Project root directory.
    root: PathBuf,
    /// Target directory (typically `<root>/target`).
    target_dir: PathBuf,
    /// Fingerprint store for incremental compilation.
    fingerprints: FingerprintStore,
}

impl Cache {
    /// Create a cache manager for the given project root.
    ///
    /// Ensures the `target/` directory exists and loads any previously saved
    /// fingerprints.
    pub fn new(root: &Path) -> GlyipResult<Self> {
        let target_dir = root.join("target");
        fs::create_dir_all(&target_dir)?;
        let fingerprints = FingerprintStore::load_from_dir(&target_dir)?;
        Ok(Self {
            root: root.to_path_buf(),
            target_dir,
            fingerprints,
        })
    }

    /// Return the target directory path.
    #[must_use]
    pub fn target_dir(&self) -> &Path {
        &self.target_dir
    }

    /// Return the debug output directory.
    #[must_use]
    pub fn debug_dir(&self) -> PathBuf {
        self.target_dir.join("debug")
    }

    /// Return the release output directory.
    #[must_use]
    pub fn release_dir(&self) -> PathBuf {
        self.target_dir.join("release")
    }

    /// Return the output directory for the given profile.
    #[must_use]
    pub fn output_dir(&self, release: bool) -> PathBuf {
        if release {
            self.release_dir()
        } else {
            self.debug_dir()
        }
    }

    /// Return the dependency cache directory.
    #[must_use]
    pub fn dep_dir(&self) -> PathBuf {
        self.target_dir.join("debug").join("dep")
    }

    /// Return the global cache directory (`~/.glyip/cache`).
    ///
    /// Unlike [`Cache::new`] (which operates on a project-local `target/`),
    /// this is a process-global location for cross-project artifacts
    /// (registry downloads, shared fingerprints). Callers that *write* here
    /// should call [`Cache::ensure_global_cache_dir`] first; this getter is
    /// side-effect-free so it stays cheap to call from read paths.
    #[must_use]
    pub fn global_cache_dir() -> PathBuf {
        home_dir().join(".glyip").join("cache")
    }

    /// Create the global cache directory (and its parents) if missing.
    ///
    /// Ensures a subsequent write to [`Cache::global_cache_dir`] cannot fail
    /// with a "no such file or directory" error (de-stubbing plan §21.1).
    pub fn ensure_global_cache_dir() -> GlyipResult<PathBuf> {
        let dir = Self::global_cache_dir();
        fs::create_dir_all(&dir)?;
        Ok(dir)
    }

    /// Check whether a single source file needs recompilation.
    pub fn needs_recompile(&self, source_path: &Path) -> GlyipResult<bool> {
        self.fingerprints.has_changed(source_path)
    }

    /// Check whether any `.g` source file under `src/` has changed.
    pub fn needs_rebuild(&self) -> GlyipResult<bool> {
        let src_dir = self.root.join("src");
        if !src_dir.exists() {
            return Ok(true);
        }
        // Plan §4.2: pass the active build-config fingerprint so a flag change
        // invalidates the incremental cache even when no source changed.
        let current = self.fingerprints.current_config_hash().unwrap_or("");
        self.fingerprints.has_any_changed(&src_dir, "g", current)
    }

    /// Record the active build configuration on this cache so subsequent
    /// rebuild checks can detect a flag change (plan §4.2). The build command
    /// must call this before [`Cache::needs_rebuild`].
    pub fn set_build_config(&mut self, opts: &crate::config::BuildOptions) {
        self.fingerprints.set_build_config(opts);
    }

    /// Update all fingerprints after a successful build and persist them.
    pub fn mark_built(&mut self) -> GlyipResult<()> {
        let src_dir = self.root.join("src");
        if src_dir.exists() {
            self.fingerprints.update_all(&src_dir, "g")?;
        }
        // Also fingerprint the Glyip.toml
        let config_path = self.root.join("Glyip.toml");
        if config_path.exists() {
            self.fingerprints.update(&config_path)?;
        }
        self.fingerprints.save_to_dir(&self.target_dir)
    }

    /// Remove all build artifacts.
    pub fn clean(&self) -> GlyipResult<()> {
        if self.target_dir.exists() {
            fs::remove_dir_all(&self.target_dir)?;
        }
        Ok(())
    }

    /// Return the expected path of the output binary.
    #[must_use]
    pub fn output_binary(&self, name: &str, release: bool) -> PathBuf {
        self.output_dir(release).join(name)
    }

    /// Store a compiled artifact in the dependency cache.
    pub fn store_artifact(&self, key: &str, data: &[u8]) -> GlyipResult<PathBuf> {
        let cache_dir = self.dep_dir();
        fs::create_dir_all(&cache_dir)?;
        let path = cache_dir.join(format!("{key}.gbc"));
        fs::write(&path, data)?;
        Ok(path)
    }

    /// Retrieve a compiled artifact from the dependency cache.
    pub fn get_artifact(&self, key: &str) -> GlyipResult<Option<Vec<u8>>> {
        let path = self.dep_dir().join(format!("{key}.gbc"));
        if path.exists() {
            Ok(Some(fs::read(&path)?))
        } else {
            Ok(None)
        }
    }

    /// Return the dependency cache directory for the given build profile.
    ///
    /// Debug builds use `target/debug/deps`, release builds use
    /// `target/release/deps`.
    #[must_use]
    pub fn profile_dep_dir(&self, release: bool) -> PathBuf {
        self.output_dir(release).join("dep")
    }

    /// Return the output directory for the given profile and optional target
    /// triple.
    ///
    /// When a target triple is specified, the output is placed under
    /// `target/<triple>/<profile>`, mirroring Cargo's layout.
    #[must_use]
    pub fn output_dir_for_target(&self, release: bool, target: Option<&str>) -> PathBuf {
        let profile = if release { "release" } else { "debug" };
        if let Some(triple) = target {
            self.target_dir.join(triple).join(profile)
        } else {
            self.target_dir.join(profile)
        }
    }

    /// Return the expected path of the output binary for a given profile and
    /// optional target triple.
    #[must_use]
    pub fn output_binary_for_target(
        &self,
        name: &str,
        release: bool,
        target: Option<&str>,
    ) -> PathBuf {
        self.output_dir_for_target(release, target).join(name)
    }

    /// Store a compiled artifact in the profile-specific dependency cache.
    ///
    /// Artifacts are stored as `<key>.gbc` under the profile's `dep`
    /// directory.
    pub fn store_artifact_for_profile(
        &self,
        key: &str,
        data: &[u8],
        release: bool,
    ) -> GlyipResult<PathBuf> {
        let cache_dir = self.profile_dep_dir(release);
        fs::create_dir_all(&cache_dir)?;
        let path = cache_dir.join(format!("{key}.gbc"));
        fs::write(&path, data)?;
        Ok(path)
    }

    /// Retrieve a compiled artifact from the profile-specific dependency cache.
    pub fn get_artifact_for_profile(
        &self,
        key: &str,
        release: bool,
    ) -> GlyipResult<Option<Vec<u8>>> {
        let path = self.profile_dep_dir(release).join(format!("{key}.gbc"));
        if path.exists() {
            Ok(Some(fs::read(&path)?))
        } else {
            Ok(None)
        }
    }
}

/// Best-effort home directory resolution.
fn home_dir() -> PathBuf {
    std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .map_or_else(|_| PathBuf::from("."), PathBuf::from)
}
