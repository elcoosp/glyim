use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SourceLocation {
    pub file_id: u32,
    pub start_line: u32,
    pub start_col: u32,
    pub end_line: u32,
    pub end_col: u32,
    pub kind: LocationKind,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum LocationKind {
    FunctionEntry,
    Statement,
    Branch,
    Expression,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileInfo {
    pub path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoverageDump {
    pub files: HashMap<u32, FileInfo>,
    pub counters: HashMap<u64, i64>,
    pub metadata: HashMap<u64, SourceLocation>,
    pub version: u32,
}

impl CoverageDump {
    pub fn merge(&mut self, other: &CoverageDump) {
        for (file_id, info) in &other.files {
            self.files.entry(*file_id).or_insert_with(|| info.clone());
        }
        for (counter_id, count) in &other.counters {
            *self.counters.entry(*counter_id).or_insert(0) += count;
        }
        for (counter_id, loc) in &other.metadata {
            self.metadata.entry(*counter_id).or_insert_with(|| loc.clone());
        }
    }
}
