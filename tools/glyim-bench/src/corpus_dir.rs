//! The committed corpora directory (plan M2.3 remainder: "corpora
//! committed with hashes").
//!
//! M2.3 slice 1 landed the generator substrate ([`crate::corpus`]) and the
//! `glyim.corpus/1` manifest whose rows carry the FNV-1a identity of every
//! generated program. This module is the slice the master plan's exit
//! gate calls for next: **materializing the corpus as committed bytes** —
//! one `.g` source per program plus the `corpus.json` manifest — and
//! re-verifying those committed bytes against the templates.
//!
//! # The seam
//!
//! [`CorpusSink`] is the write face: [`DirCorpusSink`] materializes the
//! conventional `perf/corpus/` layout today; a future registry upload or
//! in-memory sink implements the same two methods without touching the
//! plan parsing or the CLI wiring. The read face is [`check_corpus_dir`]:
//! it loads the committed manifest, runs the substrate's template-verify
//! contract ([`crate::corpus::CorpusManifest::verify`]), then
//! re-hashes every committed `.g` file — so a hand-edited program file
//! (not just a template drift) fails the gate.
//!
//! # Determinism
//!
//! Program bytes are a pure function of the `(family, seed, size)` plan
//! (no clock, no ambient PRNG), and file names are
//! `<family>-<seed>-<size>.g` — so two `write_corpus_dir` calls with the
//! same plan produce byte-identical directories (pinned by tests), which
//! is what makes the committed directory diff-able in review.

use std::fmt;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use crate::corpus::{CorpusFamily, CorpusManifest, CorpusProgram, MAX_SIZE, generate};
use crate::error::BenchError;

/// The manifest file name inside a committed corpora directory.
pub const CORPUS_DIR_MANIFEST: &str = "corpus.json";

/// The write face of the committed-corpora seam.
///
/// Implement this to land a corpus somewhere other than a directory (a
/// registry upload, an in-memory buffer for tests) without touching the
/// plan parsing or the CLI wiring.
pub trait CorpusSink {
    /// Write one generated program (the sink owns the file naming).
    ///
    /// # Errors
    ///
    /// Sink-specific IO failures.
    fn write_program(&mut self, program: &CorpusProgram) -> Result<(), BenchError>;

    /// Write the manifest covering every program written so far.
    ///
    /// # Errors
    ///
    /// Sink-specific IO failures.
    fn write_manifest(&mut self, manifest: &CorpusManifest) -> Result<(), BenchError>;
}

/// The conventional committed layout: one `<family>-<seed>-<size>.g` file
/// per program under `root`, plus `root/corpus.json`
/// ([`CORPUS_DIR_MANIFEST`]).
#[derive(Debug, Clone)]
pub struct DirCorpusSink {
    /// Directory the corpus is materialized into.
    root: PathBuf,
}

impl DirCorpusSink {
    /// Sink writing into `root` (created on demand).
    #[must_use]
    pub fn new(root: &Path) -> Self {
        Self {
            root: root.to_path_buf(),
        }
    }
}

impl CorpusSink for DirCorpusSink {
    fn write_program(&mut self, program: &CorpusProgram) -> Result<(), BenchError> {
        std::fs::create_dir_all(&self.root).map_err(|err| BenchError::CorpusDirWrite {
            path: self.root.display().to_string(),
            detail: err.to_string(),
        })?;
        let path = self
            .root
            .join(corpus_file_name(program.family, program.seed, program.size));
        std::fs::write(&path, &program.source).map_err(|err| BenchError::CorpusDirWrite {
            path: path.display().to_string(),
            detail: err.to_string(),
        })
    }

    fn write_manifest(&mut self, manifest: &CorpusManifest) -> Result<(), BenchError> {
        std::fs::create_dir_all(&self.root).map_err(|err| BenchError::CorpusDirWrite {
            path: self.root.display().to_string(),
            detail: err.to_string(),
        })?;
        let path = self.root.join(CORPUS_DIR_MANIFEST);
        let json = manifest.to_json_pretty()?;
        std::fs::write(&path, json).map_err(|err| BenchError::CorpusDirWrite {
            path: path.display().to_string(),
            detail: err.to_string(),
        })
    }
}

/// The deterministic `.g` file name for one corpus program:
/// `<family>-<seed>-<size>.g`.
#[must_use]
pub fn corpus_file_name(family: CorpusFamily, seed: u64, size: usize) -> String {
    format!("{}-{seed}-{size}.g", family.as_str())
}

/// What to materialize: families × seeds at one size — the same triple
/// [`crate::corpus::CorpusManifest::build`] consumes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CorpusDirPlan {
    /// Families to generate (canonical order preserved).
    pub families: Vec<CorpusFamily>,
    /// Seeds to generate (order preserved).
    pub seeds: Vec<u64>,
    /// The programs' `size` parameter.
    pub size: usize,
}

impl CorpusDirPlan {
    /// Parse the CLI flags: comma-separated family names and seeds
    /// (`None` = every family / the default `0,1,2` seeds), and the size
    /// (validated against the substrate's `1..=MAX_SIZE` bound).
    ///
    /// # Errors
    ///
    /// [`BenchError::CorpusPlan`] for an unknown family, a non-`u64` or
    /// empty seed list, or an out-of-range size.
    pub fn from_flags(
        families: Option<&str>,
        seeds: Option<&str>,
        size: usize,
    ) -> Result<Self, BenchError> {
        let family_list = families.unwrap_or("");
        let families: Vec<CorpusFamily> = if family_list.is_empty() {
            CorpusFamily::ALL.to_vec()
        } else {
            family_list
                .split(',')
                .map(|name| {
                    CorpusFamily::parse(name.trim()).ok_or_else(|| BenchError::CorpusPlan {
                        detail: format!("unknown corpus family {name:?} (see glyim.corpus/1)"),
                    })
                })
                .collect::<Result<_, _>>()?
        };
        let seed_list = seeds.unwrap_or("0,1,2");
        let mut parsed_seeds: Vec<u64> = Vec::new();
        for seed in seed_list.split(',') {
            let seed = seed.trim();
            let Ok(seed) = seed.parse::<u64>() else {
                return Err(BenchError::CorpusPlan {
                    detail: format!("corpus seed {seed:?} is not a u64"),
                });
            };
            parsed_seeds.push(seed);
        }
        if families.is_empty() || parsed_seeds.is_empty() {
            return Err(BenchError::CorpusPlan {
                detail: "families and seeds must be non-empty".to_string(),
            });
        }
        if size == 0 || size > MAX_SIZE {
            return Err(BenchError::CorpusPlan {
                detail: format!("corpus size {size} outside 1..={MAX_SIZE}"),
            });
        }
        Ok(Self {
            families,
            seeds: parsed_seeds,
            size,
        })
    }
}

/// Materialize the plan through the sink: build the manifest, regenerate
/// every program, and write both faces.
///
/// # Errors
///
/// [`BenchError::CorpusPlan`] for an invalid plan (surfaced by the
/// substrate build), and the sink's write errors.
pub fn write_corpus_dir(
    sink: &mut dyn CorpusSink,
    plan: &CorpusDirPlan,
) -> Result<CorpusManifest, BenchError> {
    let span = tracing::info_span!(
        "corpus_dir_write",
        families = plan.families.len(),
        seeds = plan.seeds.len(),
        size = plan.size,
    );
    let _enter = span.enter();
    let manifest = CorpusManifest::build(&plan.families, &plan.seeds, plan.size)?;
    for entry in &manifest.programs {
        let program = generate(entry.family, entry.seed, entry.size)?;
        sink.write_program(&program)?;
    }
    sink.write_manifest(&manifest)?;
    tracing::debug!(
        programs = manifest.programs.len(),
        "corpus directory materialized"
    );
    Ok(manifest)
}

/// The read face of the committed-corpora seam: what
/// [`check_corpus_dir`] found when re-verifying committed bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CorpusDirCheck {
    /// The directory that was checked.
    pub dir: PathBuf,
    /// Number of programs the committed manifest declares.
    pub entries: usize,
    /// Program files the manifest declares but the directory lacks.
    pub missing: Vec<String>,
    /// Program files whose committed bytes no longer match the manifest
    /// identity (hand edits land here, not on disk silently).
    pub mismatched: Vec<String>,
}

impl CorpusDirCheck {
    /// Whether the committed directory matches the manifest exactly.
    #[must_use]
    pub fn is_ok(&self) -> bool {
        self.missing.is_empty() && self.mismatched.is_empty()
    }

    /// Fixed-width human-readable report (deterministic for a check).
    #[must_use]
    pub fn render_text(&self) -> String {
        let mut out = String::new();
        let _ = writeln!(out, "glyim-bench corpus dir check: {}", self.dir.display());
        let _ = writeln!(out, "  programs: {} program(s)", self.entries);
        let _ = writeln!(out, "  missing: {}", self.missing.len());
        for name in &self.missing {
            let _ = writeln!(out, "    {name}");
        }
        let _ = writeln!(out, "  mismatched: {}", self.mismatched.len());
        for name in &self.mismatched {
            let _ = writeln!(out, "    {name}");
        }
        out
    }
}

impl fmt::Display for CorpusDirCheck {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.render_text())
    }
}

/// Re-verify a committed corpora directory against its manifest.
///
/// The gate runs in three steps, each with its own failure face: the
/// manifest must load ([`BenchError::CorpusDirRead`]), the templates must
/// still regenerate every row's identity/exit/flags
/// ([`BenchError::CorpusIdentityMismatch`], the substrate's own contract),
/// and every committed `.g` file must exist and hash to its recorded
/// identity (reported as `missing`/`mismatched` rows — report content, so
/// the caller can render the full picture and flip the exit code, the
/// same shape as the diff/budget gates).
///
/// # Errors
///
/// [`BenchError::CorpusDirRead`] when the manifest is missing, unreadable,
/// or not a valid `glyim.corpus/1` document;
/// [`BenchError::CorpusIdentityMismatch`] / [`BenchError::CorpusPlan`]
/// when the template verify pass fails.
pub fn check_corpus_dir(root: &Path) -> Result<CorpusDirCheck, BenchError> {
    let span = tracing::info_span!("corpus_dir_check", dir = %root.display());
    let _enter = span.enter();

    let manifest_path = root.join(CORPUS_DIR_MANIFEST);
    let text =
        std::fs::read_to_string(&manifest_path).map_err(|err| BenchError::CorpusDirRead {
            path: manifest_path.display().to_string(),
            detail: err.to_string(),
        })?;
    let manifest = CorpusManifest::from_json(&text).map_err(|err| BenchError::CorpusDirRead {
        path: manifest_path.display().to_string(),
        detail: err.to_string(),
    })?;
    manifest.verify()?;

    let mut missing = Vec::new();
    let mut mismatched = Vec::new();
    for entry in &manifest.programs {
        let name = corpus_file_name(entry.family, entry.seed, entry.size);
        let path = root.join(&name);
        let Ok(content) = std::fs::read_to_string(&path) else {
            missing.push(name);
            continue;
        };
        if crate::corpus::fnv1a64(&content) != entry.identity {
            mismatched.push(name);
        }
    }
    tracing::debug!(
        entries = manifest.programs.len(),
        missing = missing.len(),
        mismatched = mismatched.len(),
        "corpus directory checked"
    );
    Ok(CorpusDirCheck {
        dir: root.to_path_buf(),
        entries: manifest.programs.len(),
        missing,
        mismatched,
    })
}
