use std::io;
use thiserror::Error;

#[derive(Debug, Error)]
/// PilotError.
pub enum PilotError {
    #[error("protocol parse error at line {line}: {message}")]
    /// Variant.
    Parse {
        /// line field.
        line: usize,
        /// message field.
        message: String,
    },

    #[error("file apply error: {0}")]
    #[allow(missing_docs)]
    Apply(#[from] ApplyError),

    #[error("path security violation: {path} escapes worktree {root}: {reason}")]
    /// Variant.
    PathEscape {
        /// Struct.
        path: String,
        /// Struct.
        root: String,
        /// Struct.
        reason: String,
    },

    #[error("git operation failed: {0}")]
    #[allow(missing_docs)]
    Git(String),

    #[error("gate '{gate}' infrastructure failure: {message}")]
    /// Variant.
    Gate {
        /// gate field.
        gate: String,
        /// message field.
        message: String,
    },

    #[error("config error: {0}")]
    #[allow(missing_docs)]
    Config(String),

    #[error("session error: {0}")]
    #[allow(missing_docs)]
    Session(String),

    #[error("apply limits exceeded: {0}")]
    #[allow(missing_docs)]
    Limits(String),

    #[error("io error: {0}")]
    #[allow(missing_docs)]
    Io(#[source] io::Error),
}

impl PilotError {
    /// code.
    pub fn code(&self) -> &'static str {
        match self {
            Self::Parse { .. } => "E0100",
            Self::Apply(e) => e.code(),
            Self::PathEscape { .. } => "E0300",
            Self::Git(_) => "E0400",
            Self::Gate { .. } => "E0500",
            Self::Config(_) => "E0600",
            Self::Session(_) => "E0700",
            Self::Limits(_) => "E0900",
            Self::Io(_) => "E0800",
        }
    }
}

#[derive(Debug, Error)]
/// ApplyError.
pub enum ApplyError {
    #[error("FIND text not found in {path}")]
    /// Variant.
    FindNotFound {
        /// path field.
        path: String,
    },
    #[error("FIND text found {count} times in {path} (expected exactly 1)")]
    /// Variant.
    FindAmbiguous {
        /// path field.
        path: String,
        /// count field.
        count: usize,
    },
    #[error("file not found: {0}")]
    #[allow(missing_docs)]
    FileNotFound(String),
    #[error("I/O error during {operation} on {path}: {source}")]
    /// Variant.
    Io {
        /// Struct.
        path: String,
        /// Struct.
        operation: String,
        #[source]
        /// Struct.
        source: io::Error,
    },
    #[error("task join failure during {operation}: {reason}")]
    /// Variant.
    TaskJoin {
        /// operation field.
        operation: String,
        /// reason field.
        reason: String,
    },
    #[error("apply failed and was rolled back: {detail}")]
    /// Struct.
    RolledBack {
        /// detail field.
        detail: String,
    },
}

impl ApplyError {
    /// code.
    pub fn code(&self) -> &'static str {
        match self {
            Self::FindNotFound { .. } => "E0201",
            Self::FindAmbiguous { .. } => "E0202",
            Self::FileNotFound(_) => "E0203",
            Self::Io { .. } => "E0204",
            Self::TaskJoin { .. } => "E0205",
            Self::RolledBack { .. } => "E0206",
        }
    }
}

impl From<std::io::Error> for PilotError {
    fn from(e: std::io::Error) -> Self {
        PilotError::Io(e)
    }
}

/// T016-PATCHED-HELPER [PILOT-1]: reject `session_id`/`stream_id` strings
/// that could be used to escape the worktree base or inject git ref spec
/// characters. `session_id` arrives over an unauthenticated local WebSocket
/// and is joined into the worktree directory path and git branch name, so a
/// value like `../../tmp/evil` (or `..`, `/`, spaces, control chars) must
/// never reach `create_worktree`.
///
/// Accepted values are non-empty ASCII alphanumerics plus `-`, `_`, `.`
/// with the additional constraint that `.` cannot be a leading component
/// (rejects `.`, `..`, `.foo/`, ...). Length is capped at 64 characters.
pub fn validate_id(s: &str) -> Result<(), PilotError> {
    if s.is_empty() || s.len() > 64 {
        return Err(PilotError::Session(format!(
            "invalid id {s:?}: length must be in 1..=64"
        )));
    }
    if s == "." || s == ".." {
        return Err(PilotError::Session(format!(
            "invalid id {s:?}: `.` and `..` are not allowed"
        )));
    }
    for c in s.chars() {
        if !(c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.') {
            return Err(PilotError::Session(format!(
                "invalid id {s:?}: character {c:?} is not allowed"
            )));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_all_error_codes_documented() {
        let codes = [
            "E0100", "E0201", "E0202", "E0203", "E0204", "E0205", "E0206", "E0300", "E0400",
            "E0500", "E0600", "E0700", "E0800", "E0900",
        ];
        let md = include_str!("../ERROR_CODES.md");
        for code in codes {
            assert!(md.contains(code), "ERROR_CODES.md missing code {code}");
        }
    }

    #[test]
    fn test_task_join_distinct_from_io() {
        let err = ApplyError::TaskJoin {
            operation: "test".into(),
            reason: "panic".into(),
        };
        assert_eq!(err.code(), "E0205");
        assert!(!format!("{err}").contains("I/O"));
    }

    // T016-PATCHED-TEST [PILOT-1]
    #[test]
    fn validate_id_accepts_normal_ids() {
        for ok in &["W1-C01", "session_42", "a.b.c", "A", "0123456789"] {
            validate_id(ok).unwrap_or_else(|e| panic!("{ok} should be ok: {e}"));
        }
    }

    #[test]
    fn validate_id_rejects_traversal_and_ref_specials() {
        let bad = [
            "",
            ".",
            "..",
            "../../tmp/evil",
            "a/b",
            "a\\b",
            "a b",
            "a:refs",
            "a~b",
            "a^b",
            "a*b",
            "a?b",
            "a[b",
            "a]b",
            "a..b/../c",
            "\u{0}nul",
            "héllo", // non-ASCII
            &"x".repeat(65),
        ];
        for s in bad {
            assert!(
                validate_id(s).is_err(),
                "expected reject: {s:?} (len={})",
                s.len()
            );
        }
    }
}
