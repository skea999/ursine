//! Append-only JSONL audit log: one line per action, success AND failure.
//!
//! Secrets never appear here (no passwords, no tokens).

use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;

use serde::Serialize;
use ursine_api::{ApiError, ModuleId, Principal, PrincipalOrigin};

use crate::clock::Clock;
use crate::error::CoreError;

/// One audit line.
#[derive(Debug, Serialize)]
pub struct AuditEntry {
    /// Unix-ish millis from the daemon clock.
    pub ts_millis: u64,
    pub principal: String,
    pub origin: PrincipalOrigin,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub module: Option<ModuleId>,
    pub action: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub as_user: Option<String>,
    pub outcome: Outcome,
}

/// Result recorded for the action.
#[derive(Debug, Serialize)]
#[serde(tag = "result", rename_all = "snake_case")]
pub enum Outcome {
    Ok,
    Err {
        code: ursine_api::ErrorCode,
        message: String,
    },
}

impl Outcome {
    pub fn from_result(result: &Result<serde_json::Value, ApiError>) -> Self {
        match result {
            Ok(_) => Self::Ok,
            Err(e) => Self::Err {
                code: e.code,
                message: e.message.clone(),
            },
        }
    }
}

/// File-backed audit sink.
pub struct AuditLog {
    path: PathBuf,
    file: Mutex<std::fs::File>,
    clock: std::sync::Arc<dyn Clock>,
}

impl AuditLog {
    /// Open (or create) the log; parent directories are created.
    pub fn open(path: PathBuf, clock: std::sync::Arc<dyn Clock>) -> Result<Self, CoreError> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| {
                CoreError::config(format!("create audit dir {}: {e}", dir.display()))
            })?;
        }
        let file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)?;
        Ok(Self {
            path,
            file: Mutex::new(file),
            clock,
        })
    }

    /// Path of the log file (diagnostics).
    pub fn path(&self) -> &std::path::Path {
        &self.path
    }

    /// Append one entry; fsync per line so a crash cannot lose it.
    pub fn record(
        &self,
        principal: &Principal,
        module: Option<ModuleId>,
        action: impl Into<String>,
        as_user: Option<String>,
        outcome: Outcome,
    ) {
        let entry = AuditEntry {
            ts_millis: self.clock.now_millis(),
            principal: principal.user.clone(),
            origin: principal.origin,
            module,
            action: action.into(),
            as_user,
            outcome,
        };
        let Ok(mut line) = serde_json::to_string(&entry) else {
            tracing::error!("audit entry serialization failed");
            return;
        };
        line.push('\n');
        let mut file = self.file.lock().expect("audit lock");
        if let Err(e) = file
            .write_all(line.as_bytes())
            .and_then(|_| file.sync_data())
        {
            tracing::error!(error = %e, "audit write failed");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clock::ManualClock;

    fn tmp_path(name: &str) -> PathBuf {
        // nosemgrep: rust.lang.security.temp-dir.temp-dir — tests only; paths are container-local and non-security-sensitive
        std::env::temp_dir()
            .join(format!("ursine-audit-{name}-{}", std::process::id()))
            .join("audit.jsonl")
    }

    #[test]
    fn writes_parseable_jsonl_without_secrets() {
        let clock = ManualClock::new(42);
        let path = tmp_path("basic");
        let log = AuditLog::open(path.clone(), clock).unwrap();
        let p = Principal::local("skea999", vec!["wheel".into()]);

        log.record(
            &p,
            Some(ModuleId::PackagesNix),
            "install",
            None,
            Outcome::Ok,
        );
        log.record(
            &p,
            Some(ModuleId::Snapshots),
            "restore",
            Some("other".into()),
            Outcome::Err {
                code: ursine_api::ErrorCode::Unavailable,
                message: "btrbk missing".into(),
            },
        );

        let raw = std::fs::read_to_string(&path).unwrap();
        let lines: Vec<&str> = raw.lines().collect();
        assert_eq!(lines.len(), 2);
        let v: serde_json::Value = serde_json::from_str(lines[0]).unwrap();
        assert_eq!(v["principal"], "skea999");
        assert_eq!(v["module"], "packages_nix");
        assert_eq!(v["outcome"]["result"], "ok");
        let v2: serde_json::Value = serde_json::from_str(lines[1]).unwrap();
        assert_eq!(v2["outcome"]["result"], "err");
        assert_eq!(v2["as_user"], "other");
        assert!(!raw.to_lowercase().contains("password"));
        assert!(!raw.contains("token"));
    }
}
