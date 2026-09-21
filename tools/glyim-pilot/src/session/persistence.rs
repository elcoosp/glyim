use super::state::{GlobalState, SessionState};
use crate::error::PilotError;
use std::path::Path;
use tokio::sync::Mutex;

const STATE_FILE: &str = ".glyim-pilot-state.json";

struct Inner {
    path: std::path::PathBuf,
    state: GlobalState,
}

impl Inner {
    async fn load(project_root: &Path) -> Result<Self, PilotError> {
        let path = project_root.join(STATE_FILE);
        let state = if path.exists() {
            let content = tokio::fs::read_to_string(&path)
                .await
                .map_err(|e| PilotError::Session(format!("failed to read state: {e}")))?;
            serde_json::from_str(&content)
                .map_err(|e| PilotError::Session(format!("failed to parse state: {e}")))?
        } else {
            GlobalState::new()
        };
        Ok(Self { path, state })
    }
    async fn save(&self) -> Result<(), PilotError> {
        let content = serde_json::to_string(&self.state)
            .map_err(|e| PilotError::Session(format!("serialization failed: {e}")))?;
        let tmp_path = std::path::PathBuf::from(format!("{}.tmp", self.path.display()));
        tokio::fs::write(&tmp_path, &content)
            .await
            .map_err(|e| PilotError::Session(format!("temp write failed: {e}")))?;
        tokio::fs::rename(&tmp_path, &self.path)
            .await
            .map_err(|e| PilotError::Session(format!("rename failed: {e}")))?;
        Ok(())
    }
}

/// StatePersistence.
pub struct StatePersistence {
    inner: Mutex<Inner>,
}

impl StatePersistence {
    /// get_session_status.
    pub async fn get_session_status(&self, session_id: &str) -> Option<String> {
        let p = self.inner.lock().await;
        p.state.sessions.get(session_id).map(|s| {
            match s.status {
                crate::session::state::StreamStatus::Init => "init",
                crate::session::state::StreamStatus::Seeding => "seeding",
                crate::session::state::StreamStatus::Waiting => "waiting",
                crate::session::state::StreamStatus::Streaming => "streaming",
                crate::session::state::StreamStatus::Executing => "executing",
                crate::session::state::StreamStatus::Feedback => "feedback",
                crate::session::state::StreamStatus::Committing => "committing",
                crate::session::state::StreamStatus::Committed => "committed",
                crate::session::state::StreamStatus::Verifying => "verifying",
                crate::session::state::StreamStatus::Reviewing => "reviewing",
                crate::session::state::StreamStatus::Complete => "complete",
                crate::session::state::StreamStatus::Error => "error",
                crate::session::state::StreamStatus::Paused => "paused",
            }
            .to_string()
        })
    }

    /// set_pr_url.
    pub async fn set_pr_url(&self, stream_id: &str, pr_url: String) -> Result<(), PilotError> {
        let mut p = self.inner.lock().await;
        if let Some(session) = p.state.sessions.get_mut(stream_id) {
            session.pr_url = Some(pr_url);
            p.save().await
        } else {
            Err(PilotError::Session(format!(
                "Session {} not found",
                stream_id
            )))
        }
    }

    /// mark_pr_merged.
    pub async fn mark_pr_merged(&self, stream_id: &str) -> Result<(), PilotError> {
        let mut p = self.inner.lock().await;
        if let Some(session) = p.state.sessions.get_mut(stream_id) {
            session.pr_merged = true;
            p.save().await
        } else {
            Err(PilotError::Session(format!(
                "Session {} not found",
                stream_id
            )))
        }
    }

    /// get_pr_merged.
    pub async fn get_pr_merged(&self, stream_id: &str) -> Result<bool, PilotError> {
        let p = self.inner.lock().await;
        if let Some(session) = p.state.sessions.get(stream_id) {
            Ok(session.pr_merged)
        } else {
            Ok(false) // not merged if not present
        }
    }
    /// load.
    pub async fn load(project_root: &Path) -> Result<Self, PilotError> {
        let inner = Inner::load(project_root).await?;
        Ok(Self {
            inner: Mutex::new(inner),
        })
    }
    /// add_session.
    pub async fn add_session(&self, session: SessionState) -> Result<(), PilotError> {
        let mut p = self.inner.lock().await;
        p.state.sessions.insert(session.stream_id.clone(), session);
        p.save().await
    }
    /// try_update_session.
    pub async fn try_update_session<F>(&self, stream_id: &str, f: F) -> Result<(), PilotError>
    where
        F: FnOnce(&mut SessionState) -> Result<(), PilotError>,
    {
        let mut p = self.inner.lock().await;
        let session = p
            .state
            .sessions
            .get_mut(stream_id)
            .ok_or_else(|| PilotError::Session(format!("session {stream_id} not found")))?;
        let backup = session.clone();
        if let Err(e) = f(session) {
            *p.state.sessions.get_mut(stream_id).unwrap() = backup;
            return Err(e);
        }
        p.save().await
    }
    /// get_worktree_path.
    pub async fn get_worktree_path(&self, stream_id: &str) -> Option<String> {
        self.inner
            .lock()
            .await
            .state
            .sessions
            .get(stream_id)
            .map(|s| s.worktree_path.clone())
    }
    /// get_stream_id.
    pub async fn get_stream_id(&self, session_id: &str) -> Option<String> {
        let p = self.inner.lock().await;
        p.state
            .sessions
            .values()
            .find(|s| s.session_id == session_id)
            .map(|s| s.stream_id.clone())
    }
    /// get_fix_round.
    pub async fn get_fix_round(&self, stream_id: &str) -> u32 {
        self.inner
            .lock()
            .await
            .state
            .sessions
            .get(stream_id)
            .map(|s| s.fix_round)
            .unwrap_or(0)
    }
    /// all_sessions.
    pub async fn all_sessions(&self) -> Vec<SessionState> {
        self.inner
            .lock()
            .await
            .state
            .sessions
            .values()
            .cloned()
            .collect()
    }
}

impl std::fmt::Debug for StatePersistence {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StatePersistence").finish_non_exhaustive()
    }
}
