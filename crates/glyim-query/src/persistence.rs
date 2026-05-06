use crate::context::QueryContext;
use crate::dependency::Dependency;
use crate::fingerprint::Fingerprint;
use crate::result::QueryStatus;
use std::any::Any;
use std::fs;
use std::io::{BufReader, BufWriter, Read, Write};
use std::path::Path;
use std::sync::Arc;

/// Persistent storage for query state.
///
/// Saves/loads the query cache and dependency graph as binary files
/// using `postcard` (serde-compatible binary format).
pub struct PersistenceLayer;

/// Serializable representation of the query cache.
#[derive(serde::Serialize, serde::Deserialize)]
struct SerializedCache {
    entries: Vec<(Fingerprint, SerializedEntry)>,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct SerializedEntry {
    fingerprint: Fingerprint,
    dependencies: Vec<Dependency>,
    was_green: bool,
}

impl PersistenceLayer {
    /// Save the query context to a directory.
    ///
    /// Creates the directory if it doesn't exist.
    /// Writes `query-cache.bin` containing serialized cache entries.
    pub fn save(ctx: &QueryContext, dir: &Path) -> Result<(), String> {
        fs::create_dir_all(dir).map_err(|e| format!("create dir: {e}"))?;
        let cache_path = dir.join("query-cache.bin");

        let mut entries = Vec::new();
        for item in ctx.cache_iter() {
            entries.push((
                item.key,
                SerializedEntry {
                    fingerprint: item.fingerprint,
                    dependencies: item.dependencies,
                    was_green: item.is_green,
                },
            ));
        }

        let serialized = SerializedCache { entries };
        let file =
            fs::File::create(&cache_path).map_err(|e| format!("create file: {e}"))?;
        let mut writer = BufWriter::new(file);

        // Serialize to a Vec<u8> first, then write (postcard doesn't have
        // a direct to_writer serializer for serde types).
        let bytes = postcard::to_allocvec(&serialized)
            .map_err(|e| format!("serialize: {e}"))?;
        writer
            .write_all(&bytes)
            .map_err(|e| format!("write: {e}"))?;

        Ok(())
    }

    /// Load a query context from a directory.
    ///
    /// Returns an empty context if the directory doesn't exist or is empty.
    /// Loaded entries have placeholder values — the actual value will be
    /// recomputed on first access, but the fingerprint and dependencies
    /// are preserved so validity checks work.
    pub fn load(dir: &Path) -> Result<QueryContext, String> {
        let cache_path = dir.join("query-cache.bin");
        if !cache_path.exists() {
            return Ok(QueryContext::new());
        }

        let file =
            fs::File::open(&cache_path).map_err(|e| format!("open file: {e}"))?;
        let mut reader = BufReader::new(file);
        let mut buf = Vec::new();
        reader
            .read_to_end(&mut buf)
            .map_err(|e| format!("read: {e}"))?;

        let serialized: SerializedCache =
            postcard::from_bytes(&buf).map_err(|e| format!("deserialize: {e}"))?;

        let ctx = QueryContext::new();
        for (key, entry) in serialized.entries {
            let status = if entry.was_green {
                QueryStatus::Green
            } else {
                QueryStatus::Red
            };
            ctx.insert_with_status(
                key,
                Arc::new(()),
                entry.fingerprint,
                entry.dependencies,
                status,
            );
        }

        Ok(ctx)
    }
}
