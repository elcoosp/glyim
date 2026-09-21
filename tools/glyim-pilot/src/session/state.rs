use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
/// StreamStatus.
pub enum StreamStatus {
    /// Variant.
    Init,
    /// Variant.
    Seeding,
    /// Variant.
    Waiting,
    /// Variant.
    Streaming,
    /// Variant.
    Executing,
    /// Variant.
    Feedback,
    /// Variant.
    Committing,
    /// Variant.
    Committed,
    /// Variant.
    Verifying,
    /// Variant.
    Reviewing,
    /// Variant.
    Complete,
    /// Variant.
    Error,
    /// Variant.
    Paused,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
/// SessionState.
pub struct SessionState {
    /// Struct.
    pub session_id: String,
    /// Struct.
    pub pr_url: Option<String>,
    /// Struct.
    pub pr_merged: bool,
    /// Struct.
    pub stream_id: String,
    /// Struct.
    pub provider_id: String,
    /// Struct.
    pub tab_id: Option<u64>,
    /// Struct.
    pub status: StreamStatus,
    /// Struct.
    pub turn: u32,
    /// Struct.
    pub fix_round: u32,
    /// Struct.
    pub commits: u32,
    /// Struct.
    pub worktree_path: String,
    /// Struct.
    pub created_at: DateTime<Utc>,
    /// Struct.
    pub updated_at: DateTime<Utc>,
    /// Struct.
    pub last_activity: DateTime<Utc>,
    /// Struct.
    pub error_message: Option<String>,
    /// Struct.
    pub provider_cooldown_until: Option<DateTime<Utc>>,
}

impl SessionState {
    /// new.
    pub fn new(stream_id: String, provider_id: String, worktree_path: String) -> Self {
        let now = Utc::now();
        Self {
            session_id: uuid::Uuid::new_v4().to_string(),
            stream_id,
            provider_id,
            tab_id: None,
            status: StreamStatus::Init,
            turn: 0,
            fix_round: 0,
            commits: 0,
            worktree_path,
            created_at: now,
            updated_at: now,
            last_activity: now,
            error_message: None,
            provider_cooldown_until: None,
            pr_url: None,
            pr_merged: false,
        }
    }
    pub(crate) fn transition(&mut self, new_status: StreamStatus) {
        let now = Utc::now();
        self.status = new_status;
        self.updated_at = now;
        self.last_activity = now;
    }
    #[allow(dead_code)]
    pub(crate) fn record_commit(&mut self) {
        self.commits += 1;
        self.fix_round = 0;
        self.last_activity = Utc::now();
    }
    pub(crate) fn record_turn(&mut self) {
        self.turn += 1;
        self.last_activity = Utc::now();
    }
    #[allow(dead_code)]
    pub(crate) fn set_provider_cooldown(&mut self, until: DateTime<Utc>) {
        self.provider_cooldown_until = Some(until);
    }
    /// is_provider_in_cooldown.
    pub fn is_provider_in_cooldown(&self) -> bool {
        self.provider_cooldown_until
            .is_some_and(|until| Utc::now() < until)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
/// GlobalState.
pub struct GlobalState {
    /// Struct.
    pub sessions: HashMap<String, SessionState>,
    /// Struct.
    pub version: String,
}
impl GlobalState {
    /// new.
    pub fn new() -> Self {
        Self {
            sessions: HashMap::new(),
            version: env!("CARGO_PKG_VERSION").to_string(),
        }
    }
}
impl Default for GlobalState {
    fn default() -> Self {
        Self::new()
    }
}
