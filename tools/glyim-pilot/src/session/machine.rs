use super::state::{SessionState, StreamStatus};
use crate::error::PilotError;

const VALID_TRANSITIONS: &[(StreamStatus, StreamStatus)] = &[
    (StreamStatus::Init, StreamStatus::Seeding),
    (StreamStatus::Init, StreamStatus::Error),
    (StreamStatus::Seeding, StreamStatus::Waiting),
    (StreamStatus::Seeding, StreamStatus::Error),
    (StreamStatus::Waiting, StreamStatus::Streaming),
    (StreamStatus::Waiting, StreamStatus::Executing),
    (StreamStatus::Waiting, StreamStatus::Paused),
    (StreamStatus::Waiting, StreamStatus::Error),
    (StreamStatus::Streaming, StreamStatus::Executing),
    (StreamStatus::Streaming, StreamStatus::Error),
    (StreamStatus::Executing, StreamStatus::Feedback),
    (StreamStatus::Executing, StreamStatus::Error),
    (StreamStatus::Executing, StreamStatus::Committing),
    (StreamStatus::Feedback, StreamStatus::Waiting),
    (StreamStatus::Feedback, StreamStatus::Executing),
    (StreamStatus::Feedback, StreamStatus::Committing),
    (StreamStatus::Committing, StreamStatus::Committed),
    (StreamStatus::Committing, StreamStatus::Feedback),
    (StreamStatus::Committed, StreamStatus::Waiting),
    (StreamStatus::Committed, StreamStatus::Verifying),
    (StreamStatus::Verifying, StreamStatus::Reviewing),
    (StreamStatus::Verifying, StreamStatus::Feedback),
    (StreamStatus::Reviewing, StreamStatus::Complete),
    (StreamStatus::Reviewing, StreamStatus::Feedback),
    (StreamStatus::Error, StreamStatus::Seeding),
    (StreamStatus::Error, StreamStatus::Paused),
    (StreamStatus::Paused, StreamStatus::Seeding),
];

/// TransitionValidator.
pub struct TransitionValidator;

impl TransitionValidator {
    /// validate.
    pub fn validate(session: &SessionState, new_status: StreamStatus) -> Result<(), PilotError> {
        if session.status == new_status {
            return Ok(());
        }
        if VALID_TRANSITIONS
            .iter()
            .any(|(from, to)| from == &session.status && to == &new_status)
        {
            Ok(())
        } else {
            Err(PilotError::Session(format!(
                "invalid state transition: {:?} → {:?} (session {})",
                session.status, new_status, session.stream_id
            )))
        }
    }
    /// transition.
    pub fn transition(
        session: &mut SessionState,
        new_status: StreamStatus,
    ) -> Result<(), PilotError> {
        Self::validate(session, new_status.clone())?;
        session.transition(new_status);
        Ok(())
    }
}
