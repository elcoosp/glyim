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
