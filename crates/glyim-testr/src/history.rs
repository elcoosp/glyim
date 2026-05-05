use sea_orm::entity::prelude::*;
use crate::types::{TestOutcome, TestResult};
use std::time::Duration;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "test_runs")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i64,
    pub test_name: String,
    pub outcome: String,
    pub duration_ms: i64,
    pub run_at: DateTime,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}

pub fn outcome_to_string(outcome: &TestOutcome) -> String {
    match outcome {
        TestOutcome::Passed => "passed".into(),
        TestOutcome::Failed { exit_code, stderr } => format!("failed({}) {}", exit_code, stderr),
        TestOutcome::TimedOut => "timed_out".into(),
        TestOutcome::Crash { signal } => format!("crash({})", signal),
        TestOutcome::FlakyPass { retries } => format!("flaky_pass({})", retries),
        TestOutcome::CompilationError(msg) => format!("compilation_error: {}", msg),
        TestOutcome::InternalError(msg) => format!("internal_error: {}", msg),
    }
}

pub fn string_to_outcome(s: &str) -> TestOutcome {
    if s == "passed" {
        TestOutcome::Passed
    } else if s.starts_with("failed(") {
        let rest = &s[7..];
        if let Some(idx) = rest.find(')') {
            let code: i32 = rest[..idx].parse().unwrap_or(1);
            let stderr = rest[idx+1..].trim().to_string();
            TestOutcome::Failed { exit_code: code, stderr }
        } else {
            TestOutcome::Failed { exit_code: 1, stderr: "unknown".into() }
        }
    } else {
        TestOutcome::CompilationError(s.to_string())
    }
}

impl From<&TestResult> for ActiveModel {
    fn from(r: &TestResult) -> Self {
        ActiveModel {
            test_name: Set(r.name.clone()),
            outcome: Set(outcome_to_string(&r.outcome)),
            duration_ms: Set(r.duration.as_millis() as i64),
            run_at: Set(chrono::Utc::now().naive_utc()),
            ..Default::default()
        }
    }
}
