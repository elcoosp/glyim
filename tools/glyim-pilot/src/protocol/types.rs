use serde::{Deserialize, Serialize};

/// PROTOCOL_VERSION.
pub const PROTOCOL_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", content = "data")]
/// FileOp.
pub enum FileOp {
    #[serde(rename = "write")]
    /// Variant.
    Write {
        /// path field.
        path: String,
        /// content field.
        content: String,
    },
    #[serde(rename = "replace")]
    /// Variant.
    Replace {
        /// Struct.
        path: String,
        /// Struct.
        find: String,
        /// Struct.
        replace: String,
    },
    #[serde(rename = "delete")]
    /// Variant.
    Delete {
        /// path field.
        path: String,
    },
}

impl FileOp {
    /// path.
    pub fn path(&self) -> &str {
        match self {
            FileOp::Write { path, .. } => path,
            FileOp::Replace { path, .. } => path,
            FileOp::Delete { path } => path,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
/// ParsedOps.
pub struct ParsedOps {
    /// Struct.
    pub ops: Vec<FileOp>,
    /// Struct.
    pub commit_message: Option<String>,
    /// Struct.
    pub incomplete: bool,
    /// Struct.
    pub done: bool,
    /// Struct.
    pub approved: bool,
}

impl ParsedOps {
    /// empty.
    pub fn empty() -> Self {
        Self {
            ops: Vec::new(),
            commit_message: None,
            incomplete: false,
            done: false,
            approved: false,
        }
    }
    /// is_empty.
    pub fn is_empty(&self) -> bool {
        self.ops.is_empty()
            && self.commit_message.is_none()
            && !self.incomplete
            && !self.done
            && !self.approved
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_file_op_path_accessor() {
        assert_eq!(
            FileOp::Write {
                path: "a.rs".into(),
                content: String::new()
            }
            .path(),
            "a.rs"
        );
        assert_eq!(
            FileOp::Delete {
                path: "c.rs".into()
            }
            .path(),
            "c.rs"
        );
    }
}
