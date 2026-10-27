//! Long-running package job views (poll `job_output`).
//!
//! Mutations that spawn a job return [`JobStarted`]; the frontend then polls
//! `job_output` with a byte offset until `status` is `done` and only then
//! shows Close. Output is exact terminal text, never a percentage.

use serde::{Deserialize, Serialize};

/// Response of a mutation that started a background job.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JobStarted {
    pub job_id: String,
}

/// Lifecycle of a job.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobStatus {
    Running,
    Done,
}

/// One `job_output` poll result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JobOutput {
    /// Terminal text appended since `offset` (lossy UTF-8, capped per poll).
    pub lines: String,
    /// Next byte offset to pass on the following poll.
    pub offset: u64,
    pub status: JobStatus,
    /// Process exit code when `status` is `done`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
    /// True when the daemon stopped storing output at the log size cap.
    #[serde(default)]
    pub truncated: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn job_started_roundtrips() {
        let started = JobStarted {
            job_id: "0123456789abcdef".into(),
        };
        let s = serde_json::to_string(&started).unwrap();
        assert_eq!(serde_json::from_str::<JobStarted>(&s).unwrap(), started);
    }

    #[test]
    fn job_status_is_snake_case() {
        assert_eq!(
            serde_json::to_string(&JobStatus::Running).unwrap(),
            "\"running\""
        );
        assert_eq!(serde_json::to_string(&JobStatus::Done).unwrap(), "\"done\"");
    }

    #[test]
    fn job_output_optional_fields_default() {
        let v = serde_json::json!({
            "lines": "ok\n",
            "offset": 4,
            "status": "running",
        });
        let out: JobOutput = serde_json::from_value(v).unwrap();
        assert_eq!(out.status, JobStatus::Running);
        assert!(out.exit_code.is_none());
        assert!(!out.truncated);
    }
}
