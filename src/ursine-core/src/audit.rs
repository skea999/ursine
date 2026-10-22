//! Append-only JSONL audit log: one line per action, success AND failure.
//!
//! Secrets never appear here (no passwords, no tokens).

use std::io::Write;
#[cfg(unix)]
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
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

impl std::fmt::Debug for AuditLog {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AuditLog")
            .field("path", &self.path)
            .finish_non_exhaustive()
    }
}

impl AuditLog {
    /// Open (or create) the log; parent directories are created.
    pub fn open(path: PathBuf, clock: std::sync::Arc<dyn Clock>) -> Result<Self, CoreError> {
        if let Some(dir) = path.parent() {
            if dir == std::path::Path::new("/") {
                return Err(CoreError::config(
                    "audit path must not be at filesystem root",
                ));
            }
            if let Ok(meta) = std::fs::symlink_metadata(dir) {
                if meta.file_type().is_symlink() || !meta.is_dir() {
                    return Err(CoreError::config(format!(
                        "audit parent is not a directory: {}",
                        dir.display()
                    )));
                }
                #[cfg(unix)]
                if meta.permissions().mode() & 0o002 != 0 {
                    return Err(CoreError::config(format!(
                        "audit parent is world-writable: {}",
                        dir.display()
                    )));
                }
            }
            std::fs::create_dir_all(dir).map_err(|e| {
                CoreError::config(format!("create audit dir {}: {e}", dir.display()))
            })?;
            #[cfg(unix)]
            std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o750)).map_err(|e| {
                CoreError::config(format!("secure audit dir {}: {e}", dir.display()))
            })?;
        }
        if let Ok(meta) = std::fs::symlink_metadata(&path) {
            if meta.file_type().is_symlink() || !meta.is_file() {
                return Err(CoreError::config(format!(
                    "audit path is not a regular file: {}",
                    path.display()
                )));
            }
        }
        #[cfg(unix)]
        let file = {
            let file = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .mode(0o600)
                .open(&path)?;
            file.set_permissions(std::fs::Permissions::from_mode(0o600))?;
            file
        };
        #[cfg(not(unix))]
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
        let mut file = self
            .file
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Err(e) = file
            .write_all(line.as_bytes())
            .and_then(|()| file.sync_data())
        {
            tracing::error!(error = %e, "audit write failed");
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::disallowed_methods)] // tests use the host temp dir
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
        #[cfg(unix)]
        {
            let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o600);
        }
    }
}
