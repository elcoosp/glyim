use crate::protocol::types::PROTOCOL_VERSION;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
/// ExtensionMessage.
pub enum ExtensionMessage {
    #[serde(rename = "session.ready", rename_all = "camelCase")]
    /// Variant.
    SessionReady {
        /// Struct.
        session_id: String,
        /// Struct.
        provider_id: String,
        /// Struct.
        tab_id: u64,
        /// Struct.
        trace_id: Option<String>,
        /// Struct.
        v: u32,
    },
    #[serde(rename = "ops.ready", rename_all = "camelCase")]
    /// Variant.
    OpsReady {
        /// Struct.
        session_id: String,
        /// Struct.
        content: String,
        /// Struct.
        turn: u32,
        /// Struct.
        trace_id: Option<String>,
        /// Struct.
        v: u32,
    },
    #[serde(rename = "stream.complete", rename_all = "camelCase")]
    /// Variant.
    StreamComplete {
        /// Struct.
        session_id: String,
        /// Struct.
        turn: u32,
        /// Struct.
        full_response: String,
        /// Struct.
        trace_id: Option<String>,
        /// Struct.
        v: u32,
    },
    #[serde(rename = "error.detected", rename_all = "camelCase")]
    /// Variant.
    ErrorDetected {
        /// Struct.
        session_id: String,
        /// Struct.
        error_type: String,
        /// Struct.
        error_message: String,
        /// Struct.
        recoverable: bool,
        /// Struct.
        trace_id: Option<String>,
        /// Struct.
        v: u32,
    },
    #[serde(rename = "pong")]
    /// Variant.
    Pong {
        /// timestamp field.
        timestamp: u64,
        /// v field.
        v: u32,
    },
}

impl ExtensionMessage {
    /// version.
    pub fn version(&self) -> u32 {
        match self {
            Self::SessionReady { v, .. }
            | Self::OpsReady { v, .. }
            | Self::StreamComplete { v, .. }
            | Self::ErrorDetected { v, .. }
            | Self::Pong { v, .. } => *v,
        }
    }
    /// session_id.
    pub fn session_id(&self) -> Option<&str> {
        match self {
            Self::SessionReady { session_id, .. }
            | Self::OpsReady { session_id, .. }
            | Self::StreamComplete { session_id, .. }
            | Self::ErrorDetected { session_id, .. } => Some(session_id),
            Self::Pong { .. } => None,
        }
    }
    /// trace_id.
    pub fn trace_id(&self) -> Option<&str> {
        match self {
            Self::SessionReady { trace_id, .. }
            | Self::OpsReady { trace_id, .. }
            | Self::StreamComplete { trace_id, .. }
            | Self::ErrorDetected { trace_id, .. } => trace_id.as_deref(),
            Self::Pong { .. } => None,
        }
    }
    /// validate_version.
    pub fn validate_version(&self) -> Result<(), String> {
        let v = self.version();
        if v == 0 {
            return Err(format!(
                "message with v=0 rejected (current: {})",
                PROTOCOL_VERSION
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
/// CliMessage.
pub enum CliMessage {
    #[serde(rename = "session.start", rename_all = "camelCase")]
    /// Variant.
    SessionStart {
        /// Struct.
        session_id: String,
        /// Struct.
        provider_id: String,
        /// Struct.
        prompt: String,
        /// Struct.
        system_prompt: String,
        /// Struct.
        trace_id: Option<String>,
        /// Struct.
        v: u32,
    },
    #[serde(rename = "feedback.send", rename_all = "camelCase")]
    /// Variant.
    FeedbackSend {
        /// Struct.
        session_id: String,
        /// Struct.
        message: String,
        /// Struct.
        turn: u32,
        /// Struct.
        trace_id: Option<String>,
        /// Struct.
        v: u32,
    },
    #[serde(rename = "feedback.continue", rename_all = "camelCase")]
    /// Variant.
    FeedbackContinue {
        /// Struct.
        session_id: String,
        /// Struct.
        trace_id: Option<String>,
        /// Struct.
        v: u32,
    },
    #[serde(rename = "retry.prompt", rename_all = "camelCase")]
    /// Variant.
    RetryPrompt {
        /// Struct.
        session_id: String,
        /// Struct.
        message: String,
        /// Struct.
        delay: u64,
        /// Struct.
        trace_id: Option<String>,
        /// Struct.
        v: u32,
    },
    #[serde(rename = "session.pause", rename_all = "camelCase")]
    /// Variant.
    SessionPause {
        /// Struct.
        session_id: String,
        /// Struct.
        trace_id: Option<String>,
        /// Struct.
        v: u32,
    },
    #[serde(rename = "session.abort", rename_all = "camelCase")]
    /// Variant.
    SessionAbort {
        /// Struct.
        session_id: String,
        /// Struct.
        trace_id: Option<String>,
        /// Struct.
        v: u32,
    },
    #[serde(rename = "ping")]
    /// Struct.
    Ping {
        /// timestamp field.
        timestamp: u64,
        /// v field.
        v: u32,
    },
}
