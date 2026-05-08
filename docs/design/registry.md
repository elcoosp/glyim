# 🧠 BMAD Creative Intelligence Suite — Adjusted Design

## Reality-Calibrated Architecture for Glyim Registry

*All agents reconvened to re-map the brainstorm against the existing codebase.*

---

# GAP ANALYSIS: What You Have vs. What You Need

## Existing Foundation Map

```
╔══════════════════════════════════════════════════════════════════════╗
║  WHAT ALREADY EXISTS                                                ║
╠══════════════════════════════════════════════════════════════════════╣
║                                                                      ║
║  glyim-cas-server                                                    ║
║  ├─ ✅ Content-addressable blob store (LocalContentStore)            ║
║  ├─ ✅ ContentHash (SHA-256 via glyim_macro_vfs)                    ║
║  ├─ ✅ REST API: store/retrieve/find-missing/action-results         ║
║  ├─ ✅ gRPC: Bazel RE CAS protocol (batch_update/batch_read)        ║
║  ├─ ✅ Wasm reproducibility verification (/verify-wasm)             ║
║  └─ ✅ Capabilities service (SHA-256 digest, API v2)                ║
║                                                                      ║
║  glyim-pkg                                                           ║
║  ├─ ✅ CAS client (local + remote with local cache)                 ║
║  ├─ ✅ Manifest parsing (glyim.toml: package/deps/macros/workspace) ║
║  ├─ ✅ Dependency resolver (minimal version selection)               ║
║  ├─ ✅ Lockfile generation/parsing (TOML)                           ║
║  ├─ ✅ Registry client (HTTP REST, fetch/publish)                   ║
║  ├─ ✅ Wasm publish workflow (compile → store → hash)               ║
║  ├─ ✅ Workspace detection (glob members)                           ║
║  └─ ✅ Feature flags support in manifest                             ║
║                                                                      ║
╚══════════════════════════════════════════════════════════════════════╝
```

## Critical Gaps — The Development Map

```
╔══════════════════════════════════════════════════════════════════════╗
║  WHAT NEEDS DEVELOPMENT                                             ║
╠══════════════════════════════════════════════════════════════════════╣
║                                                                      ║
║  🔴 CRITICAL (Architecture-blocking)                                ║
║  ├─ No package metadata/index layer (names → version lists → hashes)║
║  ├─ No temporal resolution (no append-only event log)               ║
║  ├─ No server-side resolution engine                                ║
║  ├─ No trust/signing infrastructure (no Sigstore, no PQ-crypto)     ║
║  ├─ No capability contracts in manifest                             ║
║  └─ No function-level granularity (no static analysis at publish)   ║
║                                                                      ║
║  🟡 IMPORTANT (Differentiating)                                     ║
║  ├─ No AI Oracle / intent search                                    ║
║  ├─ No auto-migration system                                        ║
║  ├─ No delta update support (no Merkle tree of blobs)               ║
║  ├─ No CRDT local mirror                                            ║
║  ├─ No progressive trust system                                     ║
║  ├─ No build provenance / SLSA attestation                          ║
║  ├─ No multi-platform artifact manifest                             ║
║  └─ No capability-addressed discovery                               ║
║                                                                      ║
║  🟢 POLISH (Delight)                                                ║
║  ├─ No package health genome / behavioral profiling                 ║
║  ├─ No interactive dependency graph                                  ║
║  ├─ No live playground                                              ║
║  ├─ No quadratic funding pool                                       ║
║  ├─ No AI maintenance assistant                                     ║
║  └─ No learning paths                                               ║
║                                                                      ║
╚══════════════════════════════════════════════════════════════════════╝
```

---

# DR. QUINN 🔬 — ROOT CAUSE: The Missing Index Layer

## Five Whys: Why can't the registry serve packages end-to-end?

```
WHY can't users install packages from the CAS server?
→ Because the CAS only stores blobs by hash, not by name+version
  WHY doesn't it store by name+version?
  → Because the CAS is content-addressed — names are a separate concern
    WHY is there no name→hash index?
      → Because no index service has been built yet
        WHY not?
          → Because the existing code separates CAS from registry,
            but the registry client (glyim-pkg/registry.rs) talks to
            a hypothetical REST API that doesn't exist on the CAS server
            ROOT CAUSE: The CAS server and the package index are not connected.
                        There is no index service that maps:
                        package name + version → content hash → CAS blob
```

**This is the #1 blocker. Everything else builds on top of an index layer.**

---

# ADJUSTED ARCHITECTURE: The Glyim Registry

## What Changes from the Original Brainstorm

| Original Feature | Status | Adjustment |
|---|---|---|
| CAS Store | ✅ **Already exists** | Extend, don't rebuild. Add Merkle tree blob layout + delta support |
| Function-Level Granularity | 🟡 Needs new module | Build as publish-time static analysis → stores per-export blobs in CAS |
| AI Oracle | 🟡 Needs new service | Standalone service, queries the index + CAS |
| Trust Framework | 🔴 Needs major new code | Add to CAS server as middleware + new attestation crate |
| Temporal Resolution | 🔴 Needs new event log | Append-only log in CAS server, new resolver mode in glyim-pkg |
| Capability Discovery | 🔴 Needs new index | Add capability declarations to manifest, index in CAS server |
| Auto-Migration | 🟡 Needs new service | AI-assisted, operates on version diffs from index |
| Zero-Install Streaming | 🟡 Needs runtime support | Not a pkg concern — belongs in the Glyim runtime |
| CRDT Local Mirror | 🟡 Needs new module | New crate: glyim-mirror |
| Delta Updates | 🟡 Extends CAS | Store blobs as Merkle tree chunks, add delta endpoints |
| Pre-Computed Resolution | 🔴 Needs new engine | Server-side resolver that caches resolution results in CAS |
| Progressive Trust | 🔴 Needs new system | Trust scoring service + CAS middleware |
| Multi-Platform Manifest | 🟡 Extends manifest | Add `[artifacts]` section to glyim.toml |
| Build Provenance | 🔴 Needs new service | SLSA attestation generation + verification |

---

# DEVELOPMENT PLAN: 7 New Modules

## New Crate Structure

```
glyim-workspace/
├── crates/
│   ├── glyim-cas-server/          ← EXISTS — extend
│   ├── glyim-pkg/                 ← EXISTS — extend
│   ├── glyim-macro-vfs/           ← EXISTS — foundation
│   ├── glyim-codegen-llvm/        ← EXISTS — foundation
│   ├── glyim-interner/            ← EXISTS — foundation
│   │
│   ├── glyim-registry-index/      ← NEW — package name→hash index
│   ├── glyim-registry-events/     ← NEW — append-only event log
│   ├── glyim-registry-resolver/   ← NEW — server-side resolution engine
│   ├── glyim-registry-trust/      ← NEW — trust, signing, attestation
│   ├── glyim-registry-capabilities/ ← NEW — capability contracts + discovery
│   ├── glyim-registry-granularity/  ← NEW — function-level splitting
│   └── glyim-mirror/              ← NEW — CRDT offline mirror
```

---

## MODULE 1: glyim-registry-index 🔴 CRITICAL

**The missing link between names and content hashes.**

### What It Does

Maps `package name + version → content hash → CAS blob reference`. This is the index that makes the CAS server actually useful as a package registry.

### Why It's Needed

Currently:
- `glyim-cas-server` stores blobs by hash ✅
- `glyim-pkg/registry.rs` expects a REST API at `/api/v1/packages/{name}` ✅
- **But the CAS server doesn't serve that API** ❌
- The registry client and CAS server are disconnected ❌

### New Files

```
glyim-registry-index/
├── Cargo.toml
└── src/
    ├── lib.rs
    ├── index.rs          — PackageIndex struct + CRUD
    ├── version_entry.rs  — VersionEntry (version, hash, deps, capabilities, trust_level)
    ├── search.rs         — Name search, capability search, full-text search
    └── snapshot.rs       — Point-in-time snapshots for temporal resolution
```

### Core Types

```rust
// ── src/version_entry.rs ──────────────────────────────────────

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// A single published version of a package in the index.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VersionEntry {
    /// Semver version string
    pub version: String,
    /// Content hash of the package archive in the CAS
    pub content_hash: String,
    /// Content hash of the package's manifest (glyim.toml)
    pub manifest_hash: String,
    /// Whether this version contains macro code (Wasm)
    pub is_macro: bool,
    /// Direct dependencies
    pub deps: Vec<IndexDependency>,
    /// Declared capabilities (for capability-addressed discovery)
    pub capabilities: Vec<String>,
    /// Trust level of this version
    pub trust_level: TrustLevel,
    /// Platform-specific artifact hashes
    pub artifacts: HashMap<String, String>,
    /// Timestamp of publication
    pub published_at: chrono::DateTime<chrono::Utc>,
    /// Publisher identity hash
    pub publisher: String,
    /// SLSA provenance attestation hash (if available)
    pub provenance_hash: Option<String>,
    /// Whether this version is yanked
    pub yanked: bool,
    /// Deprecation message (if deprecated)
    pub deprecated: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub enum TrustLevel {
    /// Just published, minimal verification
    New,
    /// Passes automated audit, >100 downloads, >7 days old
    Established,
    /// Full audit suite, reproducible build, community attestation
    Verified,
    /// Formal verification of critical paths, manual security review
    Certified,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexDependency {
    pub name: String,
    pub version_constraint: String,
    pub is_macro: bool,
}
```

```rust
// ── src/index.rs ──────────────────────────────────────────────

use crate::version_entry::{VersionEntry, TrustLevel};
use crate::snapshot::Snapshot;
use std::collections::HashMap;

/// The package index — maps package names to their version entries.
/// Backed by the CAS for persistence.
pub struct PackageIndex {
    /// In-memory index: name → sorted version entries
    packages: HashMap<String, Vec<VersionEntry>>,
    /// CAS client for persistence
    cas: glyim_macro_vfs::LocalContentStore,
    /// Append-only event log reference
    event_log: crate::snapshot::EventLog,
}

impl PackageIndex {
    /// Load or create the index, backed by CAS storage.
    pub fn open(cas_dir: &std::path::Path) -> std::io::Result<Self> {
        let cas = glyim_macro_vfs::LocalContentStore::new(cas_dir)?;
        let mut index = Self {
            packages: HashMap::new(),
            cas,
            event_log: crate::snapshot::EventLog::open(cas_dir.join("events"))?,
        };
        index.replay_events()?;
        Ok(index)
    }

    /// Publish a new version. Returns the event ID.
    pub fn publish(&mut self, name: &str, entry: VersionEntry) -> Result<u64, IndexError> {
        // Validate: version must not already exist
        if let Some(versions) = self.packages.get(name) {
            if versions.iter().any(|v| v.version == entry.version) {
                return Err(IndexError::VersionAlreadyExists {
                    name: name.to_string(),
                    version: entry.version,
                });
            }
        }

        // Append to event log first (write-ahead log)
        let event_id = self.event_log.append_publish(name, &entry)?;

        // Then update in-memory index
        self.packages
            .entry(name.to_string())
            .or_default()
            .push(entry);

        // Sort versions by semver (descending for easy latest access)
        if let Some(versions) = self.packages.get_mut(name) {
            versions.sort_by(|a, b| {
                semver::Version::parse(&b.version)
                    .unwrap_or_default()
                    .cmp(&semver::Version::parse(&a.version).unwrap_or_default())
            });
        }

        Ok(event_id)
    }

    /// Yank a version (mark as do-not-use).
    pub fn yank(&mut self, name: &str, version: &str) -> Result<u64, IndexError> {
        let entry = self
            .packages
            .get_mut(name)
            .and_then(|versions| versions.iter_mut().find(|v| v.version == version))
            .ok_or(IndexError::VersionNotFound {
                name: name.to_string(),
                version: version.to_string(),
            })?;

        entry.yanked = true;
        let event_id = self.event_log.append_yank(name, version)?;
        Ok(event_id)
    }

    /// Get all versions for a package (excluding yanked by default).
    pub fn get_versions(&self, name: &str) -> Vec<&VersionEntry> {
        self.packages
            .get(name)
            .map(|versions| versions.iter().filter(|v| !v.yanked).collect())
            .unwrap_or_default()
    }

    /// Get a specific version entry.
    pub fn get_version(&self, name: &str, version: &str) -> Option<&VersionEntry> {
        self.packages
            .get(name)?
            .iter()
            .find(|v| v.version == version && !v.yanked)
    }

    /// Get the index state as it existed at a given timestamp.
    /// This enables temporal resolution.
    pub fn snapshot_at(&self, timestamp: chrono::DateTime<chrono::Utc>) -> Snapshot {
        self.event_log.replay_until(timestamp)
    }

    /// Search packages by name prefix (fast path).
    pub fn search_by_name(&self, prefix: &str) -> Vec<&str> {
        self.packages
            .keys()
            .filter(|k| k.starts_with(prefix))
            .map(|k| k.as_str())
            .collect()
    }

    /// Search packages by capability (slower path, uses capability index).
    pub fn search_by_capability(&self, capability: &str) -> Vec<(&str, &VersionEntry)> {
        self.packages
            .iter()
            .flat_map(|(name, versions)| {
                versions
                    .iter()
                    .filter(|v| v.capabilities.contains(&capability.to_string()))
                    .map(|v| (name.as_str(), v))
            })
            .collect()
    }

    /// Replay the event log to rebuild the in-memory index after restart.
    fn replay_events(&mut self) -> std::io::Result<()> {
        let snapshot = self.event_log.replay_all()?;
        self.packages = snapshot.into_packages();
        Ok(())
    }
}

#[derive(Debug, thiserror::Error)]
pub enum IndexError {
    #[error("version {version} already exists for package {name}")]
    VersionAlreadyExists { name: String, version: String },
    #[error("version {version} not found for package {name}")]
    VersionNotFound { name: String, version: String },
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}
```

```rust
// ── src/snapshot.rs ───────────────────────────────────────────

use crate::version_entry::VersionEntry;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::{BufRead, Write};

/// Append-only event log for temporal resolution.
///
/// Every mutation to the index (publish, yank, deprecate) is recorded
/// as an event. The log can be replayed to any point in time, enabling
/// `resolve-as-of(timestamp)` without lockfiles.
pub struct EventLog {
    log_file: std::fs::File,
    next_id: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    pub id: u64,
    pub timestamp: String, // ISO 8601
    pub kind: EventKind,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum EventKind {
    Publish { name: String, entry: VersionEntry },
    Yank { name: String, version: String },
    Unyank { name: String, version: String },
    Deprecate { name: String, version: String, message: String },
    Transfer { name: String, from_publisher: String, to_publisher: String },
}

/// A point-in-time snapshot of the index.
pub struct Snapshot {
    packages: HashMap<String, Vec<VersionEntry>>,
}

impl Snapshot {
    pub fn into_packages(self) -> HashMap<String, Vec<VersionEntry>> {
        self.packages
    }

    pub fn is_empty(&self) -> bool {
        self.packages.is_empty()
    }
}

impl EventLog {
    pub fn open(dir: std::path::PathBuf) -> std::io::Result<Self> {
        std::fs::create_dir_all(&dir)?;
        let path = dir.join("event_log.jsonl");
        let file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .read(true)
            .open(&path)?;

        // Count existing events to set next_id
        let next_id = {
            let reader = std::io::BufReader::new(&file);
            reader.lines().count() as u64
        };

        Ok(Self { log_file: file, next_id })
    }

    pub fn append_publish(&mut self, name: &str, entry: &VersionEntry) -> std::io::Result<u64> {
        let event = Event {
            id: self.next_id,
            timestamp: chrono::Utc::now().to_rfc3339(),
            kind: EventKind::Publish {
                name: name.to_string(),
                entry: entry.clone(),
            },
        };
        self.write_event(&event)?;
        self.next_id += 1;
        Ok(event.id)
    }

    pub fn append_yank(&mut self, name: &str, version: &str) -> std::io::Result<u64> {
        let event = Event {
            id: self.next_id,
            timestamp: chrono::Utc::now().to_rfc3339(),
            kind: EventKind::Yank {
                name: name.to_string(),
                version: version.to_string(),
            },
        };
        self.write_event(&event)?;
        self.next_id += 1;
        Ok(event.id)
    }

    /// Replay all events from the beginning.
    pub fn replay_all(&self) -> std::io::Result<Snapshot> {
        self.replay_until(chrono::Utc::now())
    }

    /// Replay events up to a given timestamp.
    pub fn replay_until(&self, timestamp: chrono::DateTime<chrono::Utc>) -> std::io::Result<Snapshot> {
        let mut packages: HashMap<String, Vec<VersionEntry>> = HashMap::new();

        let file = std::fs::File::open(self.log_file.path().ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::NotFound, "no log path")
        })?)?;
        let reader = std::io::BufReader::new(file);

        for line in reader.lines() {
            let line = line?;
            let event: Event = serde_json::from_str(&line)
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;

            let event_time = chrono::DateTime::parse_from_rfc3339(&event.timestamp)
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;

            if event_time.naive_utc() > timestamp.naive_utc() {
                break; // Stop at the requested timestamp
            }

            match event.kind {
                EventKind::Publish { name, entry } => {
                    packages.entry(name).or_default().push(entry);
                }
                EventKind::Yank { name, version } => {
                    if let Some(versions) = packages.get_mut(&name) {
                        if let Some(v) = versions.iter_mut().find(|v| v.version == version) {
                            v.yanked = true;
                        }
                    }
                }
                EventKind::Unyank { name, version } => {
                    if let Some(versions) = packages.get_mut(&name) {
                        if let Some(v) = versions.iter_mut().find(|v| v.version == version) {
                            v.yanked = false;
                        }
                    }
                }
                EventKind::Deprecate { name, version, message } => {
                    if let Some(versions) = packages.get_mut(&name) {
                        if let Some(v) = versions.iter_mut().find(|v| v.version == version) {
                            v.deprecated = Some(message);
                        }
                    }
                }
                EventKind::Transfer { name, from_publisher: _, to_publisher } => {
                    if let Some(versions) = packages.get_mut(&name) {
                        for v in versions.iter_mut() {
                            v.publisher = to_publisher.clone();
                        }
                    }
                }
            }
        }

        Ok(Snapshot { packages })
    }

    fn write_event(&mut self, event: &Event) -> std::io::Result<()> {
        let json = serde_json::to_string(event)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        writeln!(self.log_file, "{}", json)?;
        self.log_file.flush()?;
        Ok(())
    }
}
```

```rust
// ── src/search.rs ─────────────────────────────────────────────

use crate::index::PackageIndex;
use crate::version_entry::VersionEntry;

/// Search result with relevance scoring.
pub struct SearchResult {
    pub name: String,
    pub latest_version: String,
    pub description: Option<String>,
    pub trust_level: String,
    pub relevance: f64,
}

impl PackageIndex {
    /// Full search: name prefix + capability matching + fuzzy name matching.
    pub fn search(&self, query: &str) -> Vec<SearchResult> {
        let mut results: Vec<SearchResult> = Vec::new();

        // 1. Exact name match (highest relevance)
        if let Some(versions) = self.packages.get(query) {
            if let Some(latest) = versions.first() {
                results.push(SearchResult {
                    name: query.to_string(),
                    latest_version: latest.version.clone(),
                    description: None, // TODO: from manifest
                    trust_level: format!("{:?}", latest.trust_level),
                    relevance: 1.0,
                });
            }
        }

        // 2. Name prefix match
        for (name, versions) in &self.packages {
            if name.starts_with(query) && name != query {
                if let Some(latest) = versions.first() {
                    results.push(SearchResult {
                        name: name.clone(),
                        latest_version: latest.version.clone(),
                        description: None,
                        trust_level: format!("{:?}", latest.trust_level),
                        relevance: 0.8 * (query.len() as f64 / name.len() as f64),
                    });
                }
            }
        }

        // 3. Capability match
        for (name, versions) in &self.packages {
            if let Some(latest) = versions.first() {
                if latest.capabilities.iter().any(|c| c.contains(query)) {
                    // Don't duplicate if already in results
                    if !results.iter().any(|r| r.name == *name) {
                        results.push(SearchResult {
                            name: name.clone(),
                            latest_version: latest.version.clone(),
                            description: None,
                            trust_level: format!("{:?}", latest.trust_level),
                            relevance: 0.5,
                        });
                    }
                }
            }
        }

        // Sort by relevance descending
        results.sort_by(|a, b| b.relevance.partial_cmp(&a.relevance).unwrap());
        results
    }
}
```

### What This Unlocks

- The CAS server can now serve the `/api/v1/packages/{name}` endpoint that `glyim-pkg/registry.rs` already expects
- Temporal resolution becomes possible via `snapshot_at()`
- Capability-addressed discovery becomes possible via `search_by_capability()`
- Progressive trust levels are tracked per-version
- The event log provides the foundation for CRDT sync later

### Integration with CAS Server

Add these routes to `glyim-cas-server/src/main.rs`:

```rust
// ── New routes to add to the REST router ─────────────────────

.route("/api/v1/packages/{name}", get(index::get_package))
.route("/api/v1/packages/{name}/{version}", get(index::get_version))
.route("/api/v1/packages/{name}/{version}/upload", post(index::publish_version))
.route("/api/v1/packages/{name}/{version}/yank", post(index::yank_version))
.route("/api/v1/search", get(index::search_packages))
.route("/api/v1/snapshot", get(index::snapshot_at_timestamp))
```

---

## MODULE 2: glyim-registry-events 🔴 CRITICAL

**Append-only event log for temporal resolution.**

### What It Does

Already designed above as part of `glyim-registry-index/src/snapshot.rs`. However, for production use, this should be a standalone crate that provides:

1. **Durable append-only log** with segment rotation
2. **Checkpoint snapshots** (don't replay from epoch every time)
3. **Efficient time-range queries** (binary search on timestamps)
4. **CRDT-compatible format** (for future mirror sync)

### Key Decisions

| Decision | Choice | Rationale |
|---|---|---|
| Storage format | JSONL (JSON Lines) | Human-readable, easy to debug, append-friendly |
| Segment rotation | 64MB per segment | Balance between file count and replay speed |
| Checkpoint frequency | Every 10,000 events | Full replay takes <1s for 10K events |
| Compaction | Never delete, only compress | Audit trail is permanent |

### Development Work

```
glyim-registry-events/
├── Cargo.toml
└── src/
    ├── lib.rs
    ├── log.rs          — Segment-based append-only log
    ├── checkpoint.rs   — Snapshot checkpoints for fast startup
    └── query.rs        — Time-range queries with binary search
```

> **Note:** The `snapshot.rs` code above in Module 1 is a working prototype. This crate extracts and hardens it for production.

---

## MODULE 3: glyim-registry-resolver 🔴 CRITICAL

**Server-side dependency resolution engine.**

### What It Does

Currently, `glyim-pkg/src/resolver.rs` does client-side resolution. This is fine for small projects but doesn't scale. Server-side resolution provides:

1. **Pre-computed optimal resolution** — server has global visibility
2. **Cached resolution results** — same dependency set = same result, stored in CAS
3. **Constraint programming solver** — optimal, not just minimal
4. **Resolution proofs** — verifiable proof that the resolution is optimal

### Why It's Needed

The existing resolver has these limitations:
- Only supports exact, caret, and wildcard constraints (no ranges, no tilde)
- No conflict resolution strategy beyond "error"
- No diamond dependency handling
- No resolution caching
- Minimal version selection only (not always optimal for security/size)

### New Files

```
glyim-registry-resolver/
├── Cargo.toml
└── src/
    ├── lib.rs
    ├── solver.rs       — Constraint programming solver (backtracking + heuristics)
    ├── cache.rs        — Resolution result cache (stored in CAS)
    ├── proof.rs        — Resolution proof generation + verification
    ├── constraints.rs  — Extended constraint types (range, tilde, comparators)
    └── compat.rs       — Compatibility matrix computation
```

### Core Types

```rust
// ── src/constraints.rs ────────────────────────────────────────

/// Extended version constraint types beyond what glyim-pkg currently supports.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VersionConstraint {
    /// Exact: "1.2.3"
    Exact(semver::Version),
    /// Caret: "^1.2.3" — >=1.2.3, <2.0.0
    Caret(semver::Version),
    /// Tilde: "~1.2.3" — >=1.2.3, <1.3.0
    Tilde(semver::Version),
    /// Range: ">=1.2.3, <2.0.0"
    Range { min: semver::Version, max: semver::Version },
    /// Wildcard: "*"
    Wildcard,
    /// Comparator set: ">=1.0.0 <2.0.0 || >=3.0.0"
    ComparatorSet(Vec<Comparator>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Comparator {
    pub op: CmpOp,
    pub version: semver::Version,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CmpOp {
    Exact,  // =
    Gt,     // >
    Gte,    // >=
    Lt,     // <
    Lte,    // <=
    Ne,     // !=
}
```

```rust
// ── src/solver.rs ─────────────────────────────────────────────

use crate::constraints::VersionConstraint;
use crate::cache::ResolutionCache;
use std::collections::HashMap;

/// Server-side dependency resolver with constraint programming.
pub struct ServerResolver {
    index: glyim_registry_index::PackageIndex,
    cache: ResolutionCache,
}

/// A resolution request from a client.
pub struct ResolutionRequest {
    /// Root dependencies
    pub dependencies: HashMap<String, VersionConstraint>,
    /// Resolve as of this timestamp (temporal resolution)
    pub resolve_as_of: Option<chrono::DateTime<chrono::Utc>>,
    /// Optimization objective
    pub objective: ResolutionObjective,
}

/// What to optimize for during resolution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolutionObjective {
    /// Minimal versions (like current glyim-pkg resolver)
    MinimalVersions,
    /// Latest compatible versions (better security patches)
    LatestVersions,
    /// Smallest total download size
    MinimalSize,
    /// Highest trust scores
    MaximalTrust,
    /// Fewest transitive dependencies
    MinimalDeps,
}

/// A complete resolution result.
pub struct ResolutionResult {
    /// Resolved packages: name → (version, content_hash)
    pub packages: HashMap<String, ResolvedVersion>,
    /// Content hash of this resolution (for caching)
    pub resolution_hash: String,
    /// Proof that this resolution is valid and optimal
    pub proof: ResolutionProof,
    /// Total estimated download size
    pub total_size_bytes: u64,
    /// Time taken to resolve
    pub resolve_time_ms: u64,
}

pub struct ResolvedVersion {
    pub version: String,
    pub content_hash: String,
    pub is_macro: bool,
    pub deps: Vec<String>,
    pub trust_level: String,
}

impl ServerResolver {
    pub fn new(index: glyim_registry_index::PackageIndex, cas_dir: &std::path::Path) -> Self {
        let cache = ResolutionCache::new(cas_dir);
        Self { index, cache }
    }

    /// Resolve dependencies server-side.
    pub fn resolve(&self, request: ResolutionRequest) -> Result<ResolutionResult, ResolverError> {
        let start = std::time::Instant::now();

        // Check cache first
        let cache_key = self.compute_cache_key(&request);
        if let Some(cached) = self.cache.get(&cache_key)? {
            return Ok(cached);
        }

        // Get the appropriate snapshot (temporal or current)
        let index_view = match request.resolve_as_of {
            Some(ts) => self.index.snapshot_at(ts),
            None => self.index.snapshot_current(),
        };

        // Solve using backtracking with heuristics
        let solution = self.solve(&request.dependencies, &index_view, &request.objective)?;

        let result = ResolutionResult {
            packages: solution,
            resolution_hash: cache_key,
            proof: ResolutionProof::placeholder(), // TODO: generate actual proof
            total_size_bytes: 0, // TODO: compute from CAS blob sizes
            resolve_time_ms: start.elapsed().as_millis() as u64,
        };

        // Cache the result
        self.cache.put(&cache_key, &result)?;

        Ok(result)
    }

    fn compute_cache_key(&self, request: &ResolutionRequest) -> String {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        for (name, constraint) in request.dependencies.iter() {
            hasher.update(name.as_bytes());
            hasher.update(format!("{:?}", constraint).as_bytes());
        }
        if let Some(ts) = &request.resolve_as_of {
            hasher.update(ts.to_rfc3339().as_bytes());
        }
        hasher.update(format!("{:?}", request.objective).as_bytes());
        hex::encode(hasher.finalize())
    }

    fn solve(
        &self,
        deps: &HashMap<String, VersionConstraint>,
        index_view: &glyim_registry_index::snapshot::Snapshot,
        objective: &ResolutionObjective,
    ) -> Result<HashMap<String, ResolvedVersion>, ResolverError> {
        // TODO: Full constraint programming solver
        // For now, use the same minimal version selection as glyim-pkg
        // but with access to the full index
        
        let mut resolved: HashMap<String, ResolvedVersion> = HashMap::new();
        let mut queue: Vec<(String, VersionConstraint)> = deps.iter()
            .map(|(n, c)| (n.clone(), c.clone()))
            .collect();

        while let Some((name, constraint)) = queue.pop() {
            if resolved.contains_key(&name) {
                continue;
            }

            let versions = index_view.get_versions(&name)
                .ok_or_else(|| ResolverError::PackageNotFound(name.clone()))?;

            let selected = versions.iter()
                .filter(|v| constraint.satisfies(&v.version))
                .max_by(|a, b| match objective {
                    ResolutionObjective::MinimalVersions => {
                        semver::Version::parse(&a.version)
                            .unwrap_or_default()
                            .cmp(&semver::Version::parse(&b.version).unwrap_or_default())
                    }
                    ResolutionObjective::LatestVersions => {
                        semver::Version::parse(&b.version)
                            .unwrap_or_default()
                            .cmp(&semver::Version::parse(&a.version).unwrap_or_default())
                    }
                    ResolutionObjective::MaximalTrust => {
                        a.trust_level.cmp(&b.trust_level)
                    }
                    _ => {
                        semver::Version::parse(&b.version)
                            .unwrap_or_default()
                            .cmp(&semver::Version::parse(&a.version).unwrap_or_default())
                    }
                })
                .ok_or_else(|| ResolverError::NoSatisfyingVersion {
                    name: name.clone(),
                    constraint: format!("{:?}", constraint),
                })?;

            // Queue transitive dependencies
            for dep in &selected.deps {
                if !resolved.contains_key(&dep.name) {
                    queue.push((
                        dep.name.clone(),
                        VersionConstraint::parse(&dep.version_constraint)?,
                    ));
                }
            }

            resolved.insert(name, ResolvedVersion {
                version: selected.version.clone(),
                content_hash: selected.content_hash.clone(),
                is_macro: selected.is_macro,
                deps: selected.deps.iter().map(|d| d.name.clone()).collect(),
                trust_level: format!("{:?}", selected.trust_level),
            });
        }

        Ok(resolved)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ResolverError {
    #[error("package not found: {0}")]
    PackageNotFound(String),
    #[error("no version of '{name}' satisfies constraint {constraint}")]
    NoSatisfyingVersion { name: String, constraint: String },
    #[error("cache error: {0}")]
    Cache(String),
    #[error("constraint parse error: {0}")]
    ConstraintParse(String),
}
```

### Integration with glyim-pkg

Add a new resolution mode to the existing resolver:

```rust
// ── Add to glyim-pkg/src/resolver.rs ─────────────────────────

/// Resolve dependencies using the server-side resolver.
/// Falls back to local resolution if the server is unavailable.
pub fn resolve_remote(
    root_deps: &[Requirement],
    registry_url: &str,
    resolve_as_of: Option<String>,
    objective: &str,
) -> Result<Resolution, PkgError> {
    let client = crate::registry::RegistryClient::new(registry_url)?;

    let request = serde_json::json!({
        "dependencies": root_deps.iter().map(|r| (r.name.clone(), r.version_constraint.clone())).collect::<HashMap<_,_>>(),
        "resolve_as_of": resolve_as_of,
        "objective": objective,
    });

    let response = client.resolve(&request)?;
    // Convert response to Resolution struct
    // ...
    todo!()
}
```

---

## MODULE 4: glyim-registry-trust 🔴 CRITICAL

**Trust, signing, provenance, and progressive trust system.**

### What It Does

The CAS server already has a `/verify-wasm` endpoint that checks Wasm reproducibility. This module extends that foundation into a full trust framework:

1. **Cryptographic signing** of published artifacts
2. **SLSA provenance** generation and verification
3. **Progressive trust** scoring (New → Established → Verified → Certified)
4. **Attestation consensus** (multi-replica build verification)
5. **Post-quantum signature** readiness (CRYSTALS-Dilithium)

### New Files

```
glyim-registry-trust/
├── Cargo.toml
└── src/
    ├── lib.rs
    ├── signing.rs       — Artifact signing + verification (Ed25519 + Dilithium ready)
    ├── provenance.rs    — SLSA Level 4 provenance generation + verification
    ├── trust_score.rs   — Progressive trust scoring engine
    ├── attestation.rs   — Multi-replica build attestation consensus
    └── audit.rs         — Automated audit: static analysis + fuzz test runner
```

### Core Types

```rust
// ── src/signing.rs ────────────────────────────────────────────

use ed25519_dalek::{SigningKey, VerifyingKey, Signature, Signer, Verifier};
use glyim_macro_vfs::ContentHash;

/// A signed artifact in the registry.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SignedArtifact {
    /// The content hash of the artifact
    pub content_hash: String,
    /// Ed25519 signature over the content hash
    pub signature: Vec<u8>,
    /// The public key of the signer
    pub signer_public_key: Vec<u8>,
    /// When this signature was created
    pub signed_at: String,
    /// What this signature attests to
    pub attestation: AttestationKind,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum AttestationKind {
    /// The author published this artifact
    AuthorPublish,
    /// A CI system built this artifact from source
    CIBuild { pipeline_id: String, commit_sha: String },
    /// An auditor reviewed this artifact
    AuditReview { auditor_id: String },
    /// A reproducible build verification passed
    ReproducibleBuild { builder_id: String },
}

/// Sign a content hash with an Ed25519 key.
pub fn sign_hash(
    hash: &ContentHash,
    signing_key: &SigningKey,
    attestation: AttestationKind,
) -> SignedArtifact {
    let signature = signing_key.sign(hash.to_hex().as_bytes());
    SignedArtifact {
        content_hash: hash.to_hex(),
        signature: signature.to_bytes().to_vec(),
        signer_public_key: signing_key.verifying_key().to_bytes().to_vec(),
        signed_at: chrono::Utc::now().to_rfc3339(),
        attestation,
    }
}

/// Verify a signed artifact.
pub fn verify_signature(artifact: &SignedArtifact) -> bool {
    let verifying_key = match VerifyingKey::from_bytes(
        &artifact.signer_public_key.clone().try_into().unwrap_or([0u8; 32])
    ) {
        Ok(key) => key,
        Err(_) => return false,
    };

    let signature = match Signature::from_slice(&artifact.signature) {
        Ok(sig) => sig,
        Err(_) => return false,
    };

    verifying_key
        .verify(artifact.content_hash.as_bytes(), &signature)
        .is_ok()
}
```

```rust
// ── src/trust_score.rs ────────────────────────────────────────

use glyim_registry_index::version_entry::TrustLevel;

/// Compute the trust level for a package version based on available evidence.
pub struct TrustScorer;

impl TrustScorer {
    /// Score a package version based on multiple signals.
    pub fn score(signals: &TrustSignals) -> TrustLevel {
        let mut points: u32 = 0;

        // Age-based trust
        if signals.age_days >= 365 {
            points += 20;
        } else if signals.age_days >= 30 {
            points += 10;
        } else if signals.age_days >= 7 {
            points += 5;
        }

        // Download count
        if signals.download_count >= 10000 {
            points += 15;
        } else if signals.download_count >= 100 {
            points += 5;
        }

        // Number of distinct publishers/contributors
        if signals.publisher_count >= 3 {
            points += 10;
        } else if signals.publisher_count >= 2 {
            points += 5;
        }

        // Automated audit passed
        if signals.audit_passed {
            points += 15;
        }

        // Reproducible build verified
        if signals.reproducible_build {
            points += 15;
        }

        // Has author signature
        if signals.author_signed {
            points += 5;
        }

        // Has CI build attestation
        if signals.ci_attested {
            points += 10;
        }

        // Community attestation (peer review)
        if signals.community_attestations >= 3 {
            points += 10;
        } else if signals.community_attestations >= 1 {
            points += 5;
        }

        // No known vulnerabilities
        if !signals.has_known_vulnerabilities {
            points += 10;
        }

        // Formal verification (highest bar)
        if signals.formally_verified {
            points += 20;
        }

        // Map points to trust level
        match points {
            0..=29 => TrustLevel::New,
            30..=59 => TrustLevel::Established,
            60..=89 => TrustLevel::Verified,
            _ => TrustLevel::Certified,
        }
    }
}

pub struct TrustSignals {
    pub age_days: u32,
    pub download_count: u64,
    pub publisher_count: u32,
    pub audit_passed: bool,
    pub reproducible_build: bool,
    pub author_signed: bool,
    pub ci_attested: bool,
    pub community_attestations: u32,
    pub has_known_vulnerabilities: bool,
    pub formally_verified: bool,
}
```

### Integration with CAS Server

Extend the `verify_wasm` endpoint to also produce trust attestations:

```rust
// ── Extend glyim-cas-server/src/verify.rs ────────────────────

/// After Wasm verification passes, generate a ReproducibleBuild attestation
/// and store it alongside the blob in the CAS.
pub async fn verify_wasm_and_attest(
    State(state): State<Arc<AppState>>,
    Json(req): Json<VerifyWasmRequest>,
) -> impl IntoResponse {
    // ... existing verification logic ...

    if matches {
        // NEW: Generate attestation
        let attestation = glyim_registry_trust::signing::sign_hash(
            &actual_hash,
            &state.signing_key,
            glyim_registry_trust::signing::AttestationKind::ReproducibleBuild {
                builder_id: "glyim-cas-server-1".to_string(),
            },
        );

        // Store attestation in CAS
        let attestation_json = serde_json::to_vec(&attestation).unwrap();
        let attestation_hash = state.store.write().await.store(&attestation_json);

        // NEW: Update trust score
        // ...
    }
    // ...
}
```

---

## MODULE 5: glyim-registry-capabilities 🟡 IMPORTANT

**Capability contracts and capability-addressed discovery.**

### What It Does

Adds formal capability declarations to packages and enables discovery by capability rather than name.

### Changes to Existing Manifest

```toml
# ── Extended glyim.toml ───────────────────────────────────────

[package]
name = "serenity-json"
version = "3.1.4"

# NEW: Declared capabilities
[package.capabilities]
provides = [
    "json.parse",
    "json.stringify",
    "json.validate",
    "json.stream",
    "json.schema",
]
requires = [  # OS/runtime capabilities this package needs
    # None for a pure JSON parser
]

# NEW: Runtime sandbox constraints
[package.sandbox]
max_memory = "64MB"
max_cpu_time = "30s"
network = false
filesystem = false
```

### New Files

```
glyim-registry-capabilities/
├── Cargo.toml
└── src/
    ├── lib.rs
    ├── contract.rs     — Capability contract definition + verification
    ├── index.rs        — Capability inverted index (capability → packages)
    ├── match.rs        — Fuzzy capability matching algorithm
    └── sandbox.rs      — Sandbox policy generation from capabilities
```

### Integration with glyim-pkg Manifest

Extend the existing `PackageManifest`:

```rust
// ── Add to glyim-pkg/src/manifest.rs ─────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct Package {
    pub name: String,
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub no_std: Option<bool>,
    #[serde(default)]
    pub edition: String,
    #[serde(default)]
    pub authors: Vec<String>,
    #[serde(default)]
    pub description: Option<String>,
    // NEW:
    #[serde(default)]
    pub capabilities: Option<CapabilitiesConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct CapabilitiesConfig {
    #[serde(default)]
    pub provides: Vec<String>,
    #[serde(default)]
    pub requires: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct SandboxConfig {
    #[serde(default)]
    pub max_memory: Option<String>,
    #[serde(default)]
    pub max_cpu_time: Option<String>,
    #[serde(default)]
    pub network: bool,
    #[serde(default)]
    pub filesystem: bool,
}
```

---

## MODULE 6: glyim-registry-granularity 🟡 IMPORTANT

**Function-level package splitting at publish time.**

### What It Does

When a package is published, the registry performs static analysis to identify all exported symbols and their internal dependency graph. It then creates per-export (or per-export-group) compiled artifacts and stores them as separate CAS blobs. At install time, the client only downloads the exports it uses.

### New Files

```
glyim-registry-granularity/
├── Cargo.toml
└── src/
    ├── lib.rs
    ├── analyze.rs      — Static analysis: build export → dependency graph
    ├── split.rs        — Split compiled artifact into per-export blobs
    ├── manifest.rs     — Granularity manifest (export → CAS hash mapping)
    └── tree_shake.rs   — Determine which exports are needed by a consumer
```

### Core Types

```rust
// ── src/analyze.rs ────────────────────────────────────────────

/// The result of analyzing a package's exports.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ExportGraph {
    /// Package name
    pub package: String,
    /// Package version
    pub version: String,
    /// Each export and the internal symbols it depends on
    pub exports: Vec<ExportNode>,
    /// The content hash of the full package artifact
    pub full_artifact_hash: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ExportNode {
    /// The exported symbol name (e.g., "json.parse")
    pub symbol: String,
    /// Content hash of the minimal blob containing this export
    pub blob_hash: String,
    /// Size of the minimal blob in bytes
    pub blob_size: u64,
    /// Other exports from this package that this one depends on
    pub depends_on: Vec<String>,
    /// All internal symbols referenced (for tree-shaking)
    pub internal_refs: Vec<String>,
}

// ── src/manifest.rs ───────────────────────────────────────────

/// The granularity manifest stored alongside the package in the CAS.
/// Maps each export to its minimal artifact blob.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct GranularityManifest {
    pub package: String,
    pub version: String,
    /// Export name → CAS blob hash
    pub export_blobs: std::collections::HashMap<String, String>,
    /// The full artifact (fallback if granularity not supported)
    pub full_blob_hash: String,
}
```

### How It Works at Publish Time

```
1. Author runs: glyim pkg publish
2. Registry receives the package source + compiled artifact
3. Static analysis runs (glyim-registry-granularity):
   a. Parse the package's public API (all exported symbols)
   b. Build a dependency graph between exports
   c. For each export, compute the minimal set of code needed
4. Split the compiled artifact into per-export blobs
5. Store each blob in the CAS
6. Store the GranularityManifest in the CAS
7. Update the index with the GranularityManifest hash
```

### How It Works at Install Time

```
1. Consumer's code: use json.parse, json.stringify
2. Client sends: "I need json.parse + json.stringify from serenity-json@3.1.4"
3. Registry looks up the GranularityManifest
4. Computes the minimal set of blobs needed:
   - json.parse blob (28KB)
   - json.stringify blob (22KB)
   - shared internal dependency blob (8KB)
   Total: 58KB instead of 340KB
5. Returns only those blobs
```

### Integration with CAS Server

Add a new endpoint:

```rust
// ── Add to glyim-cas-server REST routes ──────────────────────

.route("/api/v1/granular/{name}/{version}", post(granularity::install_granular))
```

```rust
// ── granularity.rs (new file in glyim-cas-server/src/) ──────

async fn install_granular(
    State(state): State<Arc<AppState>>,
    Path((name, version)): Path<(String, String)>,
    Json(req): Json<GranularInstallRequest>,
) -> impl IntoResponse {
    // req.imports = ["json.parse", "json.stringify"]

    // 1. Look up GranularityManifest for name@version
    // 2. Compute minimal blob set needed for the requested imports
    // 3. Return the blobs (or their hashes for client-side CAS fetch)
    // ...
}
```

---

## MODULE 7: glyim-mirror 🟡 IMPORTANT

**CRDT-based offline-first local mirror.**

### What It Does

Enables full offline operation by maintaining a local mirror of the registry that syncs conflict-free when online. Uses CRDTs (Conflict-free Replicated Data Types) so multiple mirrors (team, enterprise, personal) can sync without coordination.

### New Files

```
glyim-mirror/
├── Cargo.toml
└── src/
    ├── lib.rs
    ├── mirror.rs       — Local mirror management (sync, query, prune)
    ├── crdt.rs         — CRDT data structures for conflict-free sync
    ├── sync.rs         — Sync protocol between mirror and registry
    └── policy.rs       — Mirror policy (what to cache, retention, etc.)
```

### Core Types

```rust
// ── src/crdt.rs ──────────────────────────────────────────────

/// A CRDT-based package index that can sync with the registry
/// without conflicts. Uses Last-Writer-Wins (LWW) for version entries
/// and Observed-Remove Set (OR-Set) for package membership.
pub struct CrdtPackageIndex {
    /// LWW register for each (package, version) entry
    entries: HashMap<String, LwwRegister<VersionEntry>>,
    /// OR-Set for package names
    package_names: OrSet<String>,
    /// Lamport clock for ordering
    clock: LamportClock,
    /// Node ID for this mirror
    node_id: String,
}

/// Last-Writer-Wins register — the simplest CRDT for values
/// where the latest timestamp wins on conflict.
pub struct LwwRegister<T> {
    value: T,
    timestamp: u64,
    node_id: String,
}

/// Observed-Remove Set — items can be added and removed,
/// and concurrent add/remove operations resolve deterministically.
pub struct OrSet<T> {
    items: HashMap<T, Vec<u64>>, // item → unique tags
    tombstones: HashSet<u64>,     // removed tags
}

/// Lamport clock for causal ordering.
pub struct LamportClock {
    counter: u64,
}

impl LamportClock {
    pub fn tick(&mut self) -> u64 {
        self.counter += 1;
        self.counter
    }

    pub fn merge(&mut self, other: u64) {
        self.counter = self.counter.max(other) + 1;
    }
}
```

### Integration with glyim-pkg

Add mirror commands:

```rust
// ── Add to glyim-pkg CLI ─────────────────────────────────────

// glyim mirror sync    — Sync local mirror with remote registry
// glyim mirror status  — Show mirror health/staleness
// glyim mirror prune   — Remove old/unreferenced packages from mirror
// glyim mirror serve   — Run local mirror as a registry endpoint for team use
```

---

# CHANGES TO EXISTING CRATES

## glyim-cas-server Changes

### New Files to Add

```
src/
├── main.rs           — MODIFY: add index routes, new state
├── verify.rs         — MODIFY: add attestation generation
├── grpc/
│   ├── capabilities.rs — KEEP AS-IS
│   ├── cas.rs          — MODIFY: add delta blob support
│   └── mod.rs          — KEEP AS-IS
├── index.rs          — NEW: package index REST handlers
├── granularity.rs    — NEW: granular install endpoint
├── resolve.rs        — NEW: server-side resolution endpoint
└── trust.rs          — NEW: trust scoring + attestation endpoints
```

### Modified main.rs

```rust
// ── Updated AppState ─────────────────────────────────────────

pub struct AppState {
    store: Arc<RwLock<LocalContentStore>>,
    // NEW: Package index
    index: Arc<RwLock<glyim_registry_index::PackageIndex>>,
    // NEW: Server-side resolver
    resolver: Arc<glyim_registry_resolver::ServerResolver>,
    // NEW: Signing key for attestations
    signing_key: ed25519_dalek::SigningKey,
}

// ── Updated Router ───────────────────────────────────────────

let rest_app = Router::new()
    // Existing CAS routes
    .route("/blob", post(store_blob))
    .route("/blob/{hash}", get(retrieve_blob))
    .route("/blob/missing", post(find_missing_blobs))
    .route("/action/{hash}", post(store_action_result).get(retrieve_action_result))
    .route("/verify-wasm", post(verify::verify_wasm))
    .route("/status", get(status))
    // NEW: Package index routes (what glyim-pkg/registry.rs expects)
    .route("/api/v1/packages/{name}", get(index::get_package))
    .route("/api/v1/packages/{name}/{version}", get(index::get_version_detail))
    .route("/api/v1/packages/{name}/{version}/upload", post(index::publish_version))
    .route("/api/v1/packages/{name}/{version}/yank", post(index::yank_version))
    .route("/api/v1/search", get(index::search_packages))
    // NEW: Server-side resolution
    .route("/api/v1/resolve", post(resolve::resolve_dependencies))
    .route("/api/v1/resolve/{hash}", get(resolve::get_cached_resolution))
    // NEW: Granular install
    .route("/api/v1/granular/{name}/{version}", post(granularity::install_granular))
    // NEW: Trust & attestation
    .route("/api/v1/attest", post(trust::create_attestation))
    .route("/api/v1/trust/{name}/{version}", get(trust::get_trust_report))
    // NEW: Temporal snapshot
    .route("/api/v1/snapshot", get(index::snapshot_at_timestamp))
    .with_state(state);
```

---

## glyim-pkg Changes

### Modified Files

```
src/
├── cas_client.rs     — MODIFY: add granular fetch, delta fetch
├── error.rs          — MODIFY: add new error variants
├── lib.rs            — MODIFY: re-export new modules
├── lockfile.rs       — MODIFY: add temporal resolution support
├── manifest.rs       — MODIFY: add capabilities, sandbox, artifacts sections
├── registry.rs       — MODIFY: add resolve, granular install, trust query
├── resolver.rs       — MODIFY: add server-side resolution fallback
├── wasm_publish.rs   — MODIFY: add attestation generation
├── workspace.rs      — KEEP AS-IS
├── capabilities.rs   — NEW: capability contract types
├── granular.rs       — NEW: granular install client
└── mirror.rs         — NEW: mirror sync client
```

### Modified manifest.rs (Additions)

```rust
// ── Extended Package struct ──────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct Package {
    pub name: String,
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub no_std: Option<bool>,
    #[serde(default)]
    pub edition: String,
    #[serde(default)]
    pub authors: Vec<String>,
    #[serde(default)]
    pub description: Option<String>,
    /// NEW: Declared capabilities
    #[serde(default)]
    pub capabilities: Option<CapabilitiesConfig>,
    /// NEW: Sandbox constraints
    #[serde(default)]
    pub sandbox: Option<SandboxConfig>,
    /// NEW: Multi-platform artifact declarations
    #[serde(default)]
    pub artifacts: Option<HashMap<String, ArtifactConfig>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct CapabilitiesConfig {
    #[serde(default)]
    pub provides: Vec<String>,
    #[serde(default)]
    pub requires: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct SandboxConfig {
    #[serde(default)]
    pub max_memory: Option<String>,
    #[serde(default)]
    pub max_cpu_time: Option<String>,
    #[serde(default)]
    pub network: bool,
    #[serde(default)]
    pub filesystem: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ArtifactConfig {
    pub hash: String,
    #[serde(default)]
    pub size: Option<u64>,
    #[serde(default)]
    pub triple: Option<String>,
}
```

### Modified lockfile.rs (Additions)

```rust
// ── Extended LockedPackage ───────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LockedPackage {
    pub name: String,
    pub version: String,
    pub hash: String,
    #[serde(default)]
    pub is_macro: bool,
    pub source: LockSource,
    #[serde(default)]
    pub deps: Vec<String>,
    #[serde(default)]
    pub artifact_hash: Option<String>,
    #[serde(default)]
    pub interface_hash: Option<String>,
    #[serde(default)]
    pub target_triple: Option<String>,
    // NEW:
    #[serde(default)]
    pub trust_level: Option<String>,
    #[serde(default)]
    pub capabilities: Vec<String>,
    #[serde(default)]
    pub granularity_manifest_hash: Option<String>,
    /// NEW: Only the exports we actually use (for granular installs)
    #[serde(default)]
    pub used_exports: Vec<String>,
}

// NEW: Temporal lockfile variant
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TemporalLockfile {
    /// The timestamp at which this lockfile was resolved
    pub resolve_as_of: String,
    /// The registry URL
    pub registry: String,
    /// The resolution hash (cache key on the server)
    pub resolution_hash: String,
}
```

### Modified registry.rs (Additions)

```rust
// ── New methods on RegistryClient ────────────────────────────

impl RegistryClient {
    // ... existing methods ...

    /// NEW: Server-side dependency resolution.
    pub fn resolve(
        &self,
        dependencies: &HashMap<String, String>,
        resolve_as_of: Option<&str>,
        objective: &str,
    ) -> Result<ResolutionResponse, PkgError> {
        let url = format!("{}/api/v1/resolve", self.endpoint);
        let body = serde_json::json!({
            "dependencies": dependencies,
            "resolve_as_of": resolve_as_of,
            "objective": objective,
        });
        let response = self
            .client
            .post(&url)
            .json(&body)
            .send()
            .map_err(|e| PkgError::Registry(format!("resolve: {e}")))?;
        if !response.status().is_success() {
            return Err(PkgError::Registry(format!(
                "resolve returned {}",
                response.status()
            )));
        }
        response
            .json()
            .map_err(|e| PkgError::Registry(format!("parse resolve response: {e}")))
    }

    /// NEW: Granular install — fetch only specific exports from a package.
    pub fn install_granular(
        &self,
        name: &str,
        version: &str,
        exports: &[String],
    ) -> Result<GranularInstallResponse, PkgError> {
        let url = format!("{}/api/v1/granular/{name}/{version}", self.endpoint);
        let body = serde_json::json!({ "imports": exports });
        let response = self
            .client
            .post(&url)
            .json(&body)
            .send()
            .map_err(|e| PkgError::Registry(format!("granular install: {e}")))?;
        if !response.status().is_success() {
            return Err(PkgError::Registry(format!(
                "granular install returned {}",
                response.status()
            )));
        }
        response
            .json()
            .map_err(|e| PkgError::Registry(format!("parse granular response: {e}")))
    }

    /// NEW: Get trust report for a package version.
    pub fn get_trust_report(
        &self,
        name: &str,
        version: &str,
    ) -> Result<TrustReport, PkgError> {
        let url = format!("{}/api/v1/trust/{name}/{version}", self.endpoint);
        let response = self
            .client
            .get(&url)
            .send()
            .map_err(|e| PkgError::Registry(format!("trust report: {e}")))?;
        if !response.status().is_success() {
            return Err(PkgError::Registry(format!(
                "trust report returned {}",
                response.status()
            )));
        }
        response
            .json()
            .map_err(|e| PkgError::Registry(format!("parse trust report: {e}")))
    }

    /// NEW: Search packages by capability or natural language query.
    pub fn search(&self, query: &str) -> Result<Vec<SearchResult>, PkgError> {
        let url = format!("{}/api/v1/search?q={}", self.endpoint, query);
        let response = self
            .client
            .get(&url)
            .send()
            .map_err(|e| PkgError::Registry(format!("search: {e}")))?;
        if !response.status().is_success() {
            return Err(PkgError::Registry(format!(
                "search returned {}",
                response.status()
            )));
        }
        response
            .json()
            .map_err(|e| PkgError::Registry(format!("parse search results: {e}")))
    }
}

#[derive(Debug, serde::Deserialize)]
pub struct ResolutionResponse {
    pub packages: HashMap<String, ResolvedPkg>,
    pub resolution_hash: String,
    pub total_size_bytes: u64,
    pub resolve_time_ms: u64,
}

#[derive(Debug, serde::Deserialize)]
pub struct ResolvedPkg {
    pub version: String,
    pub content_hash: String,
    pub is_macro: bool,
    pub deps: Vec<String>,
    pub trust_level: String,
}

#[derive(Debug, serde::Deserialize)]
pub struct GranularInstallResponse {
    pub blobs: HashMap<String, String>, // export_name → content_hash
    pub total_size_bytes: u64,
}

#[derive(Debug, serde::Deserialize)]
pub struct TrustReport {
    pub trust_level: String,
    pub score: u32,
    pub attestations: Vec<AttestationInfo>,
    pub last_audit: Option<String>,
    pub known_vulnerabilities: u32,
}

#[derive(Debug, serde::Deserialize)]
pub struct AttestationInfo {
    pub kind: String,
    pub signer: String,
    pub signed_at: String,
}

#[derive(Debug, serde::Deserialize)]
pub struct SearchResult {
    pub name: String,
    pub latest_version: String,
    pub description: Option<String>,
    pub trust_level: String,
    pub relevance: f64,
}
```

---

# DEVELOPMENT PRIORITY & TIMELINE

## Phase 1: Foundation (Weeks 1-4) — Ship the Index

```
╔═══════════════════════════════════════════════════════════════╗
║  PHASE 1 GOAL: Connect CAS server to package index           ║
║  so that glyim pkg install actually works end-to-end         ║
╠═══════════════════════════════════════════════════════════════╣
║                                                               ║
║  1. Create glyim-registry-index crate                         ║
║     ├─ PackageIndex struct with publish/yank/get              ║
║     ├─ VersionEntry with all new fields                       ║
║     ├─ EventLog (append-only JSONL)                           ║
║     └─ Snapshot replay                                        ║
║                                                               ║
║  2. Add /api/v1/packages/* routes to glyim-cas-server         ║
║     ├─ GET /api/v1/packages/{name}                            ║
║     ├─ GET /api/v1/packages/{name}/{version}                  ║
║     ├─ POST /api/v1/packages/{name}/{version}/upload          ║
║     ├─ POST /api/v1/packages/{name}/{version}/yank            ║
║     └─ GET /api/v1/search?q=...                               ║
║                                                               ║
║  3. Verify glyim-pkg/registry.rs works with new endpoints     ║
║                                                               ║
║  DELIVERABLE: `glyim pkg install foo` works end-to-end        ║
╚═══════════════════════════════════════════════════════════════╝
```

## Phase 2: Intelligence (Weeks 5-8) — Temporal + Server Resolution

```
╔═══════════════════════════════════════════════════════════════╗
║  PHASE 2 GOAL: Eliminate lockfiles, add server-side          ║
║  resolution and temporal pinning                              ║
╠═══════════════════════════════════════════════════════════════╣
║                                                               ║
║  1. Create glyim-registry-events crate (harden EventLog)      ║
║     ├─ Segment-based log with rotation                        ║
║     ├─ Checkpoint snapshots for fast startup                  ║
║     └─ Time-range queries                                     ║
║                                                               ║
║  2. Create glyim-registry-resolver crate                      ║
║     ├─ Server-side constraint solver                          ║
║     ├─ Resolution cache in CAS                                ║
║     ├─ Multiple optimization objectives                       ║
║     └─ Extended constraint types (tilde, range, comparators)  ║
║                                                               ║
║  3. Add temporal resolution to glyim-pkg                      ║
║     ├─ resolve_as_of field in manifest                        ║
║     ├─ TemporalLockfile variant                               ║
║     └─ Server-side resolution with fallback to local          ║
║                                                               ║
║  DELIVERABLE: Lockfiles are optional; temporal pinning works  ║
╚═══════════════════════════════════════════════════════════════╝
```

## Phase 3: Trust (Weeks 9-12) — Security Framework

```
╔═══════════════════════════════════════════════════════════════╗
║  PHASE 3 GOAL: Defense-in-depth trust framework              ║
╠═══════════════════════════════════════════════════════════════╣
║                                                               ║
║  1. Create glyim-registry-trust crate                         ║
║     ├─ Ed25519 signing + verification                         ║
║     ├─ SLSA provenance generation                             ║
║     ├─ Progressive trust scoring                              ║
║     └─ Multi-replica build attestation                        ║
║                                                               ║
║  2. Extend verify_wasm to produce attestations                ║
║                                                               ║
║  3. Add trust endpoints to CAS server                         ║
║     ├─ POST /api/v1/attest                                    ║
║     └─ GET /api/v1/trust/{name}/{version}                     ║
║                                                               ║
║  4. Add trust_level to lockfile                               ║
║                                                               ║
║  DELIVERABLE: Every installed package has a trust score       ║
╚═══════════════════════════════════════════════════════════════╝
```

## Phase 4: Performance (Weeks 13-16) — Granularity + Delta

```
╔═══════════════════════════════════════════════════════════════╗
║  PHASE 4 GOAL: Sub-10ms installs, function-level             ║
║  granularity, delta updates                                   ║
╠═══════════════════════════════════════════════════════════════╣
║                                                               ║
║  1. Create glyim-registry-granularity crate                   ║
║     ├─ Static analysis of exports                             ║
║     ├─ Per-export blob splitting                              ║
║     └─ GranularityManifest generation                         ║
║                                                               ║
║  2. Add delta blob support to CAS                             ║
║     ├─ Store blobs as Merkle tree chunks                      ║
║     ├─ Compute binary diffs between versions                  ║
║     └─ Delta fetch endpoint                                   ║
║                                                               ║
║  3. Add granular install endpoint to CAS server               ║
║                                                               ║
║  4. Add granular install to glyim-pkg                         ║
║                                                               ║
║  DELIVERABLE: Install only what you use, <10ms cold           ║
╚═══════════════════════════════════════════════════════════════╝
```

## Phase 5: Ecosystem (Weeks 17-20) — Mirror + Capabilities

```
╔═══════════════════════════════════════════════════════════════╗
║  PHASE 5 GOAL: Offline-first, capability-addressed           ║
║  discovery, CRDT mirror                                       ║
╠═══════════════════════════════════════════════════════════════╣
║                                                               ║
║  1. Create glyim-mirror crate                                 ║
║     ├─ CRDT-based local mirror                                ║
║     ├─ Sync protocol                                          ║
║     └─ Local registry server mode                             ║
║                                                               ║
║  2. Create glyim-registry-capabilities crate                  ║
║     ├─ Capability contract verification                       ║
║     ├─ Capability inverted index                              ║
║     └─ Fuzzy capability matching                              ║
║                                                               ║
║  3. Add capabilities to manifest                              ║
║                                                               ║
║  4. Add sandbox config to manifest                            ║
║                                                               ║
║  DELIVERABLE: Full offline support + search by capability     ║
╚═══════════════════════════════════════════════════════════════╝
```

---

# FINAL ARCHITECTURE DIAGRAM

```
┌─────────────────────────────────────────────────────────────────────┐
│                        GLYIM REGISTRY SYSTEM                         │
│                                                                      │
│  ┌──────────────────────────────────────────────────────────────┐   │
│  │                  glyim-cas-server (REST + gRPC)              │   │
│  │                                                              │   │
│  │  ┌────────────┐  ┌──────────────┐  ┌────────────────────┐  │   │
│  │  │  CAS Blobs  │  │  Package     │  │  Event Log         │  │   │
│  │  │  (existing) │  │  Index       │  │  (append-only)     │  │   │
│  │  │             │  │  (NEW)       │  │  (NEW)             │  │   │
│  │  └──────┬──────┘  └──────┬───────┘  └────────┬───────────┘  │   │
│  │         │                │                    │               │   │
│  │  ┌──────┴────────────────┴────────────────────┴───────────┐  │   │
│  │  │                    REST API                             │  │   │
│  │  │  /blob/*  /api/v1/packages/*  /api/v1/resolve          │  │   │
│  │  │  /verify-wasm  /api/v1/trust/*  /api/v1/granular/*    │  │   │
│  │  │  /api/v1/search  /api/v1/snapshot                      │  │   │
│  │  └────────────────────────────────────────────────────────┘  │   │
│  └──────────────────────────────────────────────────────────────┘   │
│                                  │                                   │
│                                  ▼                                   │
│  ┌──────────────────────────────────────────────────────────────┐   │
│  │                       glyim-pkg (Client)                     │   │
│  │                                                              │   │
│  │  ┌─────────────┐  ┌──────────────┐  ┌──────────────────┐  │   │
│  │  │ CAS Client   │  │ Registry     │  │ Resolver         │  │   │
│  │  │ (existing +  │  │ Client       │  │ (existing +      │  │   │
│  │  │  granular)   │  │ (existing +  │  │  server-side     │  │   │
│  │  │              │  │  resolve,    │  │  fallback)       │  │   │
│  │  │              │  │  trust,      │  │                  │  │   │
│  │  │              │  │  search)     │  │                  │  │   │
│  │  └──────────────┘  └──────────────┘  └──────────────────┘  │   │
│  │                                                              │   │
│  │  ┌─────────────┐  ┌──────────────┐  ┌──────────────────┐  │   │
│  │  │ Manifest     │  │ Lockfile     │  │ Workspace        │  │   │
│  │  │ (existing +  │  │ (existing +  │  │ (existing)       │  │   │
│  │  │  caps,       │  │  temporal,   │  │                  │  │   │
│  │  │  sandbox,    │  │  trust,      │  │                  │  │   │
│  │  │  artifacts)  │  │  granular)   │  │                  │  │   │
│  │  └──────────────┘  └──────────────┘  └──────────────────┘  │   │
│  └──────────────────────────────────────────────────────────────┘   │
│                                                                      │
│  ┌──────────────────────────────────────────────────────────────┐   │
│  │                    New Crates (Libraries)                    │   │
│  │                                                              │   │
│  │  glyim-registry-index    glyim-registry-events               │   │
│  │  glyim-registry-resolver glyim-registry-trust                │   │
│  │  glyim-registry-capabilities                                 │   │
│  │  glyim-registry-granularity  glyim-mirror                    │   │
│  └──────────────────────────────────────────────────────────────┘   │
│                                                                      │
│  ┌──────────────────────────────────────────────────────────────┐   │
│  │                Existing Foundation (Unchanged)               │   │
│  │                                                              │   │
│  │  glyim-macro-vfs     glyim-codegen-llvm    glyim-interner    │   │
│  └──────────────────────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────────────────┘
```

---

# SOPHIA 📖 — The Revised Story

> The CAS server already breathes. Blobs flow in and out. Wasm is verified. The gRPC protocol speaks Bazel. The client resolves dependencies, parses manifests, generates lockfiles.
>
> What's missing isn't a heart — it's a **nervous system**.
>
> The index layer is that nervous system. It connects the storage muscle to the intelligence that decides what to store, how to find it, whether to trust it, and how to deliver only what's needed.
>
> Once the index exists, everything else follows naturally:
> - The event log gives the index **memory** — temporal resolution
> - The solver gives the index **judgment** — optimal dependency selection
> - The trust framework gives the index **instincts** — threat detection
> - The granularity engine gives the index **precision** — function-level delivery
> - The mirror gives the index **resilience** — offline-first survival
>
> Build the index. Everything else is downstream.

---

**Next step: Start with Phase 1. Create `glyim-registry-index`, add the `/api/v1/packages/*` routes to the CAS server, and verify that `glyim pkg install` works end-to-end. That's the first domino.**
# 🧠 BMAD Creative Intelligence Suite — Revised Roadmap with hlock

## Reality Recalibration: hlock Changes Everything

---

# IMPACT ANALYSIS: What hlock Already Solves

```
╔══════════════════════════════════════════════════════════════════════════════╗
║  hlock ALREADY PROVIDES (no development needed)                            ║
╠══════════════════════════════════════════════════════════════════════════════╣
║                                                                              ║
║  🔐 TRUST & SIGNING                                                         ║
║  ├─ ✅ Ed25519 signing + verification                                       ║
║  ├─ ✅ ML-DSA-65 post-quantum signing + verification (FIPS 204)             ║
║  ├─ ✅ Ed448 algorithm slot (stub, ready to implement)                      ║
║  ├─ ✅ Signature expiry epochs                                               ║
║  ├─ ✅ Trusted key verification with algorithm matching                      ║
║  └─ ✅ Tamper detection on signed lockfiles                                  ║
║                                                                              ║
║  📋 ATTESTATION & PROVENANCE                                                ║
║  ├─ ✅ ExternalBundleSha256 attestation                                      ║
║  ├─ ✅ InlineSlsa attestation (builder + source)                             ║
║  └─ ✅ Per-hash attestation (each integrity hash can carry its own)          ║
║                                                                              ║
║  📦 PACKAGE MODEL                                                            ║
║  ├─ ✅ Multi-algorithm integrity hashes (SHA-1, SHA-256, SHA-512, BLAKE3)   ║
║  ├─ ✅ Per-export integrity (identifier + hash_algo + digest)                ║
║  ├─ ✅ Per-artifact integrity (os_id + arch_id + hash_algo + digest)         ║
║  ├─ ✅ Platform tags (TargetOS × TargetArch)                                 ║
║  ├─ ✅ Peer requirements + resolutions with hoisting                         ║
║  ├─ ✅ Dependency types (Runtime, Dev, Peer, Optional, OptionalTarget)       ║
║  ├─ ✅ Logical names / aliases                                               ║
║  ├─ ✅ Feature flags per package                                             ║
║  ├─ ✅ Hook/script hashes                                                    ║
║  └─ ✅ Patch directives with content IDs                                     ║
║                                                                              ║
║  🔗 SOURCE MODEL                                                             ║
║  ├─ ✅ Registry sources                                                      ║
║  ├─ ✅ Local / file sources                                                  ║
║  ├─ ✅ Git sources                                                           ║
║  ├─ ✅ Workspace sources                                                     ║
║  ├─ ✅ CasHttp sources (cas+https://)                                        ║
║  └─ ✅ IPFS sources                                                          ║
║                                                                              ║
║  📊 GRAPH OPERATIONS                                                         ║
║  ├─ ✅ Lockfile diff (added/removed/altered)                                 ║
║  ├─ ✅ Diff serialization (text + JSON)                                      ║
║  ├─ ✅ Subgraph extraction by content ID                                     ║
║  ├─ ✅ Platform-filtered subgraph extraction                                 ║
║  ├─ ✅ Topological sort (lexicographic tiebreak)                             ║
║  ├─ ✅ Cycle detection with path reporting                                   ║
║  ├─ ✅ Would-create-cycle prediction                                         ║
║  ├─ ✅ Runtime/dev/peer typed dependency traversal                          ║
║  ├─ ✅ Transitive deps, dependents-of, leaf packages                        ║
║  └─ ✅ Has-dependency-path queries                                           ║
║                                                                              ║
║  🛡️ INTEGRITY                                                                ║
║  ├─ ✅ BLAKE3 whole-lockfile digest                                          ║
║  ├─ ✅ @digest directive verification                                        ║
║  ├─ ✅ Per-payload BLAKE3 trailer (tamper-proof binary format)               ║
║  ├─ ✅ Base64URL encoding for text-safe transport                            ║
║  └─ ✅ Varint encoding for compact binary representation                     ║
║                                                                              ║
║  🏗️ STRUCTURAL                                                               ║
║  ├─ ✅ Overrides (@override directive)                                        ║
║  ├─ ✅ Hoist boundaries with allowed-dep whitelists                          ║
║  ├─ ✅ Workspace root + workspace package tracking                           ║
║  ├─ ✅ Metadata key-value pairs                                              ║
║  └─ ✅ Sorted package output (deterministic serialization)                   ║
║                                                                              ║
╚══════════════════════════════════════════════════════════════════════════════╝
```

## What This Eliminates from the Roadmap

| Previously Planned | Status | Reason |
|---|---|---|
| Build lockfile system | ❌ **ELIMINATED** | hlock provides a superior binary lockfile format |
| Build signing infrastructure | ❌ **ELIMINATED** | hlock has Ed25519 + ML-DSA-65 |
| Build SLSA provenance | ❌ **ELIMINATED** | hlock has InlineSlsa attestation |
| Build platform-tagged packages | ❌ **ELIMINATED** | hlock has PlatformTag + Artifact |
| Build graph operations | ❌ **ELIMINATED** | hlock has full graph module |
| Build integrity verification | ❌ **ELIMINATED** | hlock has BLAKE3 digests throughout |
| Build diff system | ❌ **ELIMINATED** | hlock has diff + text/JSON serialization |
| Build export-level hashing | ❌ **ELIMINATED** | hlock has Export struct with hash_algo + digest |
| glyim-registry-trust crate (signing) | 🟡 **REDUCED** | Only trust scoring + progressive trust needed; crypto is done |
| AI Oracle | 🟡 **DEFERRED** | Lower priority than core integration |
| Auto-Migration | 🟡 **DEFERRED** | Lower priority than core integration |
| CRDT Mirror | 🟡 **DEFERRED** | Lower priority than core integration |

---

# REVISED ARCHITECTURE: What Actually Needs Building

```
╔══════════════════════════════════════════════════════════════════════════════╗
║  WHAT STILL NEEDS DEVELOPMENT                                               ║
╠══════════════════════════════════════════════════════════════════════════════╣
║                                                                              ║
║  🔴 CRITICAL PATH (Architecture-blocking)                                   ║
║  ├─ 1. hlock ↔ glyim-pkg integration bridge                                 ║
║  ├─ 2. Package index service (name → version → CAS hash)                    ║
║  ├─ 3. CAS server ↔ index wiring (REST endpoints)                           ║
║  └─ 4. Server-side resolver that outputs hlock Lockfile structs             ║
║                                                                              ║
║  🟡 IMPORTANT (Differentiating)                                             ║
║  ├─ 5. Temporal resolution (append-only event log)                          ║
║  ├─ 6. Progressive trust scoring engine                                      ║
║  ├─ 7. Function-level granularity engine (populates hlock Exports)          ║
║  ├─ 8. Capability-addressed discovery index                                  ║
║  └─ 9. Delta update support (Merkle blob splitting in CAS)                  ║
║                                                                              ║
║  🟢 POLISH (Delight)                                                        ║
║  ├─ 10. AI Oracle                                                           ║
║  ├─ 11. Auto-migration agents                                               ║
║  ├─ 12. CRDT offline mirror                                                 ║
║  └─ 13. Package health genome                                               ║
║                                                                              ║
╚══════════════════════════════════════════════════════════════════════════════╝
```

---

# REVISED CRATE STRUCTURE

```
glyim-workspace/
├── crates/
│   ├── glyim-cas-server/           ← EXISTS — extend with index routes
│   ├── glyim-pkg/                  ← EXISTS — replace lockfile with hlock
│   ├── glyim-macro-vfs/            ← EXISTS — foundation
│   ├── glyim-codegen-llvm/         ← EXISTS — foundation
│   ├── glyim-interner/             ← EXISTS — foundation
│   │
│   ├── hlock/                      ← EXISTS (external dep) — lockfile format
│   │
│   ├── glyim-registry-index/       ← NEW — package name→hash index
│   ├── glyim-registry-resolver/    ← NEW — server-side resolution → hlock output
│   ├── glyim-registry-events/      ← NEW — temporal resolution event log
│   ├── glyim-registry-granularity/ ← NEW — function-level splitting
│   ├── glyim-registry-trust/       ← NEW — trust scoring only (crypto is hlock)
│   └── glyim-registry-capabilities/← NEW — capability contracts + discovery
```

**Removed from previous plan:**
- ~~glyim-mirror~~ (deferred)
- ~~glyim-registry-events as separate crate~~ (folded into index for simplicity)

---

# PHASE 1: hlock Integration + Index (Weeks 1-3)

## Goal: `glyim pkg install foo` works end-to-end with hlock lockfiles

---

### Step 1.1: Add hlock Dependency to glyim-pkg

```toml
# crates/glyim-pkg/Cargo.toml — ADD:
[dependencies]
hlock = { path = "../hlock" }
# REMOVE: sha2, hex (hlock provides BLAKE3)
# REMOVE: toml lockfile serialization (hlock provides binary format)
```

### Step 1.2: Replace glyim-pkg Lockfile with hlock

The existing `glyim-pkg/src/lockfile.rs` uses a simple TOML format. Replace it with hlock:

```rust
// ── crates/glyim-pkg/src/lockfile.rs — COMPLETE REWRITE ──────

//! Lockfile operations delegated to hlock.
//! 
//! glyim-pkg uses hlock's binary lockfile format with:
//! - BLAKE3 whole-file digest
//! - Per-export integrity hashes
//! - Per-artifact platform hashes  
//! - Ed25519 / ML-DSA-65 signatures
//! - SLSA provenance attestations

pub use hlock::{
    Lockfile, Package, Source, IntegrityHash, HashAlgorithm,
    Attestation, Dependency, DepType, PlatformTag, TargetOS, TargetArch,
    Export, Artifact, PeerResolution, PeerRequirement,
    SlsaPredicate, HoistBoundary, WorkspacePkg,
    read_lockfile, write_lockfile, serialize, deserialize,
    validate_digest, whole_lockfile_digest,
    sign_lockfile, verify_signature, SignatureAlgorithm,
    diff_lockfiles, extract_subgraph, extract_subgraph_platform,
    topological_sort, detect_cycle, transitive_deps, dependents_of,
    Error as HlockError,
};

use crate::error::PkgError;
use crate::resolver::{AvailableVersion, Requirement, Resolution};
use std::collections::HashMap;
use std::path::Path;

/// Convert a resolved dependency tree into an hlock Lockfile.
pub fn resolution_to_lockfile(
    resolution: &Resolution,
    registry_url: &str,
) -> Lockfile {
    let source = Source::Registry(registry_url.to_string());
    let mut packages = Vec::new();

    for (name, resolved) in &resolution.packages {
        let dep_type = if resolved.is_macro {
            DepType::Runtime // macros are runtime deps in Glyim
        } else {
            DepType::Runtime
        };

        let deps: Vec<Dependency> = resolved.deps.iter().map(|dep_name| {
            Dependency {
                name: dep_name.clone(),
                dep_type: dep_type.clone(),
                requested_features: vec![],
            }
        }).collect();

        // Parse version string "1.2.3" into (major, minor, patch)
        let (major, minor, patch) = parse_version(&resolved.version);

        packages.push(Package {
            name: name.clone(),
            logical_name: None,
            source_idx: 0, // Will be set during serialization
            major,
            minor,
            patch,
            hashes: vec![], // Will be populated from CAS
            features: vec![],
            resolved_peers: vec![],
            dependencies: deps,
            peer_requirements: vec![],
            platform_tags: vec![],
            exports: vec![],     // Will be populated by granularity engine
            artifacts: vec![],   // Will be populated by multi-platform builds
            hook_hashes: vec![],
            patch_hash: None,
        });
    }

    Lockfile {
        sources: vec![source],
        overrides: vec![],
        features: vec![],
        metadata: vec![],
        workspace_root: None,
        workspace_pkgs: vec![],
        hoist_boundaries: vec![],
        packages,
        artifacts: vec![],
        patches: vec![],
    }
}

/// Enrich a lockfile with integrity hashes from the CAS.
/// After resolution, fetch the content hash for each package
/// and store it as a BLAKE3 IntegrityHash.
pub fn enrich_with_cas_hashes(
    lockfile: &mut Lockfile,
    cas_client: &crate::cas_client::CasClient,
) -> Result<(), PkgError> {
    for pkg in &mut lockfile.packages {
        if pkg.hashes.is_empty() {
            // Compute content hash from CAS
            let ver_str = format!("{}@{}.{}.{}", pkg.name, pkg.major, pkg.minor, pkg.patch);
            if let Some(hash) = cas_client.resolve_name(&ver_str) {
                pkg.hashes.push(IntegrityHash {
                    algo: HashAlgorithm::Blake3,
                    digest: hash.to_hex().as_bytes().to_vec(),
                    attestation: Attestation::None,
                });
            }
        }
    }
    Ok(())
}

/// Write lockfile with BLAKE3 digest.
pub fn write(path: &Path, lockfile: &mut Lockfile) -> Result<(), PkgError> {
    write_lockfile(path, lockfile).map_err(|e| PkgError::Lockfile(e.to_string()))
}

/// Read and validate lockfile (verifies BLAKE3 digest).
pub fn read(path: &Path) -> Result<Lockfile, PkgError> {
    let lockfile = read_lockfile(path).map_err(|e| PkgError::Lockfile(e.to_string()))?;
    Ok(lockfile)
}

/// Sign a lockfile with the given key.
pub fn sign(
    lockfile: &mut Lockfile,
    key_id: &str,
    algorithm: SignatureAlgorithm,
    private_key: &[u8],
    expires_epoch: u64,
) -> Result<String, PkgError> {
    let serialized = serialize(lockfile).map_err(|e| PkgError::Lockfile(e.to_string()))?;
    sign_lockfile(&serialized, key_id, algorithm, private_key, expires_epoch)
        .map_err(|e| PkgError::Lockfile(e.to_string()))
}

fn parse_version(version: &str) -> (u64, u64, u64) {
    let parts: Vec<&str> = version.split('.').collect();
    let major = parts.first().and_then(|s| s.parse().ok()).unwrap_or(0);
    let minor = parts.get(1).and_then(|s| s.parse().ok()).unwrap_or(0);
    let patch = parts.get(2).and_then(|s| s.parse().ok()).unwrap_or(0);
    (major, minor, patch)
}
```

### Step 1.3: Build the Package Index

```
glyim-registry-index/
├── Cargo.toml
└── src/
    ├── lib.rs
    ├── index.rs          — PackageIndex: name → Vec<VersionEntry>
    ├── version_entry.rs  — Metadata for a single published version
    ├── event_log.rs      — Append-only JSONL for temporal resolution
    └── search.rs         — Name + capability search
```

```rust
// ── src/version_entry.rs ──────────────────────────────────────

use serde::{Deserialize, Serialize};

/// A single published version in the index.
/// This is the metadata that lives OUTSIDE the CAS blob,
/// enabling name→hash lookups and temporal resolution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VersionEntry {
    /// Semver version
    pub version: String,
    /// CAS content hash of the package archive
    pub content_hash: String,
    /// Whether this version contains macro code (Wasm)
    pub is_macro: bool,
    /// Direct dependencies (name + version constraint)
    pub deps: Vec<IndexDep>,
    /// Declared capabilities (for capability-addressed discovery)
    pub capabilities: Vec<String>,
    /// Progressive trust level
    pub trust_level: TrustLevel,
    /// Granularity manifest hash (if function-level splitting was performed)
    pub granularity_manifest_hash: Option<String>,
    /// Timestamp of publication (ISO 8601)
    pub published_at: String,
    /// Publisher identity
    pub publisher: String,
    /// SLSA provenance attestation hash (if available)
    pub provenance_hash: Option<String>,
    /// Whether this version is yanked
    pub yanked: bool,
    /// Deprecation message
    pub deprecated: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub enum TrustLevel {
    New,
    Established,
    Verified,
    Certified,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexDep {
    pub name: String,
    pub version_constraint: String,
    pub is_macro: bool,
}
```

```rust
// ── src/event_log.rs ──────────────────────────────────────────

use crate::version_entry::VersionEntry;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::{BufRead, Write};

/// Append-only event log for temporal resolution.
/// Every mutation (publish, yank, deprecate) is recorded.
/// Replay to any timestamp enables `resolve-as-of(timestamp)`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    pub id: u64,
    pub timestamp: String,
    pub kind: EventKind,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum EventKind {
    Publish { name: String, entry: VersionEntry },
    Yank { name: String, version: String },
    Unyank { name: String, version: String },
    Deprecate { name: String, version: String, message: String },
}

pub struct EventLog {
    dir: std::path::PathBuf,
    next_id: u64,
}

impl EventLog {
    pub fn open(dir: std::path::PathBuf) -> std::io::Result<Self> {
        std::fs::create_dir_all(&dir)?;
        let path = dir.join("event_log.jsonl");
        let next_id = if path.exists() {
            std::io::BufReader::new(std::fs::File::open(&path)?)
                .lines()
                .count() as u64
        } else {
            0
        };
        Ok(Self { dir, next_id })
    }

    pub fn append(&mut self, kind: EventKind) -> std::io::Result<u64> {
        let path = self.dir.join("event_log.jsonl");
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)?;

        let event = Event {
            id: self.next_id,
            timestamp: chrono::Utc::now().to_rfc3339(),
            kind,
        };

        let json = serde_json::to_string(&event)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        writeln!(file, "{}", json)?;
        file.flush()?;

        let id = self.next_id;
        self.next_id += 1;
        Ok(id)
    }

    /// Replay events up to a given timestamp, returning a snapshot
    /// of the index as it existed at that moment.
    pub fn replay_until(
        &self,
        timestamp: &str,
    ) -> std::io::Result<HashMap<String, Vec<VersionEntry>>> {
        let path = self.dir.join("event_log.jsonl");
        if !path.exists() {
            return Ok(HashMap::new());
        }

        let mut packages: HashMap<String, Vec<VersionEntry>> = HashMap::new();
        let reader = std::io::BufReader::new(std::fs::File::open(&path)?);

        for line in reader.lines() {
            let line = line?;
            let event: Event = serde_json::from_str(&line)
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;

            if event.timestamp > timestamp {
                break;
            }

            match event.kind {
                EventKind::Publish { name, entry } => {
                    packages.entry(name).or_default().push(entry);
                }
                EventKind::Yank { name, version } => {
                    if let Some(versions) = packages.get_mut(&name) {
                        for v in versions.iter_mut() {
                            if v.version == version {
                                v.yanked = true;
                            }
                        }
                    }
                }
                EventKind::Unyank { name, version } => {
                    if let Some(versions) = packages.get_mut(&name) {
                        for v in versions.iter_mut() {
                            if v.version == version {
                                v.yanked = false;
                            }
                        }
                    }
                }
                EventKind::Deprecate { name, version, message } => {
                    if let Some(versions) = packages.get_mut(&name) {
                        for v in versions.iter_mut() {
                            if v.version == version {
                                v.deprecated = Some(message.clone());
                            }
                        }
                    }
                }
            }
        }

        Ok(packages)
    }

    /// Replay all events to get the current index state.
    pub fn replay_all(&self) -> std::io::Result<HashMap<String, Vec<VersionEntry>>> {
        self.replay_until("9999-12-31T23:59:59Z")
    }
}
```

```rust
// ── src/index.rs ──────────────────────────────────────────────

use crate::event_log::{EventLog, EventKind};
use crate::version_entry::{IndexDep, TrustLevel, VersionEntry};
use std::collections::HashMap;

/// The package index — maps package names to their version entries.
/// Backed by an append-only event log for temporal resolution.
pub struct PackageIndex {
    /// In-memory cache: name → version entries (non-yanked)
    packages: HashMap<String, Vec<VersionEntry>>,
    /// Append-only event log
    event_log: EventLog,
}

impl PackageIndex {
    pub fn open(data_dir: &std::path::Path) -> std::io::Result<Self> {
        let event_log = EventLog::open(data_dir.join("events"))?;
        let packages = event_log.replay_all()?;

        // Filter out yanked versions from the in-memory cache
        let packages: HashMap<String, Vec<VersionEntry>> = packages
            .into_iter()
            .map(|(name, versions)| {
                let filtered: Vec<VersionEntry> = versions
                    .into_iter()
                    .filter(|v| !v.yanked)
                    .collect();
                (name, filtered)
            })
            .collect();

        Ok(Self { packages, event_log })
    }

    /// Publish a new version.
    pub fn publish(&mut self, name: &str, entry: VersionEntry) -> Result<u64, IndexError> {
        // Check for duplicate version
        if let Some(versions) = self.packages.get(name) {
            if versions.iter().any(|v| v.version == entry.version) {
                return Err(IndexError::VersionExists {
                    name: name.to_string(),
                    version: entry.version,
                });
            }
        }

        let event_id = self.event_log.append(EventKind::Publish {
            name: name.to_string(),
            entry: entry.clone(),
        })?;

        self.packages
            .entry(name.to_string())
            .or_default()
            .push(entry);

        Ok(event_id)
    }

    /// Yank a version (mark as do-not-use).
    pub fn yank(&mut self, name: &str, version: &str) -> Result<u64, IndexError> {
        let versions = self.packages.get_mut(name).ok_or(IndexError::NotFound {
            name: name.to_string(),
        })?;

        let entry = versions
            .iter_mut()
            .find(|v| v.version == version)
            .ok_or(IndexError::VersionNotFound {
                name: name.to_string(),
                version: version.to_string(),
            })?;

        entry.yanked = true;
        let event_id = self.event_log.append(EventKind::Yank {
            name: name.to_string(),
            version: version.to_string(),
        })?;

        // Remove from in-memory cache
        versions.retain(|v| !v.yanked);

        Ok(event_id)
    }

    /// Get all non-yanked versions for a package.
    pub fn get_versions(&self, name: &str) -> Vec<&VersionEntry> {
        self.packages
            .get(name)
            .map(|v| v.iter().collect())
            .unwrap_or_default()
    }

    /// Get a specific version.
    pub fn get_version(&self, name: &str, version: &str) -> Option<&VersionEntry> {
        self.packages
            .get(name)?
            .iter()
            .find(|v| v.version == version)
    }

    /// Get the index state as of a timestamp (temporal resolution).
    pub fn snapshot_at(&self, timestamp: &str) -> std::io::Result<HashMap<String, Vec<VersionEntry>>> {
        self.event_log.replay_until(timestamp)
    }

    /// Search by name prefix.
    pub fn search_by_name(&self, prefix: &str) -> Vec<&str> {
        self.packages
            .keys()
            .filter(|k| k.starts_with(prefix))
            .map(|k| k.as_str())
            .collect()
    }

    /// Search by capability.
    pub fn search_by_capability(&self, capability: &str) -> Vec<(&str, &VersionEntry)> {
        self.packages
            .iter()
            .flat_map(|(name, versions)| {
                versions
                    .iter()
                    .filter(|v| v.capabilities.contains(&capability.to_string()))
                    .map(|v| (name.as_str(), v))
            })
            .collect()
    }
}

#[derive(Debug, thiserror::Error)]
pub enum IndexError {
    #[error("version {version} already exists for {name}")]
    VersionExists { name: String, version: String },
    #[error("package {name} not found")]
    NotFound { name: String },
    #[error("version {version} not found for {name}")]
    VersionNotFound { name: String, version: String },
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}
```

### Step 1.4: Wire Index into CAS Server

Add routes to `glyim-cas-server/src/main.rs`:

```rust
// ── New modules to add ───────────────────────────────────────

mod index_handlers;  // NEW FILE

// ── Updated AppState ─────────────────────────────────────────

pub struct AppState {
    store: Arc<RwLock<LocalContentStore>>,
    index: Arc<RwLock<glyim_registry_index::PackageIndex>>,  // NEW
}

// ── New routes to add to Router ──────────────────────────────

let rest_app = Router::new()
    // Existing CAS routes
    .route("/blob", post(store_blob))
    .route("/blob/{hash}", get(retrieve_blob))
    .route("/blob/missing", post(find_missing_blobs))
    .route("/action/{hash}", post(store_action_result).get(retrieve_action_result))
    .route("/verify-wasm", post(verify::verify_wasm))
    .route("/status", get(status))
    // NEW: Package index routes
    .route("/api/v1/packages/{name}", get(index_handlers::get_package))
    .route("/api/v1/packages/{name}/{version}", get(index_handlers::get_version))
    .route("/api/v1/packages/{name}/{version}/upload", post(index_handlers::publish_version))
    .route("/api/v1/packages/{name}/{version}/yank", post(index_handlers::yank_version))
    .route("/api/v1/search", get(index_handlers::search))
    .route("/api/v1/snapshot", get(index_handlers::snapshot_at))
    .with_state(state);
```

```rust
// ── crates/glyim-cas-server/src/index_handlers.rs — NEW FILE ─

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Json},
};
use glyim_registry_index::version_entry::{IndexDep, TrustLevel, VersionEntry};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::RwLock;
use crate::AppState;

#[derive(Serialize)]
struct PackageResponse {
    name: String,
    versions: Vec<VersionSummary>,
}

#[derive(Serialize)]
struct VersionSummary {
    version: String,
    is_macro: bool,
    trust_level: String,
    content_hash: String,
    capabilities: Vec<String>,
    deprecated: Option<String>,
}

/// GET /api/v1/packages/{name}
pub async fn get_package(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
) -> impl IntoResponse {
    let index = state.index.read().await;
    let versions = index.get_versions(&name);

    if versions.is_empty() {
        return (StatusCode::NOT_FOUND, "package not found").into_response();
    }

    let response = PackageResponse {
        name: name.clone(),
        versions: versions
            .iter()
            .map(|v| VersionSummary {
                version: v.version.clone(),
                is_macro: v.is_macro,
                trust_level: format!("{:?}", v.trust_level),
                content_hash: v.content_hash.clone(),
                capabilities: v.capabilities.clone(),
                deprecated: v.deprecated.clone(),
            })
            .collect(),
    };

    Json(response).into_response()
}

/// GET /api/v1/packages/{name}/{version}
pub async fn get_version(
    State(state): State<Arc<AppState>>,
    Path((name, version)): Path<(String, String)>,
) -> impl IntoResponse {
    let index = state.index.read().await;
    match index.get_version(&name, &version) {
        Some(entry) => Json(entry).into_response(),
        None => (StatusCode::NOT_FOUND, "version not found").into_response(),
    }
}

#[derive(Deserialize)]
struct PublishRequest {
    content_hash: String,
    is_macro: bool,
    deps: Vec<IndexDep>,
    capabilities: Vec<String>,
    publisher: String,
}

/// POST /api/v1/packages/{name}/{version}/upload
pub async fn publish_version(
    State(state): State<Arc<AppState>>,
    Path((name, version)): Path<(String, String)>,
    Json(req): Json<PublishRequest>,
) -> impl IntoResponse {
    let entry = VersionEntry {
        version,
        content_hash: req.content_hash,
        is_macro: req.is_macro,
        deps: req.deps,
        capabilities: req.capabilities,
        trust_level: TrustLevel::New,
        granularity_manifest_hash: None,
        published_at: chrono::Utc::now().to_rfc3339(),
        publisher: req.publisher,
        provenance_hash: None,
        yanked: false,
        deprecated: None,
    };

    let mut index = state.index.write().await;
    match index.publish(&name, entry) {
        Ok(_) => (StatusCode::OK, "published").into_response(),
        Err(e) => (StatusCode::CONFLICT, e.to_string()).into_response(),
    }
}

/// POST /api/v1/packages/{name}/{version}/yank
pub async fn yank_version(
    State(state): State<Arc<AppState>>,
    Path((name, version)): Path<(String, String)>,
) -> impl IntoResponse {
    let mut index = state.index.write().await;
    match index.yank(&name, &version) {
        Ok(_) => (StatusCode::OK, "yanked").into_response(),
        Err(e) => (StatusCode::NOT_FOUND, e.to_string()).into_response(),
    }
}

#[derive(Deserialize)]
struct SearchQuery {
    q: String,
}

#[derive(Serialize)]
struct SearchResult {
    name: String,
    versions: Vec<VersionSummary>,
}

/// GET /api/v1/search?q=...
pub async fn search(
    State(state): State<Arc<AppState>>,
    Query(query): Query<SearchQuery>,
) -> impl IntoResponse {
    let index = state.index.read().await;

    // Search by name prefix
    let names = index.search_by_name(&query.q);

    // Also search by capability
    let cap_results = index.search_by_capability(&query.q);

    let mut results = Vec::new();
    let mut seen = std::collections::HashSet::new();

    for name in names {
        if seen.insert(name.to_string()) {
            let versions = index.get_versions(name);
            results.push(SearchResult {
                name: name.to_string(),
                versions: versions.iter().map(|v| VersionSummary {
                    version: v.version.clone(),
                    is_macro: v.is_macro,
                    trust_level: format!("{:?}", v.trust_level),
                    content_hash: v.content_hash.clone(),
                    capabilities: v.capabilities.clone(),
                    deprecated: v.deprecated.clone(),
                }).collect(),
            });
        }
    }

    for (name, _) in cap_results {
        if seen.insert(name.to_string()) {
            let versions = index.get_versions(name);
            results.push(SearchResult {
                name: name.to_string(),
                versions: versions.iter().map(|v| VersionSummary {
                    version: v.version.clone(),
                    is_macro: v.is_macro,
                    trust_level: format!("{:?}", v.trust_level),
                    content_hash: v.content_hash.clone(),
                    capabilities: v.capabilities.clone(),
                    deprecated: v.deprecated.clone(),
                }).collect(),
            });
        }
    }

    Json(results).into_response()
}

#[derive(Deserialize)]
struct SnapshotQuery {
    timestamp: String,
}

/// GET /api/v1/snapshot?timestamp=2025-01-15T10:00:00Z
pub async fn snapshot_at(
    State(state): State<Arc<AppState>>,
    Query(query): Query<SnapshotQuery>,
) -> impl IntoResponse {
    let index = state.index.read().await;
    match index.snapshot_at(&query.timestamp) {
        Ok(snapshot) => Json(snapshot).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}
```

### Step 1.5: Update glyim-pkg Registry Client

The existing `registry.rs` already expects these endpoints. Minimal changes needed:

```rust
// ── crates/glyim-pkg/src/registry.rs — MINIMAL CHANGES ───────

// The existing fetch_available() and publish() methods already work
// with the /api/v1/packages/* endpoints we just added to the CAS server.
// 
// CHANGES NEEDED:
// 1. Update RegistryVersionEntry to include new fields (trust_level, capabilities)
// 2. Add search() method
// 3. Add snapshot_at() method for temporal resolution
// 4. Update AvailableVersion to carry capabilities

// NEW: Add search method
pub fn search(&self, query: &str) -> Result<Vec<SearchResult>, PkgError> {
    let url = format!("{}/api/v1/search?q={}", self.endpoint, query);
    let response = self.client
        .get(&url)
        .send()
        .map_err(|e| PkgError::Registry(format!("search: {e}")))?;
    if !response.status().is_success() {
        return Err(PkgError::Registry(format!("search returned {}", response.status())));
    }
    response.json()
        .map_err(|e| PkgError::Registry(format!("parse search: {e}")))
}
```

---

# PHASE 2: Server-Side Resolution → hlock Output (Weeks 4-5)

## Goal: Resolver outputs hlock Lockfile structs directly

---

```rust
// ── crates/glyim-registry-resolver/src/lib.rs ────────────────

use glyim_registry_index::version_entry::VersionEntry;
use hlock::{
    DepType, Dependency, HashAlgorithm, IntegrityHash, Lockfile, Package,
    Source, TargetOS, TargetArch,
};
use std::collections::HashMap;

pub struct ServerResolver {
    /// Reference to the package index
    index: glyim_registry_index::PackageIndex,
}

/// What to optimize for during resolution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolutionObjective {
    MinimalVersions,
    LatestVersions,
    MaximalTrust,
}

/// Result of server-side resolution.
pub struct ServerResolution {
    /// The resolved hlock Lockfile (ready to write to disk)
    pub lockfile: Lockfile,
    /// Cache key for this resolution (sha3 of inputs)
    pub cache_key: String,
    /// Time taken to resolve
    pub resolve_time_ms: u64,
}

impl ServerResolver {
    pub fn new(index: glyim_registry_index::PackageIndex) -> Self {
        Self { index }
    }

    /// Resolve dependencies and produce an hlock Lockfile.
    pub fn resolve(
        &self,
        root_deps: &[(String, String)],  // (name, version_constraint)
        registry_url: &str,
        objective: ResolutionObjective,
        resolve_as_of: Option<&str>,
    ) -> Result<ServerResolution, ResolverError> {
        let start = std::time::Instant::now();

        // Get the appropriate index view (temporal or current)
        let index_view = match resolve_as_of {
            Some(ts) => self.index.snapshot_at(ts)
                .map_err(|e| ResolverError::Temporal(e.to_string()))?,
            None => self.index.snapshot_current(),
        };

        // Resolve
        let resolved = self.solve(root_deps, &index_view, &objective)?;

        // Convert to hlock Lockfile
        let source = Source::CasHttp(registry_url.to_string());
        let mut packages = Vec::new();

        for (name, entry) in &resolved {
            let deps: Vec<Dependency> = entry.deps.iter().map(|d| {
                Dependency {
                    name: d.name.clone(),
                    dep_type: DepType::Runtime,
                    requested_features: vec![],
                }
            }).collect();

            let (major, minor, patch) = parse_version(&entry.version);

            packages.push(Package {
                name: name.clone(),
                logical_name: None,
                source_idx: 0,
                major,
                minor,
                patch,
                hashes: vec![IntegrityHash {
                    algo: HashAlgorithm::Blake3,
                    digest: hex_to_bytes(&entry.content_hash),
                    attestation: hlock::Attestation::None,
                }],
                features: vec![],
                resolved_peers: vec![],
                dependencies: deps,
                peer_requirements: vec![],
                platform_tags: vec![],
                exports: vec![],
                artifacts: vec![],
                hook_hashes: vec![],
                patch_hash: None,
            });
        }

        let mut lockfile = Lockfile {
            sources: vec![source],
            overrides: vec![],
            features: vec![],
            metadata: vec![],
            workspace_root: None,
            workspace_pkgs: vec![],
            hoist_boundaries: vec![],
            packages,
            artifacts: vec![],
            patches: vec![],
        };

        let cache_key = compute_cache_key(root_deps, resolve_as_of, &objective);

        Ok(ServerResolution {
            lockfile,
            cache_key,
            resolve_time_ms: start.elapsed().as_millis() as u64,
        })
    }

    fn solve(
        &self,
        root_deps: &[(String, String)],
        index_view: &HashMap<String, Vec<VersionEntry>>,
        objective: &ResolutionObjective,
    ) -> Result<HashMap<String, VersionEntry>, ResolverError> {
        let mut resolved: HashMap<String, VersionEntry> = HashMap::new();
        let mut queue: Vec<(String, String)> = root_deps.to_vec();

        while let Some((name, constraint)) = queue.pop() {
            if resolved.contains_key(&name) {
                continue;
            }

            let versions = index_view.get(&name).ok_or_else(|| {
                ResolverError::PackageNotFound(name.clone())
            })?;

            let selected = select_version(versions, &constraint, objective).ok_or_else(|| {
                ResolverError::NoSatisfyingVersion {
                    name: name.clone(),
                    constraint,
                }
            })?;

            for dep in &selected.deps {
                if !resolved.contains_key(&dep.name) {
                    queue.push((dep.name.clone(), dep.version_constraint.clone()));
                }
            }

            resolved.insert(name, selected.clone());
        }

        Ok(resolved)
    }
}

fn select_version<'a>(
    versions: &'a [VersionEntry],
    constraint: &str,
    objective: &ResolutionObjective,
) -> Option<&'a VersionEntry> {
    let satisfying: Vec<&VersionEntry> = versions
        .iter()
        .filter(|v| satisfies_constraint(&v.version, constraint))
        .collect();

    match objective {
        ResolutionObjective::MinimalVersions => satisfying.iter().min_by(|a, b| {
            semver::Version::parse(&a.version).unwrap_or_default()
                .cmp(&semver::Version::parse(&b.version).unwrap_or_default())
        }),
        ResolutionObjective::LatestVersions => satisfying.iter().max_by(|a, b| {
            semver::Version::parse(&a.version).unwrap_or_default()
                .cmp(&semver::Version::parse(&b.version).unwrap_or_default())
        }),
        ResolutionObjective::MaximalTrust => satisfying.iter().max_by(|a, b| {
            a.trust_level.cmp(&b.trust_level)
        }),
    }
}

fn satisfies_constraint(version: &str, constraint: &str) -> bool {
    if constraint == "*" { return true; }
    if version == constraint { return true; }
    if let Some(rest) = constraint.strip_prefix('^') {
        if let (Ok(ver), Ok(req)) = (semver::Version::parse(version), semver::Version::parse(rest)) {
            return ver >= req && ver.major == req.major;
        }
    }
    false
}

fn parse_version(version: &str) -> (u64, u64, u64) {
    let parts: Vec<&str> = version.split('.').collect();
    (
        parts.first().and_then(|s| s.parse().ok()).unwrap_or(0),
        parts.get(1).and_then(|s| s.parse().ok()).unwrap_or(0),
        parts.get(2).and_then(|s| s.parse().ok()).unwrap_or(0),
    )
}

fn hex_to_bytes(hex: &str) -> Vec<u8> {
    (0..hex.len())
        .step_by(2)
        .filter_map(|i| u8::from_str_radix(&hex[i..i + 2], 16).ok())
        .collect()
}

fn compute_cache_key(
    deps: &[(String, String)],
    resolve_as_of: Option<&str>,
    objective: &ResolutionObjective,
) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    for (name, constraint) in deps {
        hasher.update(name.as_bytes());
        hasher.update(constraint.as_bytes());
    }
    if let Some(ts) = resolve_as_of {
        hasher.update(ts.as_bytes());
    }
    hasher.update(format!("{:?}", objective).as_bytes());
    hex::encode(hasher.finalize())
}

#[derive(Debug, thiserror::Error)]
pub enum ResolverError {
    #[error("package not found: {0}")]
    PackageNotFound(String),
    #[error("no version of '{name}' satisfies constraint {constraint}")]
    NoSatisfyingVersion { name: String, constraint: String },
    #[error("temporal resolution failed: {0}")]
    Temporal(String),
}
```

Add resolution endpoint to CAS server:

```rust
// ── Add to glyim-cas-server/src/main.rs Router ───────────────

.route("/api/v1/resolve", post(resolve_handlers::resolve))
```

```rust
// ── crates/glyim-cas-server/src/resolve_handlers.rs — NEW ───

use axum::{extract::State, http::StatusCode, response::{IntoResponse, Json}};
use serde::Deserialize;
use std::sync::Arc;
use tokio::sync::RwLock;
use crate::AppState;

#[derive(Deserialize)]
struct ResolveRequest {
    dependencies: std::collections::HashMap<String, String>,
    resolve_as_of: Option<String>,
    objective: Option<String>,
}

/// POST /api/v1/resolve
pub async fn resolve(
    State(state): State<Arc<AppState>>,
    Json(req): Json<ResolveRequest>,
) -> impl IntoResponse {
    let index = state.index.read().await;
    let resolver = glyim_registry_resolver::ServerResolver::new(index.clone());

    let objective = match req.objective.as_deref() {
        Some("minimal") => glyim_registry_resolver::ResolutionObjective::MinimalVersions,
        Some("trust") => glyim_registry_resolver::ResolutionObjective::MaximalTrust,
        _ => glyim_registry_resolver::ResolutionObjective::LatestVersions,
    };

    let deps: Vec<(String, String)> = req.dependencies.into_iter().collect();

    match resolver.resolve(
        &deps,
        &format!("http://127.0.0.1:9090"), // TODO: configurable
        objective,
        req.resolve_as_of.as_deref(),
    ) {
        Ok(result) => {
            // Return the hlock-formatted lockfile
            let mut lockfile = result.lockfile;
            match hlock::serialize(&mut lockfile) {
                Ok(serialized) => {
                    // Also return structured data
                    let response = serde_json::json!({
                        "cache_key": result.cache_key,
                        "resolve_time_ms": result.resolve_time_ms,
                        "lockfile": serialized,
                        "package_count": lockfile.packages.len(),
                    });
                    Json(response).into_response()
                }
                Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
            }
        }
        Err(e) => (StatusCode::UNPROCESSABLE_ENTITY, e.to_string()).into_response(),
    }
}
```

---

# PHASE 3: Trust Scoring + Signing Pipeline (Weeks 6-7)

## Goal: Every published package gets trust-scored and signed

---

Since hlock already provides the cryptographic primitives (Ed25519, ML-DSA-65, SLSA attestations), we only need:

1. **Progressive trust scoring engine** — computes TrustLevel from signals
2. **Sign-on-publish pipeline** — automatically signs packages on publication
3. **Verify-on-install pipeline** — verifies signatures before installing

```rust
// ── crates/glyim-registry-trust/src/lib.rs ───────────────────

use glyim_registry_index::version_entry::TrustLevel;

pub struct TrustScorer;

pub struct TrustSignals {
    pub age_days: u32,
    pub download_count: u64,
    pub publisher_count: u32,
    pub audit_passed: bool,
    pub reproducible_build: bool,
    pub author_signed: bool,
    pub ci_attested: bool,
    pub community_attestations: u32,
    pub has_known_vulnerabilities: bool,
}

impl TrustScorer {
    pub fn score(signals: &TrustSignals) -> TrustLevel {
        let mut points: u32 = 0;

        if signals.age_days >= 365 { points += 20; }
        else if signals.age_days >= 30 { points += 10; }
        else if signals.age_days >= 7 { points += 5; }

        if signals.download_count >= 10000 { points += 15; }
        else if signals.download_count >= 100 { points += 5; }

        if signals.publisher_count >= 3 { points += 10; }
        else if signals.publisher_count >= 2 { points += 5; }

        if signals.audit_passed { points += 15; }
        if signals.reproducible_build { points += 15; }
        if signals.author_signed { points += 5; }
        if signals.ci_attested { points += 10; }

        if signals.community_attestations >= 3 { points += 10; }
        else if signals.community_attestations >= 1 { points += 5; }

        if !signals.has_known_vulnerabilities { points += 10; }

        match points {
            0..=29 => TrustLevel::New,
            30..=59 => TrustLevel::Established,
            60..=89 => TrustLevel::Verified,
            _ => TrustLevel::Certified,
        }
    }
}
```

### Sign-on-Publish Pipeline

Extend the CAS server's `/verify-wasm` to produce hlock-compatible attestations:

```rust
// ── Updated glyim-cas-server/src/verify.rs ───────────────────

pub async fn verify_wasm(
    State(state): State<Arc<AppState>>,
    Json(req): Json<VerifyWasmRequest>,
) -> impl IntoResponse {
    // ... existing verification logic ...

    if matches {
        // NEW: Create SLSA attestation for the verified build
        let attestation = hlock::Attestation::InlineSlsa(hlock::SlsaPredicate {
            builder: "glyim-cas-server".to_string(),
            source: format!("reproducible-build://{}", actual_hash.to_hex()),
        });

        // NEW: Update the package's integrity hash in the index
        // to carry the attestation
        // (this is done by the caller after receiving the response)
    }
    // ...
}
```

---

# PHASE 4: Granularity Engine (Weeks 8-9)

## Goal: Function-level installs populate hlock Exports

---

hlock already has the `Export` struct:

```rust
// From hlock:
pub struct Export {
    pub identifier: String,    // e.g., "json.parse"
    pub hash_algo: HashAlgorithm,
    pub digest: Vec<u8>,       // BLAKE3 of the per-export blob
}
```

We just need to populate it:

```rust
// ── crates/glyim-registry-granularity/src/lib.rs ─────────────

use hlock::{Export, HashAlgorithm};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Result of analyzing a package's export structure.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GranularityManifest {
    pub package: String,
    pub version: String,
    /// Export name → CAS blob hash
    pub export_blobs: HashMap<String, String>,
    /// Full artifact blob hash (fallback)
    pub full_blob_hash: String,
}

/// Analyze a compiled package artifact and split it into per-export blobs.
/// Returns a GranularityManifest mapping each export to its minimal blob.
pub fn analyze_exports(
    package_name: &str,
    version: &str,
    artifact: &[u8],
    export_names: &[String],
) -> Result<GranularityManifest, GranularityError> {
    // TODO: Real static analysis using glyim-codegen-llvm
    // For now, create a manifest with the full artifact for each export

    let full_hash = blake3_hash(artifact);

    let export_blobs: HashMap<String, String> = export_names
        .iter()
        .map(|name| (name.clone(), full_hash.clone()))
        .collect();

    Ok(GranularityManifest {
        package: package_name.to_string(),
        version: version.to_string(),
        export_blobs,
        full_blob_hash: full_hash,
    })
}

/// Convert a GranularityManifest to hlock Export entries.
pub fn manifest_to_hlock_exports(
    manifest: &GranularityManifest,
) -> Vec<Export> {
    manifest
        .export_blobs
        .iter()
        .map(|(identifier, hash_hex)| Export {
            identifier: identifier.clone(),
            hash_algo: HashAlgorithm::Blake3,
            digest: hex_to_bytes(hash_hex),
        })
        .collect()
}

fn blake3_hash(data: &[u8]) -> String {
    let hash = blake3::hash(data);
    hex::encode(hash.as_bytes())
}

fn hex_to_bytes(hex: &str) -> Vec<u8> {
    (0..hex.len())
        .step_by(2)
        .filter_map(|i| u8::from_str_radix(&hex[i..i + 2], 16).ok())
        .collect()
}

#[derive(Debug, thiserror::Error)]
pub enum GranularityError {
    #[error("analysis failed: {0}")]
    Analysis(String),
}
```

---

# PHASE 5: Capability Discovery (Weeks 10-11)

## Goal: Search by capability, not by name

---

```rust
// ── crates/glyim-registry-capabilities/src/lib.rs ────────────

use glyim_registry_index::PackageIndex;
use serde::{Deserialize, Serialize};

/// A capability contract declared by a package.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapabilityContract {
    /// The capability identifier (e.g., "json.parse")
    pub identifier: String,
    /// The function signature (for type-safe matching)
    pub signature: Option<String>,
    /// Whether this capability requires sandbox permissions
    pub requires_sandbox: bool,
    /// Sandbox permissions needed
    pub sandbox_permissions: Vec<String>,
}

/// Search result with capability matching.
#[derive(Debug, Clone, Serialize)]
pub struct CapabilitySearchResult {
    pub name: String,
    pub version: String,
    pub matched_capabilities: Vec<String>,
    pub trust_level: String,
    pub relevance: f64,
}

/// Search packages by capability.
pub fn search_by_capability(
    index: &PackageIndex,
    query: &str,
) -> Vec<CapabilitySearchResult> {
    let results = index.search_by_capability(query);

    results
        .iter()
        .map(|(name, entry)| CapabilitySearchResult {
            name: name.to_string(),
            version: entry.version.clone(),
            matched_capabilities: entry.capabilities.clone(),
            trust_level: format!("{:?}", entry.trust_level),
            relevance: compute_relevance(query, &entry.capabilities),
        })
        .collect()
}

fn compute_relevance(query: &str, capabilities: &[String]) -> f64 {
    let query_lower = query.to_lowercase();
    let matches = capabilities.iter()
        .filter(|c| c.to_lowercase().contains(&query_lower))
        .count();

    if capabilities.is_empty() {
        0.0
    } else {
        matches as f64 / capabilities.len() as f64
    }
}
```

---

# COMPLETE REVISED TIMELINE

```
WEEK 1-3:  Phase 1 — hlock Integration + Index
           ├─ Add hlock dependency to glyim-pkg
           ├─ Replace glyim-pkg lockfile.rs with hlock delegation
           ├─ Build glyim-registry-index crate
           ├─ Wire /api/v1/packages/* routes into CAS server
           └─ DELIVERABLE: glyim pkg install foo works with hlock lockfiles

WEEK 4-5:  Phase 2 — Server-Side Resolution
           ├─ Build glyim-registry-resolver (outputs hlock Lockfile)
           ├─ Add /api/v1/resolve endpoint to CAS server
           ├─ Temporal resolution via event log
           └─ DELIVERABLE: Server resolves deps → signed hlock lockfile

WEEK 6-7:  Phase 3 — Trust + Signing
           ├─ Build glyim-registry-trust (scoring only)
           ├─ Sign-on-publish pipeline using hlock's ML-DSA-65
           ├─ Verify-on-install pipeline
           ├─ SLSA attestation on reproducible builds
           └─ DELIVERABLE: Every package has trust level + PQ signature

WEEK 8-9:  Phase 4 — Granularity
           ├─ Build glyim-registry-granularity
           ├─ Populate hlock Export entries on publish
           ├─ Granular install endpoint
           └─ DELIVERABLE: Install only what you use

WEEK 10-11: Phase 5 — Capabilities
           ├─ Build glyim-registry-capabilities
           ├─ Add capabilities to manifest + index
           ├─ Capability search endpoint
           └─ DELIVERABLE: Search by what you need

WEEK 12+:  Future
           ├─ AI Oracle
           ├─ Auto-migration agents
           ├─ CRDT offline mirror
           └─ Delta updates (Merkle blob splitting)
```

---

# FINAL ARCHITECTURE WITH hlock

```
┌──────────────────────────────────────────────────────────────────────────┐
│                    GLYIM REGISTRY SYSTEM (with hlock)                    │
│                                                                          │
│  ┌───────────────────────────────────────────────────────────────────┐   │
│  │                   glyim-cas-server (REST + gRPC)                  │   │
│  │                                                                   │   │
│  │  ┌────────────┐  ┌──────────────┐  ┌──────────────────────────┐ │   │
│  │  │  CAS Blobs  │  │  Package     │  │  Event Log               │ │   │
│  │  │  (existing) │  │  Index       │  │  (append-only JSONL)     │ │   │
│  │  └──────┬──────┘  └──────┬───────┘  └────────────┬─────────────┘ │   │
│  │         │                │                        │               │   │
│  │  ┌──────┴────────────────┴────────────────────────┴────────────┐ │   │
│  │  │                      REST API                               │ │   │
│  │  │  /blob/*  /api/v1/packages/*  /api/v1/resolve               │ │   │
│  │  │  /verify-wasm  /api/v1/search  /api/v1/snapshot             │ │   │
│  │  └─────────────────────────────────────────────────────────────┘ │   │
│  └───────────────────────────────────────────────────────────────────┘   │
│                                  │                                       │
│                                  ▼                                       │
│  ┌───────────────────────────────────────────────────────────────────┐   │
│  │                        glyim-pkg (Client)                         │   │
│  │                                                                   │   │
│  │  ┌─────────────┐  ┌──────────────┐  ┌──────────────────────────┐ │   │
│  │  │ CAS Client   │  │ Registry     │  │ Resolver                 │ │   │
│  │  │ (existing)   │  │ Client       │  │ (local fallback +        │ │   │
│  │  │              │  │ (existing +  │  │  server-side)            │ │   │
│  │  │              │  │  search,     │  │                          │ │   │
│  │  │              │  │  resolve)    │  │                          │ │   │
│  │  └──────────────┘  └──────────────┘  └──────────────────────────┘ │   │
│  │                                                                   │   │
│  │  ┌─────────────────────────────────────────────────────────────┐ │   │
│  │  │                    hlock (Lockfile)                          │ │   │
│  │  │  ✅ Binary format with BLAKE3 digest                        │ │   │
│  │  │  ✅ Ed25519 + ML-DSA-65 signatures                         │ │   │
│  │  │  ✅ SLSA attestations                                       │ │   │
│  │  │  ✅ Per-export integrity hashes                             │ │   │
│  │  │  ✅ Per-artifact platform hashes                            │ │   │
│  │  │  ✅ Platform-tagged packages                                │ │   │
│  │  │  ✅ Peer dependency resolution                              │ │   │
│  │  │  ✅ Graph operations (diff, extract, topo sort, cycles)     │ │   │
│  │  │  ✅ CasHttp + IPFS source types                             │ │   │
│  │  └─────────────────────────────────────────────────────────────┘ │   │
│  └───────────────────────────────────────────────────────────────────┘   │
│                                                                          │
│  ┌───────────────────────────────────────────────────────────────────┐   │
│  │                    New Crates (Libraries)                         │   │
│  │                                                                   │   │
│  │  glyim-registry-index        — name→version→hash mapping         │   │
│  │  glyim-registry-resolver     — server-side → hlock output        │   │
│  │  glyim-registry-trust        — progressive trust scoring          │   │
│  │  glyim-registry-granularity  — function-level splitting           │   │
│  │  glyim-registry-capabilities — capability contracts + discovery   │   │
│  └───────────────────────────────────────────────────────────────────┘   │
│                                                                          │
│  ┌───────────────────────────────────────────────────────────────────┐   │
│  │              Existing Foundation (Unchanged)                      │   │
│  │                                                                   │   │
│  │  hlock               glyim-macro-vfs      glyim-codegen-llvm     │   │
│  │  glyim-interner      (workspace crates)                          │   │
│  └───────────────────────────────────────────────────────────────────┘   │
└──────────────────────────────────────────────────────────────────────────┘
```

---

# KEY INSIGHT: hlock's Export Structure is the Granularity Bridge

The most important connection in this architecture:

```
PUBLISH TIME:
  1. Author publishes package with declared exports
  2. Granularity engine analyzes, splits into per-export blobs
  3. Each blob is stored in CAS → gets a BLAKE3 hash
  4. GranularityManifest is stored in CAS
  5. Index is updated with granularity_manifest_hash

LOCKFILE TIME:
  1. Resolver produces hlock Lockfile
  2. Package entries include Export[] populated from GranularityManifest
  3. Each Export has: identifier + HashAlgorithm::Blake3 + digest
  4. Lockfile is signed with ML-DSA-65

INSTALL TIME:
  1. Client reads hlock lockfile
  2. Validates BLAKE3 digest + ML-DSA-65 signature
  3. For each package, checks which Exports are actually used
  4. Only fetches the CAS blobs for used Exports
  5. Verifies each Export's BLAKE3 digest matches lockfile

This means: the lockfile IS the guarantee of function-level integrity.
No extra verification step needed — hlock already proves every export.
```

---

# SOPHIA 📖 — The Final Story

> Two crates walk into a registry.
>
> One is a CAS server — strong, silent, stores blobs by hash. It knows nothing of names, only content.
>
> The other is a lockfile library — meticulous, cryptographic, signs with post-quantum keys. It knows every export, every artifact, every platform, every attestation.
>
> Between them sits the index — the translator. It gives names to hashes, versions to content, timestamps to history. It connects storage to meaning.
>
> And when they work together, something remarkable happens: a developer types `glyim pkg install json-utils`, and in milliseconds, they receive a lockfile that is:
> - **Signed** with post-quantum cryptography (ML-DSA-65)
> - **Attested** with SLSA provenance
> - **Integrity-checked** with BLAKE3 at every level (whole file, per-package, per-export)
> - **Granular** — only the exports they use are included
> - **Platform-aware** — only artifacts for their OS/arch
> - **Temporally pinned** — resolved as of a specific moment in time
> - **Verifiable** — anyone can re-verify the entire chain
>
> This isn't a package registry. It's a **trust pipeline** — from author's keystroke to user's runtime, every step is proven.

---

**Build order unchanged: Phase 1 first. The index is the keystone. hlock is the vault door.**
# Glyim Registry — Full Specification Suite

Below are all five specification documents, fully fleshed out and cross-referenced. Each document is self-contained but traces to the others through stable IDs.

---

## Document 1: Product Vision & Strategic Alignment

```markdown
# Glyim Registry — Product Vision & Strategic Alignment

| Field | Value |
|-------|-------|
| Project | Glyim Registry |
| Document | Vision & Strategic Alignment |
| Version | 1.0 |
| Date | 2025-07-11 |
| Author | Glyim Team |
| Status | Approved |

---

## 1. Vision Statement

Glyim Registry is the first post-quantum-secured, function-granular package registry that makes every install a verifiable trust pipeline—from author keystroke to runtime execution—in under 10 milliseconds.

## 2. Elevator Pitch

For **systems programmers and language engineers** who are dissatisfied with npm/crates.io-style registries that conflate name lookups with blind trust, Glyim Registry is a **content-addressable package registry** that provides post-quantum signed lockfiles, function-level installs, and temporal dependency resolution. Unlike existing registries, our product delivers cryptographic proof of integrity at every granularity (file, export, artifact) and resolves dependencies as of any point in time—eliminating lockfile drift forever.

## 3. Problem Statement & Business Context

### 3.1 The Problem

Current package registries suffer from three critical failures:

1. **Trust is binary and retroactive.** Packages are either published or not. There is no progressive trust model—no way to distinguish a brand-new package from one that has survived months of production use, reproducible builds, and community attestation. Vulnerabilities are discovered after deployment, not before.

2. **Installs are all-or-nothing.** When you need `json.parse` from a 340KB library, you download all 340KB. Tree-shaking happens at build time, not at install time. This wastes bandwidth, increases attack surface, and slows cold starts.

3. **Lockfiles are frozen snapshots that rot.** A lockfile captures a moment in dependency space, but it cannot answer "what would I have gotten if I resolved on March 15?" Temporal resolution—reconstructing the index as it existed at any past timestamp—does not exist in mainstream registries.

### 3.2 Why Now

- **Post-quantum cryptography is standardized.** NIST finalized ML-DSA (CRYSTALS-Dilithium) as FIPS 204 in 2024. The tools exist to make every signature quantum-resistant today.
- **Supply chain attacks are accelerating.** The Sonatype 2024 report documented a 156% increase in open-source supply chain attacks over two years. Reactive scanning is insufficient.
- **Wasm macro systems demand granular delivery.** Glyim's own macro system compiles to Wasm blobs stored in a CAS. The infrastructure for content-addressable, per-export delivery already exists—it just needs an index layer.
- **SLSA and build provenance are industry requirements.** SLSA Level 3+ provenance is now required by major enterprises and government agencies for supply chain integrity.

### 3.3 Existing Foundation

The Glyim project already has:

- ✅ **glyim-cas-server**: Content-addressable blob store with REST + gRPC (Bazel RE protocol)
- ✅ **glyim-pkg**: Package manager with CAS client, manifest parser, dependency resolver, registry client
- ✅ **glyim-macro-vfs**: Virtual filesystem with ContentHash (SHA-256)
- ✅ **hlock**: Binary lockfile format with Ed25519 + ML-DSA-65 signing, BLAKE3 integrity, SLSA attestations, per-export/per-artifact hashes, graph operations, and platform tags

What is missing is the **index layer** that connects names to hashes, and the **intelligence layer** that provides server-side resolution, trust scoring, and function-level splitting.

## 4. Target Users & Customers

### 4.1 Primary Users

| User Class | Description | Needs |
|---|---|---|
| **Glyim Application Developer** | Writes Glyim applications, installs packages | Fast installs, trustworthy packages, minimal download |
| **Glyim Library Author** | Publishes reusable Glyim libraries | Easy publishing, build provenance, trust progression |
| **Glyim Macro Author** | Publishes Wasm-compiled macros | Reproducible build verification, macro-specific trust |

### 4.2 Secondary Users

| User Class | Description | Needs |
|---|---|---|
| **DevOps / Platform Engineer** | Manages CI/CD pipelines, offline mirrors | Deterministic builds, air-gapped operation, caching |
| **Security Auditor** | Reviews package trust, validates provenance | Attestation chains, audit trails, trust scoring |
| **Enterprise IT** | Governs package usage in organization | Policy enforcement, private mirrors, vulnerability tracking |

### 4.3 Explicitly NOT Targeting (Non-Goals)

- General-purpose package registries for other languages (Rust, Python, etc.)
- Binary distribution of native executables (focus on Wasm macros and Glyim bytecode)
- Real-time collaboration or IDE integration (separate product concern)
- Mobile application package management

## 5. User Needs & Value Proposition

### 5.1 Top User Needs

1. **Verifiable trust**: "I need to know that the package I install is what the author published, that it was built from the claimed source, and that it has no known vulnerabilities."
2. **Minimal installs**: "I need to download only the code I actually use, not the entire library."
3. **Reproducible resolution**: "I need to know exactly what dependencies I would have gotten on any past date, not just today."

### 5.2 Value Proposition

Glyim Registry is the only registry that makes every install a **verifiable trust pipeline**:

- **Post-quantum signatures** (ML-DSA-65) protect against future key compromise
- **Function-level delivery** reduces install size by 5–10× on average
- **Temporal resolution** eliminates lockfile drift and enables forensic reconstruction
- **Progressive trust** (New → Established → Verified → Certified) makes risk visible at a glance

Unlike crates.io (no signing, no granularity), npm (no PQ crypto, no temporal resolution), or Bazel registries (no trust scoring), Glyim Registry delivers all four capabilities as an integrated system.

## 6. Desired Outcomes & Success Metrics

### 6.1 Business Outcomes

| ID | Objective | Key Results | Measurement |
|----|-----------|-------------|-------------|
| G-1 | Make Glyim the most trusted language ecosystem | KR1: 100% of published packages carry PQ signatures within 6 months of launch | Automated scan of index |
| G-2 | Reduce install times by an order of magnitude | KR1: Median cold install <50ms for typical project; KR2: 5× size reduction via granularity | Benchmark suite |
| G-3 | Eliminate supply-chain surprise | KR1: Zero undetected tampering in 12-month red-team test; KR2: 100% of critical packages reach "Verified" trust within 90 days | Security audit + trust dashboard |

### 6.2 Product Outcomes

| ID | Outcome | Metric | Target |
|----|---------|--------|--------|
| P-1 | Developers trust the registry | % of installs where trust level ≥ Established | ≥ 80% |
| P-2 | Granularity is used | % of installs using granular (per-export) delivery | ≥ 60% |
| P-3 | Temporal resolution is adopted | % of lockfiles using temporal pinning | ≥ 30% within 6 months |
| P-4 | Offline operation is reliable | % of mirror syncs completing without conflict | 100% (CRDT guarantee) |

## 7. Strategic Constraints

| ID | Constraint | Type | Impact |
|----|-----------|------|--------|
| C-1 | Must use existing glyim-cas-server as blob storage | Technical | Index layer must be additive, not a replacement |
| C-2 | Must use hlock as the lockfile format | Technical | All resolution output must be hlock Lockfile structs |
| C-3 | CAS protocol must remain Bazel RE compatible | Compatibility | gRPC endpoints cannot change |
| C-4 | PQ signatures must use ML-DSA-65 (FIPS 204) | Regulatory | Cannot substitute with weaker algorithms |
| C-5 | First release within 12 weeks | Schedule | 5 phases of 2–3 weeks each |
| C-6 | Must operate offline after initial sync | Operational | Mirror/CRDT architecture is mandatory (Phase 5+) |

## 8. Goals and Non-Goals

### 8.1 Goals

- G-4: Connect CAS server to package index so `glyim pkg install` works end-to-end
- G-5: Implement server-side dependency resolution that outputs hlock Lockfiles
- G-6: Implement progressive trust scoring with automated attestation
- G-7: Implement function-level package splitting at publish time
- G-8: Implement capability-addressed package discovery
- G-9: Implement temporal resolution via append-only event log
- G-10: Implement sign-on-publish and verify-on-install pipelines using hlock's PQ crypto

### 8.2 Non-Goals (Anti-Scope)

- NG-1: Building a general-purpose lockfile format (hlock already exists)
- NG-2: Building a cryptographic signing library (hlock already provides Ed25519 + ML-DSA-65)
- NG-3: Building graph operations on lockfiles (hlock already provides diff, extract, topo sort, cycle detection)
- NG-4: Building SLSA provenance from scratch (hlock already has InlineSlsa attestation)
- NG-5: AI Oracle for intent-based search (deferred to post-launch)
- NG-6: Auto-migration agents (deferred to post-launch)
- NG-7: CRDT offline mirror (deferred to Phase 5, post-MVP)
- NG-8: Delta updates via Merkle blob splitting (deferred to post-launch)
- NG-9: Package health genome / behavioral profiling (deferred indefinitely)
- NG-10: Quadratic funding pool (deferred indefinitely)

## 9. Operational Concept & High-Level Scenarios

### 9.1 Scenario: Author Publishes a Package

1. Author runs `glyim pkg publish` from a project directory
2. glyim-pkg compiles the package, creates a Wasm blob (if macro) or bytecode artifact
3. Blob is stored in CAS → receives SHA-256 content hash
4. Manifest is analyzed → capabilities and exports are extracted
5. Granularity engine splits artifact into per-export blobs → stored in CAS
6. Package metadata (name, version, hash, deps, capabilities, exports) is sent to `/api/v1/packages/{name}/{version}/upload`
7. Index creates a VersionEntry, appends a Publish event to the event log
8. Trust scorer assigns TrustLevel::New
9. If CI build is configured, a ReproducibleBuild attestation is generated via hlock
10. Author receives confirmation with content hash and trust level

### 9.2 Scenario: Developer Installs a Package

1. Developer runs `glyim pkg install json-utils`
2. glyim-pkg queries `/api/v1/packages/json-utils` → gets version list
3. Resolver selects latest compatible version (or uses server-side `/api/v1/resolve`)
4. Client generates hlock Lockfile with BLAKE3 integrity hashes per package and per-export
5. For each package in lockfile, client fetches only the needed export blobs from CAS
6. BLAKE3 digests are verified against lockfile entries
7. If lockfile is signed, ML-DSA-65 signature is verified
8. Package artifacts are placed in local cache
9. Total install time: <50ms for cached, <200ms for cold

### 9.3 Scenario: Temporal Resolution

1. Developer adds `resolve_as_of = "2025-03-15T10:00:00Z"` to glyim.toml
2. On `glyim pkg install`, resolver sends request with timestamp to `/api/v1/resolve`
3. Server replays event log up to the given timestamp
4. Resolution is performed against the index as it existed on that date
5. Lockfile is generated with temporal pinning metadata
6. Future installs with the same lockfile reproduce identical dependency graph

### 9.4 Scenario: Trust Progression

1. Package `crypto-helpers@1.0.0` is published → TrustLevel::New
2. After 30 days and 100+ downloads → TrustLevel::Established
3. Automated audit passes, reproducible build verified → TrustLevel::Verified
4. Manual security review by certified auditor → TrustLevel::Certified
5. At each transition, a new attestation is appended to the event log
6. Consumers can set minimum trust thresholds in their manifests

### 9.5 Scenario: Capability Search

1. Developer needs JSON parsing but doesn't know which package to use
2. Developer runs `glyim pkg search "json.parse"`
3. Registry searches capability index → returns packages that declare `json.parse`
4. Results ranked by trust level and relevance
5. Developer selects package and installs

## 10. Stakeholders, Sponsorship, and Governance

| Role | Person/Group | Responsibility |
|------|-------------|----------------|
| Executive Sponsor | Glyim Project Lead | Approves scope, budget, and release decisions |
| Product Owner | Lead Engineer | Owns vision document, prioritizes features |
| Architecture Owner | Senior Engineer | Owns ADRs, reviews design docs |
| Engineering Team | 2–4 engineers | Implements modules, writes tests |
| Security Advisor | External auditor | Reviews trust framework, validates crypto |

**Decision Model**: Architecture decisions require ADRs reviewed by Architecture Owner. Feature changes require approval from Product Owner. Crypto changes require Security Advisor sign-off.

**Review Cadence**: Vision reviewed annually or on pivot. Strategy reviewed quarterly.

## 11. Risks, Assumptions, and Open Questions

### 11.1 Risks

| ID | Risk | Likelihood | Impact | Mitigation |
|----|------|-----------|--------|------------|
| R-1 | hlock API changes break integration | Medium | High | Pin hlock version; write adapter layer |
| R-2 | Function-level splitting is inaccurate | Medium | Medium | Start with conservative splitting; iterate |
| R-3 | PQ signature performance is too slow | Low | High | Benchmark early; fallback to Ed25519 |
| R-4 | Event log grows unbounded | High | Medium | Implement compaction with checkpoint snapshots |
| R-5 | Trust scoring is gamed | Medium | Medium | Weight audit and reproducibility heavily |

### 11.2 Assumptions

| ID | Assumption | If False |
|----|-----------|----------|
| A-1 | hlock provides all necessary crypto primitives | Must build signing library from scratch (3+ weeks) |
| A-2 | CAS server can be extended without breaking existing clients | Must fork or wrap CAS server |
| A-3 | Glyim macro exports can be statically analyzed at publish time | Granularity falls back to whole-package delivery |
| A-4 | BLAKE3 is acceptable as the primary hash algorithm | Must support SHA-256 fallback |

### 11.3 Open Questions

| ID | Question | Due Date | Owner |
|----|---------|----------|-------|
| OQ-1 | Should the event log use JSONL or a binary format for production? | 2025-07-25 | Architecture Owner |
| OQ-2 | What is the minimum trust level for packages in the default search results? | 2025-08-01 | Product Owner |
| OQ-3 | Should granularity splitting happen at publish time or at install time? | 2025-07-18 | Engineering Team |
| OQ-4 | How many replicas must agree for a ReproducibleBuild attestation? | 2025-08-15 | Security Advisor |
```

---

## Document 2: Business & Stakeholder Requirements Specification

```markdown
# Glyim Registry — Business & Stakeholder Requirements Specification

| Field | Value |
|-------|-------|
| Project | Glyim Registry |
| Document | Business & Stakeholder Requirements Specification (BStRS) |
| Version | 1.0 |
| Date | 2025-07-11 |
| Author | Glyim Team |
| Status | Approved |
| Traces From | Vision v1.0 (G-1 through G-10) |

---

## 1. Introduction

This document defines what the business and stakeholders need from the Glyim Registry, expressed in business and domain language. It is deliberately implementation-free—no APIs, no UI designs, no technology choices. Those belong in the SRS.

### 1.1 Scope

The Glyim Registry encompasses:
- A **package index service** that maps package names and versions to content hashes
- A **dependency resolution service** that computes optimal dependency graphs
- A **trust framework** that assigns progressive trust levels to packages
- A **granularity engine** that enables function-level package delivery
- A **capability discovery system** that allows searching by functionality rather than name

Out of scope for this document: AI Oracle, auto-migration, CRDT mirror, delta updates.

## 2. Definitions, Acronyms, and Abbreviations

### 2.1 Glossary (Ubiquitous Language)

| Term | Definition | Notes |
|------|-----------|-------|
| **Package** | A named, versioned unit of reusable Glyim code that can be published and installed | Synonyms: "crate", "library". We use "package" exclusively. |
| **Macro** | A package that contains Wasm-compiled code executed at compile time | Macros are a subset of packages with special trust requirements. |
| **Artifact** | A compiled binary representation of a package, stored in the CAS | One package version may have multiple artifacts (different platforms). |
| **Export** | A named symbol that a package makes available to consumers | e.g., `json.parse`, `http.get`. Each export has its own integrity hash. |
| **Capability** | A declared functionality that a package provides | e.g., `json.parse`, `json.stringify`. Used for discovery. |
| **Content Hash** | A cryptographic digest (BLAKE3 or SHA-256) identifying a blob in the CAS | The CAS stores and retrieves blobs exclusively by content hash. |
| **Trust Level** | A progressive classification of package reliability: New, Established, Verified, Certified | Higher levels require more evidence (age, audits, reproducibility). |
| **Lockfile** | A cryptographic record of the exact packages, versions, and hashes installed in a project | In Glyim, lockfiles use the hlock binary format. |
| **Temporal Resolution** | Resolving dependencies as the index existed at a specific past timestamp | Enables reproducible builds without frozen lockfiles. |
| **Granularity** | The ability to install only specific exports from a package, not the entire artifact | Reduces download size and attack surface. |
| **Attestation** | A signed statement about a package's provenance, build, or review | Types: AuthorPublish, CIBuild, AuditReview, ReproducibleBuild. |
| **Event Log** | An append-only record of all mutations to the package index | Enables temporal resolution and audit trails. |
| **Index** | The mapping of package names and versions to content hashes and metadata | The "nervous system" connecting names to storage. |
| **CAS** | Content-Addressable Storage — stores and retrieves blobs by hash | The "muscle" of the registry. Already implemented. |
| **Publisher** | The identity (person or CI system) that published a package version | Identified by public key hash. |
| **Yank** | Marking a published version as "do not use" without deleting it | Yanked versions remain in the CAS but are excluded from resolution. |

### 2.2 Forbidden Terms

| Do Not Use | Use Instead | Reason |
|------------|-------------|--------|
| "Crate" | Package | Avoids Rust ecosystem confusion |
| "Module" | Package or Export | "Module" is ambiguous (could mean code module or package) |
| "Registry" (alone) | Registry System or Index | "Registry" conflates the index, the CAS, and the client |
| "User" (alone) | Application Developer, Library Author, etc. | "User" is too vague |

## 3. Business Context

### 3.1 Business Purpose

The Glyim Registry exists to make Glyim the most trustworthy and efficient language ecosystem by ensuring that every package install is cryptographically verified, minimally sized, and temporally reproducible.

### 3.2 Business Problem

Currently, the Glyim CAS server can store and retrieve blobs, and the Glyim package manager can resolve dependencies locally—but they are disconnected. There is no index layer mapping package names to content hashes. Developers cannot install packages by name, cannot verify trust, cannot install only what they use, and cannot reproduce past dependency states.

### 3.3 Business Scope

**In Scope**:
- Package name→hash indexing
- Server-side dependency resolution
- Progressive trust scoring
- Function-level delivery
- Capability-based discovery
- Temporal resolution
- Post-quantum signing of published artifacts

**Out of Scope** (for this release):
- AI-powered search (OQ deferred)
- Automated code migration (OQ deferred)
- Offline-first CRDT mirrors (Phase 5+)
- Delta/binary-diff updates (post-launch)
- Package health genome (indefinitely deferred)

## 4. Business Goals, Objectives, and Success Metrics

| ID | Business Objective | Fit Criterion / Key Result | Traces From |
|----|-------------------|---------------------------|-------------|
| BG-01 | Establish a fully functional package registry for Glyim | 100% of published packages are discoverable and installable by name within 1 second | G-4 |
| BG-02 | Make every install cryptographically verifiable | 100% of installed packages carry valid PQ signatures and BLAKE3 integrity proofs | G-1, G-10 |
| BG-03 | Reduce average install size by 5× | Median install uses ≤20% of full artifact bytes via granular delivery | G-7 |
| BG-04 | Enable fully reproducible dependency resolution | Any past resolution can be reconstructed from the event log within 5 seconds | G-9 |
| BG-05 | Make supply-chain risk visible and progressive | 80%+ of installed packages have trust level ≥ Established within 6 months of launch | G-6 |

## 5. Business Model and Processes

### 5.1 Value Propositions

| For | We Provide | So They Can |
|-----|-----------|------------|
| Application Developers | Verifiable, minimal package installs | Ship faster with less risk |
| Library Authors | Progressive trust with automated attestation | Build reputation over time |
| Security Auditors | Complete audit trail with PQ signatures | Verify provenance end-to-end |
| Enterprise IT | Policy-governed package consumption | Enforce compliance at scale |

### 5.2 Core Business Processes

**BP-1: Package Publication**
1. Author prepares package source and manifest
2. Package is compiled and stored in CAS
3. Package metadata is submitted to the index
4. Event log records the publication
5. Trust scorer assigns initial trust level
6. Attestations are generated (if applicable)

**BP-2: Package Installation**
1. Consumer requests package by name
2. Index resolves name to version and content hash
3. Dependencies are resolved (client or server-side)
4. Lockfile is generated with integrity hashes
5. Only needed exports are fetched from CAS
6. Signatures and integrity are verified

**BP-3: Trust Progression**
1. Package is published at TrustLevel::New
2. Automated signals (age, downloads, audit results) are collected
3. Trust scorer recomputes trust level periodically
4. Level transitions are recorded in the event log
5. Consumers can filter by minimum trust level

**BP-4: Temporal Resolution**
1. Consumer specifies a timestamp in their manifest
2. Event log is replayed up to that timestamp
3. Index state is reconstructed as it existed at that moment
4. Resolution is performed against the historical index
5. Result is identical to what would have been produced on that date

## 6. Business Rules and Policies

| ID | Rule | Source | Impact on Requirements |
|----|------|--------|----------------------|
| BR-01 | A package name, once published, cannot be reassigned to a different publisher without explicit transfer | Security policy | Requires Transfer event in event log |
| BR-02 | A version, once published, cannot be modified—only yanked or deprecated | Immutability policy | CAS content-addressing enforces this; index must reject duplicate versions |
| BR-03 | Packages with trust level "New" are excluded from default search results | Trust policy | Search endpoint must support trust-level filtering |
| BR-04 | All cryptographic operations must use approved algorithms: Ed25519, ML-DSA-65, BLAKE3, SHA-256 | Crypto policy | No SHA-1, no RSA, no ECDSA |
| BR-05 | Post-quantum signatures (ML-DSA-65) are mandatory for all published packages | PQ policy | Sign-on-publish pipeline must default to ML-DSA-65 |
| BR-06 | Event log entries are never deleted, only appended | Audit policy | Compaction must preserve audit trail |
| BR-07 | Yanked versions remain in the CAS but are excluded from resolution | Yank policy | Resolver must filter yanked versions |
| BR-08 | Attestations must be signed by a different key than the package author when possible | Independence policy | CI and audit attestations use separate keys |

## 7. Stakeholders and User Classes

### 7.1 Stakeholder Map

| Stakeholder | Influence | Interest | Primary Concern |
|-------------|----------|---------|-----------------|
| Application Developer | High | High | Fast, trustworthy installs |
| Library Author | High | High | Easy publishing, trust progression |
| Macro Author | Medium | High | Reproducible build verification |
| Security Auditor | Medium | High | Complete provenance chain |
| Enterprise IT | Medium | Medium | Policy enforcement, private mirrors |
| DevOps Engineer | Low | Medium | Deterministic builds, caching |
| Glyim Project Lead | High | High | Strategic alignment, timeline |

### 7.2 User Classes

| Class | Type | Description | JTBD |
|-------|------|-------------|------|
| **App Developer (Primary)** | Primary | Builds Glyim applications, consumes packages daily | "When I need a library, I want to find and install a trustworthy version in under 10 seconds, so I can focus on building my app." |
| **Lib Author (Primary)** | Primary | Publishes reusable packages, builds reputation | "When I publish a package, I want automatic trust progression, so my users know it's safe." |
| **Macro Author (Secondary)** | Secondary | Publishes compile-time Wasm macros | "When I publish a macro, I want reproducible build verification, so users trust my macro doesn't do anything unexpected." |
| **Security Auditor (Secondary)** | Secondary | Reviews package provenance and trust | "When I audit a package, I want a complete, tamper-proof history of all attestations, so I can verify the supply chain." |
| **Enterprise Admin (Disfavored)** | Disfavored | Manages organizational package policies | Has needs but not the primary design target for v1 |

### 7.3 Key Personas

**Persona 1: Alex, Application Developer**
- Writes Glyim web services
- Installs 10–20 packages per project
- Concerned about supply-chain attacks
- Wants: `glyim pkg install X` to "just work" and be safe

**Persona 2: Morgan, Library Author**
- Maintains 3 popular Glyim libraries
- Publishes updates weekly
- Wants: automatic trust progression, easy CI integration

**Persona 3: Sam, Security Auditor**
- Reviews packages for enterprise compliance
- Needs: complete provenance chain, tamper evidence
- Uses: trust reports, attestation logs

## 8. Domain Model

### 8.1 Conceptual Domain Model

```
Package ──1:N──→ Version
Version ──1:1──→ Artifact
Version ──1:N──→ Export
Version ──1:N──→ Capability
Version ──1:N──→ Attestation
Version ──1:N──→ Dependency
Package ──N:N──→ Publisher
Version ──1:1──→ TrustLevel
EventLog ──1:N──→ Event
Event ───────→ Version (references)
```

### 8.2 Bounded Contexts

| Context | Responsibility | Key Terms |
|---------|---------------|-----------|
| **Index** | Name→hash mapping, version management, search | Package, Version, Entry, Search |
| **Storage** | Content-addressable blob storage | Blob, Hash, CAS |
| **Resolution** | Dependency graph computation | Constraint, Resolution, Lockfile |
| **Trust** | Trust scoring, attestation management | TrustLevel, Attestation, Provenance |
| **Granularity** | Per-export splitting and delivery | Export, Blob, GranularityManifest |
| **Capabilities** | Capability declaration and discovery | Capability, Contract, Sandbox |

### 8.3 Context Map

```
              ┌─────────────────┐
              │   Index (Core)   │
              │  Package↔Version │
              └────────┬─────────┘
                       │
         ┌─────────────┼─────────────┐
         │             │             │
    ┌────┴────┐  ┌─────┴─────┐  ┌───┴──────┐
    │ Storage │  │ Resolution│  │  Trust   │
    │  (CAS)  │  │  (Solver) │  │ (Scorer) │
    └─────────┘  └───────────┘  └──────────┘
         │             │             │
    ┌────┴─────────────┴─────────────┴────┐
    │          Client (glyim-pkg)          │
    └─────────────────────────────────────┘
```

## 9. Stakeholder Needs and User Requirements

| ID | Stakeholder Need | User Class | Priority | Traces From |
|----|-----------------|------------|----------|-------------|
| SN-01 | Discover packages by name | App Developer | Must | BG-01 |
| SN-02 | Discover packages by capability | App Developer | Should | BG-01 |
| SN-03 | Install packages with cryptographic verification | App Developer | Must | BG-02 |
| SN-04 | Install only the code I use | App Developer | Should | BG-03 |
| SN-05 | Publish packages with automatic trust assignment | Lib Author | Must | BG-05 |
| SN-06 | See trust progression over time | Lib Author | Should | BG-05 |
| SN-07 | Get reproducible build attestations | Macro Author | Must | BG-02 |
| SN-08 | Reproduce past dependency states | DevOps Engineer | Should | BG-04 |
| SN-09 | Audit complete package provenance | Security Auditor | Must | BG-02 |
| SN-10 | Set minimum trust thresholds | Enterprise Admin | Could | BG-05 |
| SN-11 | Search for packages that provide a specific capability | App Developer | Should | BG-01 |
| SN-12 | Resolve dependencies server-side for consistency | DevOps Engineer | Should | BG-04 |

## 10. System-in-Context and Operational Concept

### 10.1 System Boundary

The Glyim Registry System consists of:
- The **CAS Server** (existing, extended) — stores and retrieves blobs
- The **Index Service** (new) — maps names to hashes, provides search
- The **Resolution Service** (new) — computes dependency graphs
- The **Client** (existing, extended) — glyim-pkg, interacts with all services

External systems:
- CI/CD pipelines (produce build attestations)
- Developer workstations (run glyim-pkg)
- External auditors (produce audit attestations)

### 10.2 Operational Concept

The registry operates in a request-response model:
1. **Publishing** is initiated by developers or CI systems
2. **Resolution** is performed server-side with client fallback
3. **Installation** is initiated by developers or CI systems
4. **Trust scoring** runs as a background process on the server
5. **Temporal queries** are served by replaying the event log

## 11. Stakeholder-Level Constraints and Quality Expectations

| ID | Constraint / Expectation | Source |
|----|-------------------------|--------|
| SC-01 | Package lookups must complete in <100ms at p99 under normal load | App Developer expectation |
| SC-02 | Resolution of 100-package dependency graph must complete in <5s | DevOps expectation |
| SC-03 | The registry must remain available during CAS server maintenance | Operational requirement |
| SC-04 | All published data must be immutable once written | Audit requirement |
| SC-05 | Cryptographic verification must not add more than 50ms to install time | App Developer expectation |
| SC-06 | The system must handle at least 100 publishes per hour at launch | Scale estimate |
| SC-07 | Package metadata must be human-readable and machine-parseable | Tooling requirement |

## 12. Business and Stakeholder-Level Success Metrics

| Metric | Measurement Method | Target |
|--------|-------------------|--------|
| Package discoverability | % of published packages findable by name within 1s | 100% |
| Install trust coverage | % of installed packages with trust level ≥ Established | ≥ 80% within 6 months |
| Install size reduction | Median ratio of granular install size to full artifact size | ≤ 20% |
| Resolution reproducibility | % of temporal resolutions that produce identical results | 100% |
| Publication latency | Time from publish command to index availability | <5s |
| Install latency (cold) | Time from install command to package available locally | <200ms |

## 13. Risks, Assumptions, and Open Issues

| ID | Item | Type | Status | Traces From |
|----|------|------|--------|-------------|
| BRS-R-01 | hlock binary format may not support all VersionEntry fields | Risk | Open | A-1 |
| BRS-R-02 | Trust scoring weights may be contested by community | Risk | Open | R-5 |
| BRS-A-01 | hlock's CasHttp source type matches our CAS endpoint pattern | Assumption | Validating | A-1 |
| BRS-OQ-01 | Should capability declarations be validated at publish time? | Open Question | Open | OQ-3 |

## 14. Traceability Mapping to Vision

| Business Goal | Stakeholder Need | Feature/Capability | SRS Section |
|---------------|-----------------|-------------------|-------------|
| BG-01 | SN-01, SN-02 | Package Index, Search | SRS §4.1, §4.6 |
| BG-02 | SN-03, SN-07, SN-09 | Trust Framework, Signing | SRS §4.3, §4.4 |
| BG-03 | SN-04 | Granularity Engine | SRS §4.5 |
| BG-04 | SN-08, SN-12 | Temporal Resolution, Server Resolution | SRS §4.2 |
| BG-05 | SN-05, SN-06, SN-10 | Trust Scoring, Progressive Trust | SRS §4.3 |
```

---

## Document 3: Software Requirements Specification

```markdown
# Glyim Registry — Software Requirements Specification

| Field | Value |
|-------|-------|
| Project | Glyim Registry |
| Document | Software Requirements Specification (SRS) |
| Version | 1.0 |
| Date | 2025-07-11 |
| Author | Glyim Team |
| Status | Approved |
| Traces From | BStRS v1.0 (BG-01 through BG-05, SN-01 through SN-12) |

---

## 1. Introduction and Scope

### 1.1 Purpose

This document specifies the software requirements for the Glyim Registry System, comprising the package index service, dependency resolver, trust framework, granularity engine, and capability discovery system. It defines what the software shall do and how well it shall perform.

### 1.2 Scope

**In Scope**: All software components that implement the Glyim Registry, including:
- glyim-registry-index (package name→hash mapping)
- glyim-registry-resolver (server-side dependency resolution)
- glyim-registry-trust (progressive trust scoring)
- glyim-registry-granularity (function-level splitting)
- glyim-registry-capabilities (capability contracts and discovery)
- Extensions to glyim-cas-server (new REST endpoints)
- Extensions to glyim-pkg (hlock integration, new client methods)

**Out of Scope**: AI Oracle, auto-migration, CRDT mirror, delta updates, package health genome.

### 1.3 References

- Vision & Strategic Alignment v1.0
- Business & Stakeholder Requirements Specification v1.0
- hlock crate documentation (Ed25519, ML-DSA-65, BLAKE3, SLSA)
- ISO/IEC 25010:2023 (Quality Characteristics)
- FIPS 204 (ML-DSA Post-Quantum Signatures)

## 2. System Context and Overview

### 2.1 System Context

```
┌──────────────┐     ┌──────────────┐     ┌──────────────┐
│   Developer   │     │  CI/CD Pipe  │     │   Auditor    │
│  Workstation  │     │   (GitHub)   │     │ Workstation  │
└──────┬────────┘     └──────┬───────┘     └──────┬───────┘
       │                     │                     │
       ▼                     ▼                     ▼
┌──────────────────────────────────────────────────────────┐
│                    glyim-pkg (Client)                     │
│  Manifest Parser │ Resolver │ CAS Client │ Registry Client│
└──────────────────────┬───────────────────────────────────┘
                       │
                       ▼
┌──────────────────────────────────────────────────────────┐
│              glyim-cas-server (REST + gRPC)               │
│                                                          │
│  ┌──────────┐  ┌───────────┐  ┌───────────┐  ┌────────┐│
│  │ CAS Blobs│  │   Index   │  │ Event Log │  │Resolver││
│  └──────────┘  └───────────┘  └───────────┘  └────────┘│
└──────────────────────────────────────────────────────────┘
```

### 2.2 External Interfaces

| Interface | Type | Direction | Description |
|-----------|------|-----------|-------------|
| REST API | HTTP/JSON | Bidirectional | Package index operations, resolution, search |
| gRPC API | Bazel RE Protocol | Bidirectional | CAS blob operations (existing) |
| hlock Format | Binary | Output | Lockfile serialization from resolver |
| Filesystem | Local | Bidirectional | CAS storage, event log, index snapshots |

## 3. Functional Capabilities and Behavior

### 3.1 Package Index (REQ-INDEX)

**Goal**: Enable package discovery and version resolution by name.

**REQ-FUNC-001**: The system shall accept a publish request containing a package name, version, content hash, dependency list, and capability declarations, and shall create a new index entry if the version does not already exist.

- **EARS Pattern**: Event-driven — "When a publish request is received for a package name and version that does not exist in the index, the system shall create a new VersionEntry and append a Publish event to the event log."
- **Priority**: Must
- **Traces From**: BG-01, SN-01

**REQ-FUNC-002**: The system shall reject a publish request for a package name and version that already exists in the index.

- **EARS Pattern**: Unwanted behavior — "If a publish request is received for a package name and version that already exists, the system shall return an error response with code 409 and a descriptive message, and shall not modify the index."
- **Priority**: Must
- **Traces From**: BR-02

**REQ-FUNC-003**: The system shall return all non-yanked versions of a package given its name, sorted by semver descending.

- **Priority**: Must
- **Traces From**: SN-01

**REQ-FUNC-004**: The system shall return the VersionEntry for a specific package name and version.

- **Priority**: Must
- **Traces From**: SN-01

**REQ-FUNC-005**: The system shall accept a yank request for a package name and version, mark the version as yanked, and append a Yank event to the event log.

- **EARS Pattern**: Event-driven — "When a yank request is received for an existing, non-yanked version, the system shall set the yanked flag to true and append a Yank event."
- **Priority**: Must
- **Traces From**: BR-07

**REQ-FUNC-006**: Yanked versions shall be excluded from version listings and dependency resolution by default.

- **Priority**: Must
- **Traces From**: BR-07

**REQ-FUNC-007**: The system shall search packages by name prefix and return matching package names with their latest version summary.

- **Priority**: Must
- **Traces From**: SN-01, SN-02

**REQ-FUNC-008**: The system shall search packages by declared capability and return matching packages with capability relevance scores.

- **Priority**: Should
- **Traces From**: SN-02, SN-11

### 3.2 Dependency Resolution (REQ-RESOLVE)

**Goal**: Compute optimal dependency graphs and produce hlock Lockfiles.

**REQ-FUNC-010**: The system shall accept a resolution request containing a set of root dependency constraints, an optional timestamp, and an optimization objective, and shall return an hlock Lockfile representing the resolved dependency graph.

- **Priority**: Must
- **Traces From**: BG-04, SN-08, SN-12

**REQ-FUNC-011**: When a timestamp is provided in the resolution request, the system shall replay the event log up to that timestamp, reconstruct the index state as it existed at that moment, and resolve against the historical index.

- **EARS Pattern**: State-driven — "While resolving with a timestamp, the system shall use the index state as it existed at the specified timestamp."
- **Priority**: Should
- **Traces From**: BG-04, SN-08

**REQ-FUNC-012**: The system shall support the following optimization objectives: MinimalVersions, LatestVersions, MaximalTrust.

- **Priority**: Should
- **Traces From**: SN-12

**REQ-FUNC-013**: The system shall cache resolution results by computing a deterministic cache key from the input parameters and storing the result in the CAS.

- **Priority**: Should
- **Traces From**: SN-12

**REQ-FUNC-014**: When a dependency cannot be found in the index, the system shall return an error response identifying the missing package and the constraint that could not be satisfied.

- **EARS Pattern**: Unwanted behavior — "If a dependency cannot be found in the index, the system shall return an error response with the package name and unsatisfiable constraint."
- **Priority**: Must
- **Traces From**: BG-01

**REQ-FUNC-015**: When a dependency cycle is detected, the system shall return an error response with the cycle path.

- **EARS Pattern**: Unwanted behavior — "If a dependency cycle is detected during resolution, the system shall return an error response listing the packages in the cycle."
- **Priority**: Must

**REQ-FUNC-016**: The resolver shall output an hlock Lockfile struct with BLAKE3 integrity hashes for each resolved package.

- **Priority**: Must
- **Traces From**: BG-02, C-2

**REQ-FUNC-017**: The resolver shall support version constraints including exact, caret, and wildcard patterns.

- **Priority**: Must
- **Traces From**: BG-01

**REQ-FUNC-018**: The resolver shall exclude yanked versions from consideration during resolution.

- **Priority**: Must
- **Traces From**: BR-07

### 3.3 Trust Framework (REQ-TRUST)

**Goal**: Assign and track progressive trust levels for packages.

**REQ-FUNC-020**: The system shall assign TrustLevel::New to every newly published package version.

- **Priority**: Must
- **Traces From**: BG-05, SN-05

**REQ-FUNC-021**: The system shall compute a trust score based on the following signals: age in days, download count, publisher count, audit pass status, reproducible build status, author signature, CI attestation, community attestation count, known vulnerability count, and formal verification status.

- **Priority**: Must
- **Traces From**: BG-05

**REQ-FUNC-022**: The system shall map trust scores to trust levels as follows: 0–29 → New, 30–59 → Established, 60–89 → Verified, 90+ → Certified.

- **Priority**: Must
- **Traces From**: BG-05

**REQ-FUNC-023**: The system shall return a trust report for a given package name and version, including the current trust level, score breakdown, attestation list, and vulnerability count.

- **Priority**: Must
- **Traces From**: SN-06, SN-09

**REQ-FUNC-024**: The system shall accept attestation submissions containing a content hash, signature, signer public key, and attestation kind, and shall verify the signature before storing the attestation.

- **EARS Pattern**: Event-driven — "When an attestation is submitted with a valid signature, the system shall store it and update the package's trust score."
- **Priority**: Must
- **Traces From**: BG-02, SN-07, SN-09

**REQ-FUNC-025**: The system shall reject attestation submissions with invalid signatures.

- **EARS Pattern**: Unwanted behavior — "If an attestation is submitted with an invalid signature, the system shall reject it and return an error response."
- **Priority**: Must
- **Traces From**: BR-04

**REQ-FUNC-026**: The system shall generate a ReproducibleBuild attestation automatically when the `/verify-wasm` endpoint confirms a successful reproducible build.

- **Priority**: Should
- **Traces From**: SN-07, BG-02

**REQ-FUNC-027**: The system shall support Ed25519 and ML-DSA-65 signature algorithms for attestations.

- **Priority**: Must
- **Traces From**: BR-04, BR-05

### 3.4 Event Log (REQ-EVENTS)

**Goal**: Maintain an append-only audit trail enabling temporal resolution.

**REQ-FUNC-030**: The system shall maintain an append-only event log recording all index mutations (Publish, Yank, Unyank, Deprecate, Transfer).

- **Priority**: Must
- **Traces From**: BG-04, BR-06

**REQ-FUNC-031**: Each event shall contain a unique sequential ID, an ISO 8601 timestamp, and a typed payload.

- **Priority**: Must
- **Traces From**: BR-06

**REQ-FUNC-032**: The system shall never delete or modify existing events.

- **Priority**: Must
- **Traces From**: BR-06

**REQ-FUNC-033**: The system shall support replay of events up to an arbitrary timestamp to reconstruct the index state at that point in time.

- **Priority**: Must
- **Traces From**: BG-04

**REQ-FUNC-034**: The system shall support replay of all events to reconstruct the current index state.

- **Priority**: Must
- **Traces From**: BG-01

**REQ-FUNC-035**: On server startup, the system shall replay the event log to rebuild the in-memory index.

- **Priority**: Must
- **Traces From**: BG-01

### 3.5 Granularity Engine (REQ-GRANULARITY)

**Goal**: Enable function-level package delivery.

**REQ-FUNC-040**: The system shall accept a granular install request containing a package name, version, and list of requested exports, and shall return the minimal set of CAS blobs needed to provide those exports.

- **Priority**: Should
- **Traces From**: BG-03, SN-04

**REQ-FUNC-041**: The system shall store a GranularityManifest for each published package, mapping each export name to its CAS blob hash.

- **Priority**: Should
- **Traces From**: BG-03

**REQ-FUNC-042**: When a GranularityManifest is not available for a package, the system shall return the full artifact as a fallback.

- **EARS Pattern**: Optional feature — "Where a GranularityManifest is available, the system shall return per-export blobs; where it is not available, the system shall return the full artifact."
- **Priority**: Should
- **Traces From**: A-3

**REQ-FUNC-043**: The GranularityManifest shall be populated during the publish workflow by analyzing the package's exported symbols.

- **Priority**: Should
- **Traces From**: BG-03, A-3

**REQ-FUNC-044**: Each export in the GranularityManifest shall be represented as an hlock Export struct with identifier, HashAlgorithm::Blake3, and digest.

- **Priority**: Should
- **Traces From**: C-2

### 3.6 Capability Discovery (REQ-CAPABILITY)

**Goal**: Enable package discovery by declared functionality.

**REQ-FUNC-050**: The system shall accept capability declarations as part of the package manifest, consisting of a list of provided capabilities and a list of required capabilities.

- **Priority**: Should
- **Traces From**: SN-02, SN-11

**REQ-FUNC-051**: The system shall index capabilities at publish time, creating a capability→package inverted index.

- **Priority**: Should
- **Traces From**: SN-02

**REQ-FUNC-052**: The system shall return search results ranked by capability relevance when a capability query is provided.

- **Priority**: Should
- **Traces From**: SN-02, SN-11

**REQ-FUNC-053**: The system shall support fuzzy capability matching (e.g., "json.parse" matches "json.parser").

- **Priority**: Could
- **Traces From**: SN-11

### 3.7 Client Integration (REQ-CLIENT)

**Goal**: glyim-pkg uses hlock lockfiles and communicates with the registry.

**REQ-FUNC-060**: The glyim-pkg client shall use hlock's binary lockfile format with BLAKE3 whole-file digest for all lockfile operations.

- **Priority**: Must
- **Traces From**: C-2, BG-02

**REQ-FUNC-061**: The glyim-pkg client shall support server-side resolution via the `/api/v1/resolve` endpoint, falling back to local resolution if the server is unavailable.

- **Priority**: Should
- **Traces From**: SN-12

**REQ-FUNC-062**: The glyim-pkg client shall verify ML-DSA-65 signatures on lockfiles before using them.

- **Priority**: Must
- **Traces From**: BR-05, BG-02

**REQ-FUNC-063**: The glyim-pkg client shall support granular installs, requesting only specific exports from packages when a GranularityManifest is available.

- **Priority**: Should
- **Traces From**: SN-04

**REQ-FUNC-064**: The glyim-pkg manifest shall support capability declarations in a `[package.capabilities]` section.

- **Priority**: Should
- **Traces From**: SN-02

**REQ-FUNC-065**: The glyim-pkg manifest shall support sandbox constraint declarations in a `[package.sandbox]` section.

- **Priority**: Could
- **Traces From**: SC-07

## 4. Quality and Non-Functional Requirements

### 4.1 Performance Efficiency (ISO 25010)

**REQ-NFR-PERF-001**: The index service shall respond to package lookup requests (`GET /api/v1/packages/{name}`) within 100ms at p95 under a sustained load of 100 requests per second.

- **Fit Criterion**: p95 latency ≤ 100ms measured at the HTTP endpoint over a 5-minute window under 100 RPS load.
- **Verification**: Load test with k6, threshold: `http_req_duration["p(95)"] < 100`
- **Traces From**: SC-01

**REQ-NFR-PERF-002**: The resolution service shall resolve a dependency graph of up to 100 packages within 5 seconds at p95.

- **Fit Criterion**: p95 latency ≤ 5000ms for resolution requests with ≤100 packages.
- **Verification**: Load test with synthetic dependency graphs.
- **Traces From**: SC-02

**REQ-NFR-PERF-003**: Cryptographic verification (ML-DSA-65 signature + BLAKE3 digest) shall add no more than 50ms to the install process per package.

- **Fit Criterion**: Verification time per package ≤ 50ms measured over 1000 verifications.
- **Verification**: Micro-benchmark.
- **Traces From**: SC-05

**REQ-NFR-PERF-004**: The event log replay for 10,000 events shall complete within 2 seconds.

- **Fit Criterion**: Full replay of 10K events ≤ 2 seconds.
- **Verification**: Unit test with generated event log.
- **Traces From**: BG-04

### 4.2 Reliability (ISO 25010)

**REQ-NFR-REL-001**: The registry service shall achieve 99.9% availability measured monthly (excluding scheduled maintenance).

- **Fit Criterion**: ≥ 99.9% successful HTTP responses over calendar month.
- **Verification**: Monitoring dashboard + SLO compliance report.

**REQ-NFR-REL-002**: The system shall recover from a single-process failure within 60 seconds without data loss.

- **Fit Criterion**: Service availability restored ≤ 60s after process kill; zero event log entries lost.
- **Verification**: Chaos test.

**REQ-NFR-REL-003**: The event log shall be durable across process restarts.

- **Fit Criterion**: All events written before a crash are present after restart.
- **Verification**: Integration test with process kill mid-write.

### 4.3 Security (ISO 25010)

**REQ-NFR-SEC-001**: All cryptographic operations shall use only approved algorithms: Ed25519, ML-DSA-65, BLAKE3, SHA-256.

- **Fit Criterion**: No other algorithm identifiers appear in code paths for signing, verification, or hashing.
- **Verification**: Code audit + automated lint.

**REQ-NFR-SEC-002**: All attestation signatures shall be verified before storage.

- **Fit Criterion**: 100% of stored attestations pass signature verification; 100% of invalid signature submissions are rejected.
- **Verification**: Unit test with invalid signatures.

**REQ-NFR-SEC-003**: The event log shall be append-only; no API endpoint shall support event deletion or modification.

- **Fit Criterion**: No DELETE or PUT endpoints exist for event log entries.
- **Verification**: API surface audit.

**REQ-NFR-SEC-004**: All HTTP endpoints shall require TLS 1.2 or higher in production.

- **Fit Criterion**: TLS version <1.2 connections are rejected.
- **Verification**: Network-level test.

### 4.4 Maintainability (ISO 25010)

**REQ-NFR-MAIN-001**: Each new module (index, resolver, trust, granularity, capabilities) shall be a separate Rust crate with a well-defined public API.

- **Fit Criterion**: `cargo test --workspace` passes with no cross-crate internal dependencies.
- **Verification**: CI check.

**REQ-NFR-MAIN-002**: All architecture decisions shall be documented as ADRs in `docs/adr/`.

- **Fit Criterion**: One ADR per major decision; ADRs are reviewed in PRs.
- **Verification**: CI check for ADR existence.

### 4.5 Compatibility (ISO 25010)

**REQ-NFR-COMP-001**: The existing CAS server REST and gRPC endpoints shall remain backward-compatible.

- **Fit Criterion**: All existing endpoint contracts (blob store/retrieve, find-missing, action-results, verify-wasm, capabilities) continue to function without breaking changes.
- **Verification**: Regression test suite.

**REQ-NFR-COMP-002**: The hlock lockfile format shall be used as-is, without modification.

- **Fit Criterion**: glyim-pkg uses only hlock's public API; no hlock source modifications.
- **Verification**: Dependency audit.

## 5. External Interfaces and Data Contracts

### 5.1 REST API — Package Index

| Method | Path | Request | Response | Traces To |
|--------|------|---------|----------|-----------|
| GET | `/api/v1/packages/{name}` | — | `{ name, versions: [...] }` | REQ-FUNC-003 |
| GET | `/api/v1/packages/{name}/{version}` | — | VersionEntry | REQ-FUNC-004 |
| POST | `/api/v1/packages/{name}/{version}/upload` | `{ content_hash, is_macro, deps, capabilities, publisher }` | 200 OK / 409 Conflict | REQ-FUNC-001, 002 |
| POST | `/api/v1/packages/{name}/{version}/yank` | — | 200 OK / 404 Not Found | REQ-FUNC-005 |
| GET | `/api/v1/search?q={query}` | — | `[{ name, versions, relevance }]` | REQ-FUNC-007, 008 |

### 5.2 REST API — Resolution

| Method | Path | Request | Response | Traces To |
|--------|------|---------|----------|-----------|
| POST | `/api/v1/resolve` | `{ dependencies, resolve_as_of, objective }` | `{ cache_key, lockfile, package_count }` | REQ-FUNC-010 |
| GET | `/api/v1/resolve/{hash}` | — | Cached resolution result | REQ-FUNC-013 |

### 5.3 REST API — Trust

| Method | Path | Request | Response | Traces To |
|--------|------|---------|----------|-----------|
| POST | `/api/v1/attest` | SignedArtifact | 200 OK / 400 Bad Request | REQ-FUNC-024, 025 |
| GET | `/api/v1/trust/{name}/{version}` | — | TrustReport | REQ-FUNC-023 |

### 5.4 REST API — Granularity

| Method | Path | Request | Response | Traces To |
|--------|------|---------|----------|-----------|
| POST | `/api/v1/granular/{name}/{version}` | `{ imports: [...] }` | `{ blobs: {...}, total_size_bytes }` | REQ-FUNC-040 |

### 5.5 REST API — Temporal

| Method | Path | Request | Response | Traces To |
|--------|------|---------|----------|-----------|
| GET | `/api/v1/snapshot?timestamp={ts}` | — | Index snapshot at timestamp | REQ-FUNC-033 |

### 5.6 Data Contracts

**VersionEntry** (core index record):
- `version`: String (semver)
- `content_hash`: String (BLAKE3 hex)
- `is_macro`: Boolean
- `deps`: Array of `{ name, version_constraint, is_macro }`
- `capabilities`: Array of String
- `trust_level`: Enum { New, Established, Verified, Certified }
- `granularity_manifest_hash`: Optional String
- `published_at`: String (ISO 8601)
- `publisher`: String (public key hash)
- `provenance_hash`: Optional String
- `yanked`: Boolean
- `deprecated`: Optional String

**TrustReport**:
- `trust_level`: String
- `score`: u32
- `score_breakdown`: Object (points per signal)
- `attestations`: Array of AttestationInfo
- `last_audit`: Optional String
- `known_vulnerabilities`: u32

**SignedArtifact** (attestation):
- `content_hash`: String
- `signature`: Base64URL bytes
- `signer_public_key`: Base64URL bytes
- `signed_at`: String (ISO 8601)
- `attestation`: Enum { AuthorPublish, CIBuild, AuditReview, ReproducibleBuild }

## 6. Constraints, Assumptions, and Dependencies

| ID | Constraint | Source |
|----|-----------|--------|
| CON-01 | CAS server must be extended, not replaced | C-1 |
| CON-02 | hlock is the only supported lockfile format | C-2 |
| CON-03 | gRPC Bazel RE protocol cannot be changed | C-3 |
| CON-04 | ML-DSA-65 (FIPS 204) is mandatory for PQ signatures | C-4, BR-05 |
| CON-05 | First functional release in 12 weeks | C-5 |

## 7. TBD Log

| ID | Item | Section | Owner | Due Date |
|----|------|---------|-------|----------|
| TBD-01 | Event log storage format: JSONL vs binary | §3.4 | Architecture Owner | 2025-07-25 |
| TBD-02 | Minimum trust level for default search | §3.3 | Product Owner | 2025-08-01 |
| TBD-03 | Granularity split: publish-time vs install-time | §3.5 | Engineering Team | 2025-07-18 |
| TBD-04 | Number of replicas for ReproducibleBuild consensus | §3.3 | Security Advisor | 2025-08-15 |

## 8. Requirements Attributes and Traceability Model

### 8.1 ID Convention

| Prefix | Meaning |
|--------|---------|
| REQ-FUNC-XXX | Functional requirement |
| REQ-NFR-PERF-XXX | Performance NFR |
| REQ-NFR-REL-XXX | Reliability NFR |
| REQ-NFR-SEC-XXX | Security NFR |
| REQ-NFR-MAIN-XXX | Maintainability NFR |
| REQ-NFR-COMP-XXX | Compatibility NFR |
| CON-XXX | Constraint |
| TBD-XXX | To-be-determined item |

### 8.2 Attributes per Requirement

Each requirement carries: ID, statement, EARS pattern (if applicable), priority (MoSCoW), traces-from (BRS IDs), verification method, status.

### 8.3 Traceability Summary

| SRS Requirement | BRS Need | Vision Goal | Test Artefact |
|----------------|----------|-------------|---------------|
| REQ-FUNC-001 | SN-01, SN-05 | G-4, BG-01 | BDD: publish new package |
| REQ-FUNC-002 | SN-01 | BG-01, BR-02 | BDD: reject duplicate publish |
| REQ-FUNC-003 | SN-01 | BG-01 | BDD: list package versions |
| REQ-FUNC-005 | SN-01 | BG-01, BR-07 | BDD: yank version |
| REQ-FUNC-010 | SN-08, SN-12 | BG-04 | BDD: resolve dependencies |
| REQ-FUNC-011 | SN-08 | BG-04 | BDD: temporal resolution |
| REQ-FUNC-020 | SN-05 | BG-05 | BDD: new package trust level |
| REQ-FUNC-021 | SN-05, SN-06 | BG-05 | Unit: trust score computation |
| REQ-FUNC-024 | SN-07, SN-09 | BG-02 | BDD: submit attestation |
| REQ-FUNC-030 | SN-08 | BG-04, BR-06 | Unit: event log append/immutability |
| REQ-FUNC-040 | SN-04 | BG-03 | BDD: granular install |
| REQ-FUNC-050 | SN-02, SN-11 | BG-01 | BDD: capability search |
| REQ-FUNC-060 | SN-03 | BG-02, C-2 | Integration: hlock lockfile round-trip |
| REQ-NFR-PERF-001 | SC-01 | BG-01 | Load test |
| REQ-NFR-SEC-001 | BR-04 | G-1 | Code audit |
```

---

## Document 4: Architecture & Design Specification

```markdown
# Glyim Registry — Architecture & Design Specification

| Field | Value |
|-------|-------|
| Project | Glyim Registry |
| Document | Architecture & Design Specification |
| Version | 1.0 |
| Date | 2025-07-11 |
| Author | Glyim Team |
| Status | Approved |
| Traces From | SRS v1.0 (REQ-FUNC-*, REQ-NFR-*) |

---

## 1. Context and Scope

This document describes the architecture of the Glyim Registry System, covering the new crates (index, resolver, trust, granularity, capabilities), extensions to existing crates (cas-server, pkg), and how they integrate with the hlock library.

### 1.1 Problem

The CAS server stores blobs but has no concept of package names. The package manager expects a REST API that doesn't exist on the CAS server. The two are disconnected.

### 1.2 Key Constraints

- Extend, don't replace, the CAS server (CON-01)
- Use hlock as the lockfile format (CON-02)
- Maintain Bazel RE gRPC compatibility (CON-03)
- Use ML-DSA-65 for PQ signatures (CON-04)

## 2. Goals and Non-Goals

### 2.1 Goals

- DG-01: Connect CAS server to package index so `glyim pkg install` works end-to-end
- DG-02: Produce hlock Lockfile structs from server-side resolution
- DG-03: Enable temporal resolution via append-only event log
- DG-04: Implement progressive trust scoring with automated attestation
- DG-05: Enable function-level package delivery
- DG-06: Enable capability-addressed discovery

### 2.2 Non-Goals

- DN-01: Build a lockfile format (hlock exists)
- DN-02: Build a crypto library (hlock provides Ed25519 + ML-DSA-65)
- DN-03: Build graph operations (hlock provides diff, extract, topo sort, cycles)
- DN-04: Build AI-powered search
- DN-05: Build CRDT offline mirror
- DN-06: Build delta/binary-diff updates

## 3. Architecturally Significant Requirements

| ASR ID | Requirement | Category | Source |
|--------|------------|----------|--------|
| ASR-01 | Index lookup latency ≤100ms at p95 under 100 RPS | Performance | REQ-NFR-PERF-001 |
| ASR-02 | Resolution of 100 packages ≤5s at p95 | Performance | REQ-NFR-PERF-002 |
| ASR-03 | PQ verification overhead ≤50ms per package | Performance | REQ-NFR-PERF-003 |
| ASR-04 | Event log replay of 10K events ≤2s | Performance | REQ-NFR-PERF-004 |
| ASR-05 | 99.9% monthly availability | Reliability | REQ-NFR-REL-001 |
| ASR-06 | Append-only event log (no deletion API) | Security | REQ-NFR-SEC-003 |
| ASR-07 | Only approved crypto algorithms (Ed25519, ML-DSA-65, BLAKE3, SHA-256) | Security | REQ-NFR-SEC-001 |
| ASR-08 | Backward-compatible CAS endpoints | Compatibility | REQ-NFR-COMP-001 |

## 4. The Design

### 4.1 System Overview

```
┌─────────────────────────────────────────────────────────────────────┐
│                     GLYIM REGISTRY SYSTEM                           │
│                                                                     │
│  ┌────────────────────────────────────────────────────────────┐    │
│  │                glyim-cas-server (REST + gRPC)              │    │
│  │                                                            │    │
│  │  Existing:              New:                               │    │
│  │  ┌──────────┐          ┌──────────┐  ┌─────────────────┐  │    │
│  │  │ CAS Blobs│          │  Index   │  │  Event Log      │  │    │
│  │  │(LocalCS) │◄────────►│(Package) │◄─┤│ (append-only)   │  │    │
│  │  └──────────┘          └────┬─────┘  └─────────────────┘  │    │
│  │                             │                              │    │
│  │  ┌──────────┐          ┌────┴─────┐  ┌─────────────────┐  │    │
│  │  │  gRPC    │          │ Resolver │  │  Trust Scorer   │  │    │
│  │  │(Bazel RE)│          │(Server)  │  │  (Background)   │  │    │
│  │  └──────────┘          └──────────┘  └─────────────────┘  │    │
│  │                                                            │    │
│  │  ┌────────────────────────────────────────────────────┐   │    │
│  │  │              REST API Router                        │   │    │
│  │  │  /blob/*  /api/v1/packages/*  /api/v1/resolve      │   │    │
│  │  │  /verify-wasm  /api/v1/trust/*  /api/v1/granular/* │   │    │
│  │  │  /api/v1/search  /api/v1/snapshot                  │   │    │
│  │  └────────────────────────────────────────────────────┘   │    │
│  └────────────────────────────────────────────────────────────┘    │
│                                │                                    │
│                                ▼                                    │
│  ┌────────────────────────────────────────────────────────────┐    │
│  │                      glyim-pkg (Client)                    │    │
│  │                                                            │    │
│  │  ┌──────────┐  ┌──────────┐  ┌──────────┐  ┌──────────┐  │    │
│  │  │ CAS      │  │ Registry │  │ Resolver │  │ hlock    │  │    │
│  │  │ Client   │  │ Client   │  │ (local + │  │ (format) │  │    │
│  │  │          │  │          │  │  server) │  │          │  │    │
│  │  └──────────┘  └──────────┘  └──────────┘  └──────────┘  │    │
│  └────────────────────────────────────────────────────────────┘    │
│                                                                     │
│  ┌────────────────────────────────────────────────────────────┐    │
│  │                  Library Crates (New)                      │    │
│  │                                                            │    │
│  │  glyim-registry-index        glyim-registry-resolver       │    │
│  │  glyim-registry-trust        glyim-registry-granularity    │    │
│  │  glyim-registry-capabilities                               │    │
│  └────────────────────────────────────────────────────────────┘    │
└─────────────────────────────────────────────────────────────────────┘
```

### 4.2 C4 Model — System Context (Level 1)

```
                    ┌──────────────────┐
                    │  Application     │
                    │  Developer       │
                    └────────┬─────────┘
                             │
                             ▼
┌──────────────┐    ┌────────────────┐    ┌──────────────┐
│  CI/CD       │    │  Glyim Registry│    │  Security    │
│  Pipeline    ├───►│  System        │◄───┤  Auditor     │
└──────────────┘    └────────────────┘    └──────────────┘
                             │
                             ▼
                    ┌──────────────────┐
                    │  CAS Storage     │
                    │  (Filesystem)    │
                    └──────────────────┘
```

### 4.3 C4 Model — Container (Level 2)

```
┌───────────────────────────────────────────────────────────────┐
│                     Glyim Registry System                      │
│                                                               │
│  ┌─────────────────────────────────────────────────────────┐ │
│  │              glyim-cas-server (Rust / Axum)             │ │
│  │                                                         │ │
│  │  ┌────────────┐  ┌────────────┐  ┌──────────────────┐  │ │
│  │  │ CAS Module │  │ Index      │  │ Event Log Module │  │ │
│  │  │ (existing) │  │ Module     │  │ (JSONL files)    │  │ │
│  │  │            │  │ (in-memory │  │                  │  │ │
│  │  │            │  │  + CAS)    │  │                  │  │ │
│  │  └─────┬──────┘  └──────┬─────┘  └────────┬─────────┘  │ │
│  │        │                │                  │             │ │
│  │  ┌─────┴────────────────┴──────────────────┴─────────┐ │ │
│  │  │              REST + gRPC Router                    │ │ │
│  │  └───────────────────────────────────────────────────┘ │ │
│  └─────────────────────────────────────────────────────────┘ │
│                                                               │
│  ┌─────────────────────────────────────────────────────────┐ │
│  │               glyim-pkg (Rust / CLI)                    │ │
│  │  ┌──────────┐  ┌──────────┐  ┌──────────┐             │ │
│  │  │ CAS      │  │ Registry │  │ Resolver │             │ │
│  │  │ Client   │  │ Client   │  │          │             │ │
│  │  └──────────┘  └──────────┘  └──────────┘             │ │
│  └─────────────────────────────────────────────────────────┘ │
└───────────────────────────────────────────────────────────────┘
```

### 4.4 Key Data Flows

**Publish Flow:**
```
Author → glyim-pkg → POST /api/v1/packages/{name}/{version}/upload
                                    │
                                    ▼
                          Index.create_entry()
                                    │
                          EventLog.append(Publish)
                                    │
                          VersionEntry created in-memory
                                    │
                          Response: 200 OK
```

**Install Flow:**
```
Developer → glyim-pkg → POST /api/v1/resolve
                              │
                              ▼
                    ServerResolver.resolve()
                              │
                    ┌─────────┴──────────┐
                    │ If temporal:        │
                    │ EventLog.replay_until()
                    │ Else:               │
                    │ Use current index   │
                    └─────────┬──────────┘
                              │
                    solve() → Lockfile
                              │
                    Client: fetch blobs from CAS
                              │
                    Verify BLAKE3 + ML-DSA-65
```

### 4.5 Data Model

**Index Storage**: The in-memory index (HashMap<String, Vec<VersionEntry>>) is rebuilt on startup by replaying the event log. No separate database is used—the event log IS the source of truth.

**Event Log**: JSONL files in `{data_dir}/events/event_log.jsonl`. Each line is a JSON Event object.

**CAS Blobs**: Existing LocalContentStore in `{data_dir}/cas/`. No changes to blob storage format.

**Resolution Cache**: Resolution results stored as CAS blobs, keyed by SHA-256 of the resolution request.

### 4.6 Security Architecture

```
Publish:
  Author → sign(content_hash, Ed25519_private_key)
         → POST /api/v1/packages/{name}/{version}/upload
         → Index stores VersionEntry with publisher = public_key_hash

Attestation:
  CI → sign(content_hash, ML-DSA-65_private_key)
     → POST /api/v1/attest
     → Server verifies signature
     → Stores attestation
     → Updates trust score

Install:
  Client → GET lockfile
         → verify_signature(lockfile, ML-DSA-65_public_key)
         → For each package: verify BLAKE3 digest matches CAS blob
         → For each export: verify BLAKE3 digest matches blob
```

## 5. Architecture Decision Records

### ADR-0001: Event Log as Source of Truth

- **Status**: Accepted
- **Date**: 2025-07-11
- **Context**: The package index needs to support temporal resolution (REQ-FUNC-011) and immutable audit trail (REQ-NFR-SEC-003). We need a durable record of all index mutations.
- **Decision**: Use an append-only event log (JSONL) as the source of truth for the package index. The in-memory index is a materialized view rebuilt on startup.
- **Alternatives Considered**:
  - (A) SQLite database — simpler queries, but harder to implement temporal snapshots and append-only semantics
  - (B) In-memory only with periodic snapshots — fast but loses durability
  - (C) Event sourcing with Kafka — overkill for single-server deployment
- **Consequences**: + Temporal resolution is natural (replay to any point); + Audit trail is inherent; - Startup time depends on event log size; - Need checkpoint mechanism for large logs
- **Decision Drivers**: ASR-04, ASR-06

### ADR-0002: hlock as Lockfile Format

- **Status**: Accepted
- **Date**: 2025-07-11
- **Context**: The resolver needs to output lockfiles. hlock already provides BLAKE3 digests, Ed25519 + ML-DSA-65 signatures, per-export hashes, platform tags, graph operations, and a binary format.
- **Decision**: Use hlock as the exclusive lockfile format. Do not modify hlock source.
- **Alternatives Considered**:
  - (A) Custom TOML lockfile — simpler but no built-in crypto or graph operations
  - (B) Extend hlock — risk of forking and divergence
- **Consequences**: + All crypto is provided; + Graph operations come for free; + Binary format is compact and tamper-evident; - Dependency on external crate; - Must map VersionEntry fields to hlock Package fields
- **Decision Drivers**: CON-02, ASR-07

### ADR-0003: In-Memory Index with Event Log Rebuild

- **Status**: Accepted
- **Date**: 2025-07-11
- **Context**: The index needs sub-100ms lookup latency (ASR-01). Disk-based lookups would be too slow.
- **Decision**: Maintain the index as an in-memory HashMap, rebuilt from the event log on startup. Use checkpoint snapshots to speed up restart for large logs.
- **Alternatives Considered**:
  - (A) mmap-based key-value store (sled, LMDB) — adds dependency, more complex
  - (B) PostgreSQL — overkill for expected scale
- **Consequences**: + Simple implementation; + Fast lookups; - Must rebuild on every restart; - Need checkpoint mechanism for logs >100K events
- **Decision Drivers**: ASR-01

### ADR-0004: JSONL for Event Log Storage

- **Status**: Proposed
- **Date**: 2025-07-11
- **Context**: The event log needs a durable, append-friendly, human-readable storage format.
- **Decision**: Use JSONL (JSON Lines) — one JSON event per line, appended to a single file.
- **Alternatives Considered**:
  - (A) Binary format (bincode + varint) — more compact, faster to parse, but not human-readable
  - (B) SQLite WAL — built-in durability, but harder to implement append-only semantics
- **Consequences**: + Human-readable and debuggable; + Easy to implement; + Append-friendly; - Larger on disk; - Slower to parse than binary
- **Decision Drivers**: ASR-06, simplicity

### ADR-0005: Server-Side Resolution as Primary Mode

- **Status**: Accepted
- **Date**: 2025-07-11
- **Context**: Client-side resolution (existing glyim-pkg resolver) is limited in constraint support and doesn't benefit from global visibility. Server-side resolution can cache results and support temporal resolution.
- **Decision**: Implement server-side resolution as the primary mode. Client-side resolution remains as a fallback when the server is unavailable.
- **Alternatives Considered**:
  - (A) Client-only resolution — no caching, no global view
  - (B) Server-only resolution — fails when offline
- **Consequences**: + Better resolution quality; + Cached results; + Temporal resolution; - Server dependency for optimal behavior; - Need fallback logic
- **Decision Drivers**: ASR-02

### ADR-0006: Separate Crates for Each Domain

- **Status**: Accepted
- **Date**: 2025-07-11
- **Context**: The registry has distinct domains (index, resolution, trust, granularity, capabilities). These may evolve at different rates and have different dependencies.
- **Decision**: Create separate Rust crates for each domain: glyim-registry-index, glyim-registry-resolver, glyim-registry-trust, glyim-registry-granularity, glyim-registry-capabilities.
- **Alternatives Considered**:
  - (A) Single monolithic crate — simpler dependency management, but harder to test and evolve independently
  - (B) Feature flags within one crate — reduces crate count but creates tight coupling
- **Consequences**: + Independent versioning and testing; + Clear boundaries; - More crate boilerplate; - Must manage inter-crate dependencies
- **Decision Drivers**: REQ-NFR-MAIN-001

## 6. API and Interface Contracts

### 6.1 Design-First Approach

All new REST endpoints follow a design-first approach:
1. Define the endpoint contract (path, method, request/response schemas)
2. Implement the handler
3. Validate against contract tests

### 6.2 API Specification Structure

Each endpoint is specified in the SRS §5 with:
- Method, path, request body schema, response schema
- Error response codes and messages
- Traces to functional requirements

### 6.3 Data Contract: VersionEntry

```rust
pub struct VersionEntry {
    pub version: String,           // semver
    pub content_hash: String,      // BLAKE3 hex
    pub is_macro: bool,
    pub deps: Vec<IndexDep>,
    pub capabilities: Vec<String>,
    pub trust_level: TrustLevel,
    pub granularity_manifest_hash: Option<String>,
    pub published_at: String,      // ISO 8601
    pub publisher: String,         // public key hash
    pub provenance_hash: Option<String>,
    pub yanked: bool,
    pub deprecated: Option<String>,
}
```

### 6.4 Data Contract: hlock Lockfile Mapping

| VersionEntry Field | hlock Package Field | Notes |
|-------------------|---------------------|-------|
| name | name | Direct |
| version (major.minor.patch) | major, minor, patch | Parsed from string |
| content_hash | hashes[0].digest | HashAlgorithm::Blake3 |
| deps | dependencies | Mapped to Dependency structs |
| is_macro | — | Stored in lockfile metadata |
| capabilities | — | Stored in lockfile metadata |
| trust_level | — | Stored in lockfile metadata |

## 7. Cross-Cutting Concerns

### 7.1 Observability

- All REST endpoints emit structured logs with request_id, method, path, status_code, latency
- Index operations (publish, yank) emit event-sourced logs
- Resolution operations log cache hits/misses
- Trust scoring runs log level transitions

### 7.2 Error Handling

- All endpoints return structured JSON error responses: `{ "error": "code", "message": "description" }`
- Internal errors never expose stack traces
- CAS errors (blob not found) return 404
- Index errors (version exists, package not found) return 409/404

### 7.3 Deployment

- Single binary deployment (glyim-cas-server with all new modules)
- Configuration via environment variables and/or config file
- Data directory structure: `{data_dir}/cas/`, `{data_dir}/events/`, `{data_dir}/checkpoints/`

## 8. Alternatives Considered

### 8.1 Database-Backed Index

Considered using PostgreSQL or SQLite as the index storage. Rejected because:
- Adds operational complexity (database management)
- Temporal resolution requires event sourcing anyway
- In-memory index with event log replay is simpler and sufficient for expected scale

### 8.2 Microservice Architecture

Considered splitting the registry into separate services (index service, resolution service, trust service). Rejected because:
- Adds deployment and operational complexity
- Network latency between services would violate ASR-01
- Single-server deployment is sufficient for initial scale
- Can be refactored later if needed (ADR-0006's separate crates make this feasible)

## 9. Traceability

| ASR | ADR | Design Element | Test Artefact |
|-----|-----|----------------|---------------|
| ASR-01 | ADR-0003 | In-memory HashMap index | Load test: p95 < 100ms |
| ASR-02 | ADR-0005 | Server-side resolver with caching | Load test: 100 packages < 5s |
| ASR-03 | ADR-0002 | hlock ML-DSA-65 verification | Micro-benchmark: < 50ms |
| ASR-04 | ADR-0001 | Event log replay | Unit test: 10K events < 2s |
| ASR-05 | — | Single-process with restart recovery | Chaos test: < 60s recovery |
| ASR-06 | ADR-0004 | JSONL append-only log | API audit: no DELETE endpoints |
| ASR-07 | ADR-0002 | hlock crypto primitives | Code audit: no other algorithms |
| ASR-08 | — | Backward-compatible REST routes | Regression test suite |
```

---

## Document 5: Behavioral Specification & Test Verification Plan

```markdown
# Glyim Registry — Behavioral Specification & Test Verification Plan

| Field | Value |
|-------|-------|
| Project | Glyim Registry |
| Document | Behavioral Spec & Test Verification |
| Version | 1.0 |
| Date | 2025-07-11 |
| Author | Glyim Team |
| Status | Approved |
| Traces From | SRS v1.0 (REQ-FUNC-*, REQ-NFR-*) |

---

## 1. Behavioral Specifications

### 1.1 Package Index — Publish (REQ-FUNC-001, 002)

```gherkin
Feature: Package Publication

  Scenario: Publish a new package version successfully
    Given the package "json-utils" does not exist in the index
    When a publish request is sent for "json-utils" version "1.0.0" with content hash "abc123"
    Then the index shall contain an entry for "json-utils" version "1.0.0"
    And the entry's content_hash shall be "abc123"
    And the entry's trust_level shall be "New"
    And the entry's yanked flag shall be false
    And a Publish event shall be appended to the event log

  Scenario: Reject duplicate version publication
    Given the package "json-utils" version "1.0.0" already exists in the index
    When a publish request is sent for "json-utils" version "1.0.0"
    Then the system shall return a 409 Conflict response
    And the response body shall contain "version already exists"
    And the index shall remain unchanged
    And no event shall be appended to the event log

  Scenario: Publish a second version of an existing package
    Given the package "json-utils" version "1.0.0" exists in the index
    When a publish request is sent for "json-utils" version "1.1.0" with content hash "def456"
    Then the index shall contain entries for both "1.0.0" and "1.1.0"
    And a Publish event shall be appended to the event log
```

### 1.2 Package Index — Yank (REQ-FUNC-005, 006)

```gherkin
Feature: Package Yanking

  Scenario: Yank an existing version
    Given the package "json-utils" version "1.0.0" exists and is not yanked
    When a yank request is sent for "json-utils" version "1.0.0"
    Then the entry's yanked flag shall be true
    And a Yank event shall be appended to the event log
    And the version "1.0.0" shall not appear in version listings

  Scenario: Yank a non-existent version
    Given the package "json-utils" does not exist in the index
    When a yank request is sent for "json-utils" version "1.0.0"
    Then the system shall return a 404 Not Found response

  Scenario: Yanked version excluded from resolution
    Given the package "json-utils" version "1.0.0" is yanked
    And the package "json-utils" version "1.1.0" is not yanked
    When a resolution request includes "json-utils" with constraint "^1.0.0"
    Then the resolver shall select version "1.1.0"
    And version "1.0.0" shall not be considered
```

### 1.3 Package Index — Search (REQ-FUNC-007, 008)

```gherkin
Feature: Package Search

  Scenario: Search by exact name
    Given the package "json-utils" exists in the index
    When a search request is sent with query "json-utils"
    Then "json-utils" shall appear in the results with relevance 1.0

  Scenario: Search by name prefix
    Given the packages "json-utils" and "json-parser" exist in the index
    When a search request is sent with query "json"
    Then both "json-utils" and "json-parser" shall appear in the results
    And their relevance shall be less than 1.0

  Scenario: Search by capability
    Given the package "json-utils" declares capability "json.parse"
    When a search request is sent with query "json.parse"
    Then "json-utils" shall appear in the results
    And the matched_capabilities shall include "json.parse"

  Scenario: Search returns no results for unknown query
    Given no packages declare capability "xml.parse"
    When a search request is sent with query "xml.parse"
    Then the results shall be empty
```

### 1.4 Dependency Resolution (REQ-FUNC-010, 011, 014, 015, 018)

```gherkin
Feature: Dependency Resolution

  Scenario: Resolve simple dependency graph
    Given the package "app" depends on "json-utils" with constraint "^1.0.0"
    And "json-utils" version "1.2.0" exists in the index
    When a resolution request is sent for "app" with objective "LatestVersions"
    Then the result shall contain "json-utils" version "1.2.0"
    And the result shall be an hlock Lockfile
    And each package entry shall have a BLAKE3 integrity hash

  Scenario: Temporal resolution
    Given "json-utils" version "1.0.0" was published on "2025-01-15T10:00:00Z"
    And "json-utils" version "1.1.0" was published on "2025-03-01T10:00:00Z"
    And "json-utils" version "1.2.0" was published on "2025-06-01T10:00:00Z"
    When a resolution request is sent with resolve_as_of "2025-02-01T00:00:00Z"
    Then the resolver shall only consider "1.0.0"
    And the result shall contain "json-utils" version "1.0.0"

  Scenario: Missing dependency produces error
    Given the package "nonexistent" does not exist in the index
    When a resolution request is sent with dependency "nonexistent" version "^1.0.0"
    Then the system shall return a 422 Unprocessable Entity response
    And the error shall identify "nonexistent" as the missing package

  Scenario: Dependency cycle produces error
    Given the package "A" depends on "B"
    And the package "B" depends on "A"
    When a resolution request is sent for "A"
    Then the system shall return a 422 Unprocessable Entity response
    And the error shall identify the cycle "A → B → A"

  Scenario: Optimization objective MinimalVersions
    Given "json-utils" versions "1.0.0", "1.1.0", "1.2.0" all satisfy "^1.0.0"
    When a resolution request is sent with objective "MinimalVersions"
    Then the resolver shall select "1.0.0"

  Scenario: Optimization objective LatestVersions
    Given "json-utils" versions "1.0.0", "1.1.0", "1.2.0" all satisfy "^1.0.0"
    When a resolution request is sent with objective "LatestVersions"
    Then the resolver shall select "1.2.0"
```

### 1.5 Trust Framework (REQ-FUNC-020, 021, 022, 024, 025)

```gherkin
Feature: Progressive Trust

  Scenario: New package gets TrustLevel::New
    Given a package version is just published
    Then its trust_level shall be "New"
    And its trust score shall be 0

  Scenario: Trust score computation
    Given the following trust signals:
      | age_days | download_count | publisher_count | audit_passed | reproducible_build | author_signed | ci_attested | community_attestations | has_known_vulnerabilities |
      | 365      | 10000          | 3               | true         | true               | true          | true        | 3                      | false                     |
    When the trust scorer computes a score
    Then the trust level shall be "Certified"
    And the score shall be ≥ 90

  Scenario: Valid attestation is accepted
    Given a valid Ed25519 signature over content hash "abc123"
    When an attestation is submitted with this signature
    Then the attestation shall be stored
    And the package's trust score shall be updated

  Scenario: Invalid attestation is rejected
    Given an invalid signature over content hash "abc123"
    When an attestation is submitted with this signature
    Then the system shall return a 400 Bad Request response
    And the attestation shall not be stored
```

### 1.6 Event Log (REQ-FUNC-030, 031, 032, 033, 034, 035)

```gherkin
Feature: Event Log

  Scenario: Events are appended, never deleted
    Given the event log contains 10 events
    When a new Publish event is appended
    Then the event log shall contain 11 events
    And the original 10 events shall be unchanged

  Scenario: Replay reconstructs index state
    Given the following events exist:
      | type    | name        | version | yanked |
      | Publish | json-utils  | 1.0.0   | false  |
      | Publish | json-utils  | 1.1.0   | false  |
      | Yank    | json-utils  | 1.0.0   | true   |
    When the event log is replayed
    Then the index shall contain "json-utils" with version "1.1.0" only
    And "1.0.0" shall be marked as yanked

  Scenario: Temporal replay stops at timestamp
    Given "json-utils" version "1.0.0" was published at "2025-01-15T10:00:00Z"
    And "json-utils" version "1.1.0" was published at "2025-03-01T10:00:00Z"
    When the event log is replayed until "2025-02-01T00:00:00Z"
    Then the snapshot shall contain only "1.0.0"

  Scenario: Server startup rebuilds index
    Given the event log contains 100 events
    And the server is restarted
    When the server completes startup
    Then the in-memory index shall match the state produced by replaying all 100 events
```

### 1.7 Granularity Engine (REQ-FUNC-040, 041, 042, 044)

```gherkin
Feature: Granular Installation

  Scenario: Install specific exports
    Given the package "json-utils" version "1.0.0" has a GranularityManifest
    And the manifest maps "json.parse" to blob hash "blob_parse"
    And the manifest maps "json.stringify" to blob hash "blob_string"
    When a granular install request is sent for exports ["json.parse"]
    Then the response shall include blob hash "blob_parse"
    And the response shall not include blob hash "json.stringify"
    And the total_size_bytes shall be less than the full artifact size

  Scenario: Fallback to full artifact when no manifest
    Given the package "legacy-lib" version "1.0.0" has no GranularityManifest
    When a granular install request is sent for exports ["some_fn"]
    Then the response shall include the full artifact blob hash

  Scenario: Granularity manifest populates hlock Exports
    Given a GranularityManifest with export "json.parse" and BLAKE3 digest "abcd1234"
    When the manifest is converted to hlock exports
    Then the result shall contain an Export with identifier "json.parse"
    And hash_algo shall be Blake3
    And digest shall be "abcd1234"
```

### 1.8 Client Integration (REQ-FUNC-060, 061, 062, 063)

```gherkin
Feature: glyim-pkg Client

  Scenario: Lockfile round-trip with hlock
    Given a resolution produces a Lockfile with 5 packages
    When the lockfile is written to disk and read back
    Then the read lockfile shall be identical to the written lockfile
    And the BLAKE3 whole-file digest shall validate

  Scenario: Server-side resolution with fallback
    Given the registry server is available
    When a resolution request is made
    Then the client shall use server-side resolution
    And the result shall be an hlock Lockfile

  Scenario: Fallback to local resolution when server unavailable
    Given the registry server is unavailable
    When a resolution request is made
    Then the client shall fall back to local resolution
    And the result shall still be an hlock Lockfile

  Scenario: ML-DSA-65 signature verification on install
    Given a lockfile signed with ML-DSA-65
    When the client verifies the signature
    Then the verification shall succeed
    And the verification time shall be less than 50ms
```

## 2. Decision Tables

### 2.1 Trust Score Computation

| age_days | download_count | audit_passed | reproducible_build | author_signed | ci_attested | community_attestations | known_vulns | Expected Level |
|----------|---------------|-------------|-------------------|--------------|------------|----------------------|-------------|----------------|
| 0 | 0 | N | N | N | N | 0 | N | New (0 pts) |
| 7 | 5 | N | N | Y | N | 0 | N | New (10 pts) |
| 30 | 100 | N | N | Y | N | 0 | N | Established (30 pts) |
| 90 | 500 | Y | N | Y | Y | 1 | N | Established (50 pts) |
| 365 | 10000 | Y | Y | Y | Y | 3 | N | Certified (110 pts) |
| 365 | 10000 | Y | Y | Y | Y | 3 | Y | Verified (100 pts) |

### 2.2 Resolution Objective Selection

| Objective | Available Versions | Selected |
|-----------|-------------------|----------|
| MinimalVersions | 1.0.0, 1.1.0, 1.2.0 | 1.0.0 |
| LatestVersions | 1.0.0, 1.1.0, 1.2.0 | 1.2.0 |
| MaximalTrust | 1.0.0 (Verified), 1.2.0 (New) | 1.0.0 |

## 3. Test Strategy

### 3.1 Test Pyramid

```
         ┌─────────┐
         │  E2E    │  ← 5–10 tests: full publish→resolve→install→verify
         │  Tests  │
         ├─────────┤
         │Integr.  │  ← 20–40 tests: API endpoints, hlock integration
         │  Tests  │
         ├─────────┤
         │  Unit   │  ← 100+ tests: trust scoring, resolution, event replay
         │  Tests  │
         └─────────┘
```

### 3.2 Test Types

| Layer | Tools | Coverage Target |
|-------|-------|----------------|
| Unit | Rust `#[test]`, `proptest` (property-based) | ≥ 80% line coverage |
| Integration | `axum::test`, `reqwest`, `tokio::test` | All REST endpoints |
| Contract | hlock round-trip serialization | All lockfile paths |
| Load | k6 with thresholds | ASR-01, ASR-02 |
| Security | `cargo audit`, signature verification tests | ASR-07 |
| Mutation | `cargo-mutants` on critical modules | Trust scorer, resolver |

### 3.3 Test Priority (Risk-Based)

| Priority | Area | Rationale |
|----------|------|-----------|
| P0 | Event log durability and immutability | Data loss = total failure |
| P0 | Index consistency after restart | Wrong index = wrong installs |
| P0 | Signature verification correctness | False accept = security breach |
| P1 | Resolution correctness | Wrong resolution = broken builds |
| P1 | Temporal resolution accuracy | Historical inaccuracy = audit failure |
| P2 | Search relevance | Poor search = bad UX, not catastrophic |
| P2 | Granularity splitting | Fallback to full artifact is safe |

## 4. NFR Verification Plans

### 4.1 Performance (ASR-01, 02, 03, 04)

**k6 Load Test Script**:
```javascript
import http from 'k6/http';
import { check } from 'k6';

export const options = {
  scenarios: {
    index_lookup: {
      executor: 'constant-arrival-rate',
      rate: 100,
      timeUnit: '1s',
      duration: '5m',
      preAllocatedVUs: 50,
    },
  },
  thresholds: {
    http_req_duration: ['p(95)<100'],  // ASR-01
    http_req_failed: ['rate<0.01'],
  },
};

export default function () {
  const res = http.get(`${__ENV.REGISTRY_URL}/api/v1/packages/json-utils`);
  check(res, { 'status is 200': (r) => r.status === 200 });
}
```

**Resolution Benchmark**:
```javascript
export const options = {
  thresholds: {
    http_req_duration: ['p(95)<5000'],  // ASR-02
  },
};
```

**Crypto Micro-Benchmark**:
```rust
#[bench]
fn bench_ml_dsa_65_verify(b: &mut test::Bencher) {
    let (public_key, signature, message) = setup_ml_dsa_65();
    b.iter(|| {
        assert!(hlock::verify_signature(&message, &public_key, &signature));
    });
}
// Target: < 50ms per verification (ASR-03)
```

### 4.2 Security (ASR-06, 07)

**Append-Only Verification**:
```rust
#[test]
fn no_delete_endpoints_exist() {
    let router = build_router();
    let routes: Vec<_> = router.routes()
        .filter(|r| r.method() == http::Method::DELETE)
        .collect();
    assert!(routes.is_empty(), "No DELETE endpoints allowed (ASR-06)");
}
```

**Algorithm Allowlist**:
```rust
#[test]
fn only_approved_algorithms_used() {
    // Static analysis: grep codebase for algorithm identifiers
    let approved = ["Ed25519", "ML-DSA-65", "BLAKE3", "SHA-256", "sha256"];
    let disallowed = ["SHA-1", "sha1", "RSA", "ECDSA", "MD5"];
    // Check no disallowed algorithms appear in signing/verification code paths
    // (ASR-07)
}
```

### 4.3 Reliability (ASR-05)

**Chaos Test**:
```rust
#[tokio::test]
async fn recovery_after_process_kill() {
    let server = start_server().await;
    publish_package("test-pkg", "1.0.0").await;
    
    // Kill server
    server.kill().await;
    
    // Restart
    let server = start_server().await;
    
    // Verify index is intact
    let versions = get_package("test-pkg").await;
    assert_eq!(versions.len(), 1);
    assert_eq!(versions[0].version, "1.0.0");
}
```

## 5. Requirements Traceability Matrix

| Business Goal | Stakeholder Need | System Requirement | BDD Scenario | Test Case | Verification Method |
|---------------|-----------------|-------------------|-------------|----------|-------------------|
| BG-01 | SN-01 | REQ-FUNC-001 | Publish new package | TC-INDEX-001 | Test (automated) |
| BG-01 | SN-01 | REQ-FUNC-002 | Reject duplicate publish | TC-INDEX-002 | Test (automated) |
| BG-01 | SN-01 | REQ-FUNC-003 | List package versions | TC-INDEX-003 | Test (automated) |
| BG-01 | SN-01 | REQ-FUNC-005 | Yank version | TC-INDEX-005 | Test (automated) |
| BG-01 | SN-02 | REQ-FUNC-007 | Search by name | TC-SEARCH-001 | Test (automated) |
| BG-01 | SN-02 | REQ-FUNC-008 | Search by capability | TC-SEARCH-002 | Test (automated) |
| BG-04 | SN-08 | REQ-FUNC-010 | Resolve dependencies | TC-RESOLVE-001 | Test (automated) |
| BG-04 | SN-08 | REQ-FUNC-011 | Temporal resolution | TC-RESOLVE-002 | Test (automated) |
| BG-02 | SN-03 | REQ-FUNC-020 | Trust level assignment | TC-TRUST-001 | Test (automated) |
| BG-05 | SN-05 | REQ-FUNC-021 | Trust score computation | TC-TRUST-002 | Test (property-based) |
| BG-02 | SN-07 | REQ-FUNC-024 | Attestation acceptance | TC-TRUST-003 | Test (automated) |
| BG-02 | SN-09 | REQ-FUNC-025 | Invalid attestation rejection | TC-TRUST-004 | Test (automated) |
| BG-04 | SN-08 | REQ-FUNC-030 | Event log append-only | TC-EVENTS-001 | Test (automated) |
| BG-04 | SN-08 | REQ-FUNC-033 | Temporal replay | TC-EVENTS-002 | Test (automated) |
| BG-03 | SN-04 | REQ-FUNC-040 | Granular install | TC-GRAN-001 | Test (automated) |
| BG-03 | SN-04 | REQ-FUNC-042 | Fallback to full artifact | TC-GRAN-002 | Test (automated) |
| BG-02 | SN-03 | REQ-FUNC-060 | hlock lockfile round-trip | TC-CLIENT-001 | Test (automated) |
| BG-01 | — | REQ-NFR-PERF-001 | Index lookup latency | TC-PERF-001 | Test (load) |
| BG-04 | — | REQ-NFR-PERF-002 | Resolution latency | TC-PERF-002 | Test (load) |
| BG-02 | — | REQ-NFR-SEC-001 | Approved algorithms only | TC-SEC-001 | Analysis (code audit) |
| BG-02 | — | REQ-NFR-SEC-003 | No event deletion API | TC-SEC-002 | Analysis (API audit) |

## 6. Living Documentation Strategy

### 6.1 Spec-Code Co-Location

- All BDD scenarios stored in `tests/features/` within each crate's directory
- Scenarios are version-controlled alongside code
- CI runs `cargo test --all-features` including BDD scenarios

### 6.2 Continuous Validation

- Every PR must pass all BDD scenarios
- Performance thresholds enforced via k6 in CI
- Mutation testing runs on critical modules weekly

### 6.3 Spec Evolution

- When a requirement changes, the corresponding BDD scenario is updated first
- Test failures drive requirement clarification
- The RTM is maintained as a Markdown table in `docs/rtm.md`
```

---

## Summary: Full Specification Suite Cross-References

| Document | Key IDs | Traces To |
|----------|---------|-----------|
| **Vision** | G-1 through G-10, C-1 through C-6, NG-1 through NG-10 | Strategic goals |
| **BRS/BStRS** | BG-01 through BG-05, SN-01 through SN-12, BR-01 through BR-08 | Vision goals |
| **SRS** | REQ-FUNC-001 through 065, REQ-NFR-PERF/REL/SEC/MAIN/COMP | BRS needs + goals |
| **Architecture** | ASR-01 through ASR-08, ADR-0001 through ADR-0006 | SRS NFRs + constraints |
| **Test Plan** | TC-INDEX/SEARCH/RESOLVE/TRUST/EVENTS/GRAN/CLIENT/PERF/SEC | SRS functional + NFR requirements |

Every requirement traces backward to a stakeholder need and a business goal, and forward to a test case and verification method. Every architecture decision traces to an ASR. No orphan requirements, no untested specifications.
