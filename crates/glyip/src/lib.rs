//! Glyip — Cargo-like build tool for the Glyim compiler.
//!
//! Provides project scaffolding (`glyip new`), compilation (`glyip build`),
//! test execution (`glyip test`), and binary execution (`glyip run`),
//! with dependency resolution, incremental compilation via fingerprinting,
//! and crate caching.
// Stylistic clippy lints suppressed crate-wide (test-noise lints).

pub mod cache;
pub mod commands;
pub mod config;
pub mod dep;
pub mod error;
pub mod fingerprint;
pub mod lockfile;
pub mod test_discovery;

pub use cache::Cache;
pub use commands::{
    BuildResult, NewResult, RunResult, TestResult, cmd_build, cmd_new, cmd_run, cmd_test,
};
pub use config::{BuildOptions, GlyipToml, NewOptions, RunOptions, TestOptions};
pub use dep::{CrateIndex, DependencyResolver, IndexEntry, RegistryClient};

#[cfg(feature = "registry")]
pub use dep::HttpRegistryClient;
pub use error::{GlyipError, GlyipResult};
pub use fingerprint::{Fingerprint, FingerprintStore};
pub use lockfile::{CrateSource, LockedCrate, Lockfile};

#[cfg(test)]
mod tests;
