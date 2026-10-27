//! RAM job registry with on-disk terminal logs (package progress).
//!
//! A package mutation that would block on a package manager instead starts a
//! job: the handler returns [`JobStarted`], a background task runs the command
//! and streams its exact terminal bytes into `<dir>/<id>.log`, and the
//! frontend polls `job_output` until `status` is `done`. Output is terminal
//! text, never a percentage. Start and completion are audited; polls are not.

use std::collections::HashMap;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use ursine_api::{ApiError, ErrorCode, JobOutput, JobStarted, JobStatus, ModuleId, Principal};

use crate::audit::{AuditLog, Outcome};
use crate::clock::Clock;
use crate::error::CoreError;

/// Per-job log cap; further bytes are dropped and `truncated` is set.
const LOG_CAP_BYTES: u64 = 1024 * 1024;
/// One `job_output` poll returns at most this many bytes.
const POLL_CAP_BYTES: usize = 64 * 1024;
/// Finished jobs (and their logs) are purged after this long.
const FINISHED_TTL_MILLIS: u64 = 10 * 60 * 1000;
/// A running job with no exit after this long is declared lost (dead worker).
const RUNNING_MAX_MILLIS: u64 = 2 * 60 * 60 * 1000;
/// Concurrent running jobs; beyond this the daemon answers `conflict`.
const MAX_RUNNING: usize = 4;
/// Total live entries (running + recently finished) before `conflict`.
const MAX_JOBS: usize = 64;
/// Job ids: 16 hex chars (no path separators).
const JOB_ID_LEN: usize = 16;

/// Append-only writer for one job log; enforces the size cap with atomic
/// counters so `job_output` can report `truncated` without re-reading.
pub struct JobSink {
    file: Mutex<std::fs::File>,
    written: AtomicU64,
    truncated: AtomicBool,
    cap: u64,
}

impl std::fmt::Debug for JobSink {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("JobSink")
            .field("written", &self.written.load(Ordering::SeqCst))
            .field("truncated", &self.truncated.load(Ordering::SeqCst))
            .finish_non_exhaustive()
    }
}

impl JobSink {
    /// Create a fresh `0600` log file (fails if it already exists).
    fn create(path: &Path, cap: u64) -> Result<Self, ApiError> {
        #[cfg(unix)]
        let file = {
            use std::os::unix::fs::OpenOptionsExt;
            std::fs::OpenOptions::new()
                .create_new(true)
                .write(true)
                .mode(0o600)
                .open(path)
        };
        #[cfg(not(unix))]
        let file = std::fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(path);
        let file = file.map_err(|e| ApiError::internal(format!("create job log: {e}")))?;
        Ok(Self {
            file: Mutex::new(file),
            written: AtomicU64::new(0),
            truncated: AtomicBool::new(false),
            cap,
        })
    }

    /// Append bytes, keeping the log within `cap`. The room check runs under
    /// the file lock so concurrent stdout/stderr writers cannot overshoot.
    pub fn append(&self, bytes: &[u8]) {
        if bytes.is_empty() {
            return;
        }
        let mut file = self.file.lock().unwrap_or_else(PoisonError::into_inner);
        let written = self.written.load(Ordering::SeqCst);
        if written >= self.cap {
            self.truncated.store(true, Ordering::SeqCst);
            return;
        }
        let room = usize::try_from(self.cap - written).unwrap_or(usize::MAX);
        let chunk = bytes.get(..room.min(bytes.len())).unwrap_or_default();
        if let Err(error) = file.write_all(chunk) {
            tracing::error!(error = %error, "job log write failed");
        } else {
            let len = u64::try_from(chunk.len()).unwrap_or(u64::MAX);
            self.written.fetch_add(len, Ordering::SeqCst);
        }
        if chunk.len() < bytes.len() {
            self.truncated.store(true, Ordering::SeqCst);
        }
    }

    /// Append one line of text (adds the newline).
    pub fn append_line(&self, line: &str) {
        let mut buf = String::with_capacity(line.len() + 1);
        buf.push_str(line);
        buf.push('\n');
        self.append(buf.as_bytes());
    }

    fn truncated(&self) -> bool {
        self.truncated.load(Ordering::SeqCst)
    }
}

/// One registry entry. The log bytes live in the file; this is metadata.
struct JobEntry {
    status: JobStatus,
    exit_code: Option<i32>,
    module: ModuleId,
    action: String,
    principal: Principal,
    as_user: Option<String>,
    created_millis: u64,
    finished_millis: Option<u64>,
    path: PathBuf,
    sink: Arc<JobSink>,
}

struct Inner {
    dir: PathBuf,
    clock: Arc<dyn Clock>,
    audit: Arc<AuditLog>,
    jobs: Mutex<HashMap<String, JobEntry>>,
}

impl Inner {
    /// Purge expired finished jobs and declare lost running jobs finished.
    /// Caller holds the `jobs` lock. Side effects (file unlinks, audit fsync)
    /// are returned so the caller can run them after releasing the lock.
    fn sweep_locked(&self, jobs: &mut HashMap<String, JobEntry>) -> Swept {
        let mut swept = Swept::default();
        let now = self.clock.now_millis();
        let expired: Vec<String> = jobs
            .iter()
            .filter(|(_, entry)| {
                entry
                    .finished_millis
                    .is_some_and(|finished| now.saturating_sub(finished) > FINISHED_TTL_MILLIS)
            })
            .map(|(id, _)| id.clone())
            .collect();
        for id in expired {
            if let Some(entry) = jobs.remove(&id) {
                swept.paths.push(entry.path.clone());
            }
        }
        let lost: Vec<String> = jobs
            .iter()
            .filter(|(_, entry)| {
                entry.status == JobStatus::Running
                    && now.saturating_sub(entry.created_millis) > RUNNING_MAX_MILLIS
            })
            .map(|(id, _)| id.clone())
            .collect();
        for id in lost {
            if let Some(entry) = jobs.get_mut(&id) {
                // Worker died without reporting; close the job so its slot
                // and the client's poll loop both terminate.
                let error = ApiError::internal("job worker stopped without an exit status");
                entry.sink.append_line(&format!("error: {}", error.message));
                if let Some(record) = complete(entry, &id, &Err(error), &*self.clock) {
                    swept.audits.push(record);
                }
            }
        }
        swept
    }

    /// Run sweep side effects: unlink purged logs, write completion audits.
    fn apply_swept(&self, swept: Swept) {
        for path in swept.paths {
            let _ = std::fs::remove_file(path);
        }
        for record in swept.audits {
            record.record(&self.audit);
        }
    }
}

/// Deferred sweep side effects (file unlinks + audit records).
#[derive(Default)]
struct Swept {
    paths: Vec<PathBuf>,
    audits: Vec<CompletionAudit>,
}

/// Completion audit computed under the jobs lock, written after it drops
/// (the audit log fsyncs per line; holding the registry lock through it
/// would stall every poll and start behind the disk).
struct CompletionAudit {
    principal: Principal,
    module: ModuleId,
    action: String,
    as_user: Option<String>,
    outcome: Outcome,
}

impl CompletionAudit {
    fn record(self, audit: &AuditLog) {
        audit.record(
            &self.principal,
            Some(self.module),
            &self.action,
            self.as_user,
            self.outcome,
        );
    }
}

/// Mark a job finished under the registry lock; returns the audit record to
/// write once the lock is released. Returns `None` when the job already
/// finished (late worker after a lost-sweep) so one job is never audited twice.
fn complete(
    entry: &mut JobEntry,
    id: &str,
    result: &Result<i32, ApiError>,
    clock: &dyn Clock,
) -> Option<CompletionAudit> {
    if entry.finished_millis.is_some() {
        return None;
    }
    let (exit_code, outcome) = match result {
        Ok(code) => {
            let outcome = if *code == 0 {
                Outcome::Ok
            } else {
                Outcome::Err {
                    code: ErrorCode::Internal,
                    message: format!("job {id} exited {code}"),
                }
            };
            (Some(*code), outcome)
        }
        Err(error) => (
            None,
            Outcome::Err {
                code: error.code,
                message: format!("job {id}: {}", error.message),
            },
        ),
    };
    entry.status = JobStatus::Done;
    entry.exit_code = exit_code;
    entry.finished_millis = Some(clock.now_millis());
    Some(CompletionAudit {
        principal: entry.principal.clone(),
        module: entry.module,
        action: entry.action.clone(),
        as_user: entry.as_user.clone(),
        outcome,
    })
}

/// Job registry: cheap to clone, shared across handlers and worker tasks.
#[derive(Clone)]
pub struct JobRegistry {
    inner: Arc<Inner>,
}

impl std::fmt::Debug for JobRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("JobRegistry")
            .field("dir", &self.inner.dir)
            .finish_non_exhaustive()
    }
}

impl JobRegistry {
    /// Create the registry and its `0700` log directory.
    pub fn new(
        dir: PathBuf,
        audit: Arc<AuditLog>,
        clock: Arc<dyn Clock>,
    ) -> Result<Self, CoreError> {
        if dir == Path::new("/") || dir.file_name().is_none() {
            return Err(CoreError::config("job dir must not be at filesystem root"));
        }
        if let Some(parent) = dir.parent() {
            if let Ok(meta) = std::fs::symlink_metadata(parent) {
                if meta.file_type().is_symlink() || !meta.is_dir() {
                    return Err(CoreError::config(format!(
                        "job parent is not a directory: {}",
                        parent.display()
                    )));
                }
            }
        }
        std::fs::create_dir_all(&dir)
            .map_err(|e| CoreError::config(format!("create job dir {}: {e}", dir.display())))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700))
                .map_err(|e| CoreError::config(format!("secure job dir: {e}")))?;
        }
        // The registry is RAM-only: logs left by a previous daemon in this
        // boot are orphaned and would pile up in tmpfs until reboot.
        if let Ok(entries) = std::fs::read_dir(&dir) {
            for entry in entries.flatten() {
                let name = entry.file_name();
                let Some(name) = name.to_str() else {
                    continue;
                };
                let stem = name.strip_suffix(".log").unwrap_or(name);
                if stem.len() == JOB_ID_LEN && stem.bytes().all(|b| b.is_ascii_hexdigit()) {
                    let _ = std::fs::remove_file(entry.path());
                }
            }
        }
        Ok(Self {
            inner: Arc::new(Inner {
                dir,
                clock,
                audit,
                jobs: Mutex::new(HashMap::new()),
            }),
        })
    }

    /// Start a background job. `run` receives the log sink and must return
    /// the process exit code (`-1` when killed by a signal); an `Err` is
    /// appended to the log as `error: …` and closes the job without a code.
    pub fn start<F, Fut>(
        &self,
        module: ModuleId,
        action: impl Into<String>,
        principal: &Principal,
        as_user: Option<&str>,
        run: F,
    ) -> Result<JobStarted, ApiError>
    where
        F: FnOnce(Arc<JobSink>) -> Fut + Send + 'static,
        Fut: std::future::Future<Output = Result<i32, ApiError>> + Send + 'static,
    {
        let action = action.into();
        let inner = Arc::clone(&self.inner);
        let mut jobs = inner.jobs.lock().unwrap_or_else(PoisonError::into_inner);
        let swept = inner.sweep_locked(&mut jobs);

        let running = jobs
            .values()
            .filter(|entry| entry.status == JobStatus::Running)
            .count();
        if running >= MAX_RUNNING {
            drop(jobs);
            inner.apply_swept(swept);
            return Err(ApiError::new(
                ErrorCode::Conflict,
                "another package operation is already running",
            ));
        }
        if jobs.len() >= MAX_JOBS {
            drop(jobs);
            inner.apply_swept(swept);
            return Err(ApiError::new(
                ErrorCode::Conflict,
                "too many recent jobs; retry in a few minutes",
            ));
        }

        let (id, sink) = match create_log(&inner.dir) {
            Ok(created) => created,
            Err(error) => {
                drop(jobs);
                inner.apply_swept(swept);
                return Err(error);
            }
        };
        jobs.insert(
            id.clone(),
            JobEntry {
                status: JobStatus::Running,
                exit_code: None,
                module,
                action,
                principal: principal.clone(),
                as_user: as_user.map(str::to_string),
                created_millis: inner.clock.now_millis(),
                finished_millis: None,
                path: inner.dir.join(format!("{id}.log")),
                sink: Arc::clone(&sink),
            },
        );
        drop(jobs);
        inner.apply_swept(swept);

        let task_id = id.clone();
        let task_sink = Arc::clone(&sink);
        let monitor_inner = Arc::clone(&inner);
        let handle = tokio::spawn(async move {
            let result = run(task_sink).await;
            if let Err(error) = &result {
                // Surface the failure in the terminal log the user is watching.
                sink.append_line(&format!("error: {}", error.message));
            }
            let record = {
                let mut jobs = inner.jobs.lock().unwrap_or_else(PoisonError::into_inner);
                jobs.get_mut(&task_id)
                    .and_then(|entry| complete(entry, &task_id, &result, &*inner.clock))
            };
            if let Some(record) = record {
                record.record(&inner.audit);
            }
        });
        // A panicking worker would otherwise leave the entry `Running` until
        // the 2h lost sweep: close it immediately with the panic message.
        let monitor_id = id.clone();
        tokio::spawn(async move {
            let Err(error) = handle.await else {
                return;
            };
            let message = format!("job worker failed: {error}");
            let record = {
                let mut jobs = monitor_inner
                    .jobs
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner);
                match jobs.get_mut(&monitor_id) {
                    Some(entry) if entry.finished_millis.is_none() => {
                        entry.sink.append_line(&format!("error: {message}"));
                        complete(
                            entry,
                            &monitor_id,
                            &Err(ApiError::internal(message)),
                            &*monitor_inner.clock,
                        )
                    }
                    _ => None,
                }
            };
            if let Some(record) = record {
                record.record(&monitor_inner.audit);
            }
        });
        Ok(JobStarted { job_id: id })
    }

    /// Read a job log from `offset` (bounded, UTF-8 safe) plus its lifecycle.
    /// The caller's principal must own the job: terminal output can contain
    /// sensitive package-manager detail and must not cross users.
    pub fn output(
        &self,
        caller: &Principal,
        job_id: &str,
        offset: u64,
    ) -> Result<JobOutput, ApiError> {
        if job_id.len() != JOB_ID_LEN || !job_id.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(ApiError::invalid("invalid job id"));
        }
        let inner = Arc::clone(&self.inner);
        let mut jobs = inner.jobs.lock().unwrap_or_else(PoisonError::into_inner);
        let swept = inner.sweep_locked(&mut jobs);
        let snapshot = jobs.get(job_id).map(|entry| {
            (
                entry.status,
                entry.exit_code,
                entry.path.clone(),
                entry.sink.truncated(),
                entry.principal.clone(),
            )
        });
        drop(jobs);
        inner.apply_swept(swept);
        let Some((status, exit_code, path, truncated, owner)) = snapshot else {
            return Err(ApiError::new(ErrorCode::NotFound, "unknown job"));
        };
        if owner.origin != caller.origin || owner.user != caller.user {
            return Err(ApiError::new(
                ErrorCode::Forbidden,
                "job output belongs to another session",
            ));
        }

        let mut file = std::fs::File::open(&path)
            .map_err(|e| ApiError::new(ErrorCode::NotFound, format!("job log unavailable: {e}")))?;
        file.seek(SeekFrom::Start(offset))
            .map_err(|e| ApiError::internal(format!("seek job log: {e}")))?;
        let mut buf = vec![0u8; POLL_CAP_BYTES];
        let read = file
            .read(&mut buf)
            .map_err(|e| ApiError::internal(format!("read job log: {e}")))?;
        buf.truncate(read);
        // Never deliver a torn UTF-8 sequence: keep only the valid prefix,
        // skip interior invalid bytes, leave a trailing partial sequence for
        // the next poll so the client can append losslessly.
        let consumed = match std::str::from_utf8(&buf) {
            Ok(_) => u64::try_from(buf.len()).unwrap_or(u64::MAX),
            Err(error) => {
                let valid = error.valid_up_to();
                let dropped = valid + error.error_len().unwrap_or(0);
                buf.truncate(valid);
                u64::try_from(dropped).unwrap_or(u64::MAX)
            }
        };
        let lines = String::from_utf8(buf)
            .map_err(|e| ApiError::internal(format!("job log decode: {e}")))?;
        Ok(JobOutput {
            lines,
            offset: offset.saturating_add(consumed),
            status,
            exit_code,
            truncated,
        })
    }
}

/// Create a fresh log file, retrying on the (astronomically rare) id clash.
fn create_log(dir: &Path) -> Result<(String, Arc<JobSink>), ApiError> {
    #[allow(clippy::expect_used)] // OS RNG failure cannot be handled here
    fn new_job_id() -> String {
        use std::fmt::Write as _;
        let mut raw = [0u8; 8];
        getrandom::getrandom(&mut raw).expect("os rng must work");
        let mut id = String::with_capacity(JOB_ID_LEN);
        for byte in raw {
            let _ = write!(id, "{byte:02x}");
        }
        id
    }

    let mut last: Option<ApiError> = None;
    for _ in 0..4 {
        let id = new_job_id();
        let path = dir.join(format!("{id}.log"));
        match JobSink::create(&path, LOG_CAP_BYTES) {
            Ok(sink) => return Ok((id, Arc::new(sink))),
            Err(error) => last = Some(error),
        }
    }
    Err(last.unwrap_or_else(|| ApiError::internal("cannot create job log")))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::disallowed_methods)] // tests use the host temp dir
    use super::*;

    fn registry(name: &str) -> (JobRegistry, Arc<crate::clock::ManualClock>) {
        let dir = std::env::temp_dir().join(format!("ursine-jobs-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let clock = crate::clock::ManualClock::new(0);
        let audit = AuditLog::open(dir.join("audit.jsonl"), clock.clone()).unwrap();
        let jobs = JobRegistry::new(dir.join("jobs"), Arc::new(audit), clock.clone()).unwrap();
        (jobs, clock)
    }

    fn principal() -> Principal {
        Principal::local("skea999", vec!["wheel".into()])
    }

    async fn wait_done(jobs: &JobRegistry, id: &str) -> JobOutput {
        for _ in 0..200 {
            let out = jobs.output(&principal(), id, 0).unwrap();
            if out.status == JobStatus::Done {
                return out;
            }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
        panic!("job did not finish");
    }

    #[tokio::test]
    async fn job_runs_to_completion_and_streams_log() {
        let (jobs, _clock) = registry("run");
        let started = jobs
            .start(
                ModuleId::PackagesApk,
                "install_apk",
                &principal(),
                None,
                |sink| async move {
                    sink.append(b"fetching...\n");
                    sink.append(b"done\n");
                    Ok(0)
                },
            )
            .unwrap();
        assert_eq!(started.job_id.len(), JOB_ID_LEN);
        let out = wait_done(&jobs, &started.job_id).await;
        assert_eq!(out.status, JobStatus::Done);
        assert_eq!(out.exit_code, Some(0));
        assert_eq!(out.lines, "fetching...\ndone\n");
        assert_eq!(out.offset, 17);
        assert!(!out.truncated);
    }

    #[tokio::test]
    async fn output_resumes_from_offset() {
        let (jobs, _clock) = registry("offset");
        let started = jobs
            .start(
                ModuleId::PackagesApk,
                "install_apk",
                &principal(),
                None,
                |sink| async move {
                    sink.append(b"aaaa\nbbbb\n");
                    Ok(0)
                },
            )
            .unwrap();
        let first = wait_done(&jobs, &started.job_id).await;
        let second = jobs
            .output(&principal(), &started.job_id, first.offset)
            .unwrap();
        assert!(second.lines.is_empty());
        assert_eq!(second.offset, first.offset);
    }

    #[tokio::test]
    async fn invalid_and_unknown_ids_are_rejected() {
        let (jobs, _clock) = registry("ids");
        let err = jobs.output(&principal(), "../etc/passwd", 0).unwrap_err();
        assert_eq!(err.code, ErrorCode::Invalid);
        let err = jobs
            .output(&principal(), "0000000000000000", 0)
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::NotFound);
    }

    #[tokio::test]
    async fn concurrent_jobs_are_capped() {
        let (jobs, _clock) = registry("cap");
        let mut ids = Vec::new();
        for _ in 0..MAX_RUNNING {
            let started = jobs
                .start(
                    ModuleId::PackagesApk,
                    "install_apk",
                    &principal(),
                    None,
                    |sink| async move {
                        sink.append(b"running\n");
                        std::future::pending::<()>().await;
                        Ok(0)
                    },
                )
                .unwrap();
            ids.push(started.job_id);
        }
        let err = jobs
            .start(
                ModuleId::PackagesApk,
                "install_apk",
                &principal(),
                None,
                |_| async { Ok(0) },
            )
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::Conflict);
        for id in ids {
            let out = jobs.output(&principal(), &id, 0).unwrap();
            assert_eq!(out.status, JobStatus::Running);
        }
    }

    #[tokio::test]
    async fn log_is_capped_and_flags_truncation() {
        let (jobs, _clock) = registry("cap-log");
        let started = jobs
            .start(
                ModuleId::PackagesApk,
                "install_apk",
                &principal(),
                None,
                |sink| async move {
                    let block = vec![b'x'; 64 * 1024];
                    for _ in 0..(LOG_CAP_BYTES / (64 * 1024) + 4) {
                        sink.append(&block);
                    }
                    Ok(0)
                },
            )
            .unwrap();
        let out = wait_done(&jobs, &started.job_id).await;
        assert!(out.truncated);
        // Drain the log: the stored byte count must stop exactly at the cap.
        let mut offset = 0;
        for _ in 0..64 {
            let page = jobs.output(&principal(), &started.job_id, offset).unwrap();
            if page.lines.is_empty() {
                break;
            }
            offset = page.offset;
        }
        assert_eq!(offset, LOG_CAP_BYTES);
    }

    #[tokio::test]
    async fn failed_job_logs_error_and_has_no_exit_code() {
        let (jobs, _clock) = registry("fail");
        let started = jobs
            .start(
                ModuleId::PackagesNix,
                "install",
                &principal(),
                Some("alice"),
                |_| async { Err(ApiError::internal("spawn command: no such file")) },
            )
            .unwrap();
        let out = wait_done(&jobs, &started.job_id).await;
        assert_eq!(out.status, JobStatus::Done);
        assert!(out.exit_code.is_none());
        assert!(out.lines.contains("error: spawn command: no such file"));
    }

    #[tokio::test]
    async fn finished_jobs_purge_after_ttl() {
        let (jobs, clock) = registry("ttl");
        let started = jobs
            .start(
                ModuleId::PackagesApk,
                "install_apk",
                &principal(),
                None,
                |_| async { Ok(0) },
            )
            .unwrap();
        wait_done(&jobs, &started.job_id).await;
        clock.advance(FINISHED_TTL_MILLIS + 1);
        let err = jobs.output(&principal(), &started.job_id, 0).unwrap_err();
        assert_eq!(err.code, ErrorCode::NotFound);
    }

    #[tokio::test]
    async fn output_rejects_a_different_principal() {
        let (jobs, _clock) = registry("principal");
        let started = jobs
            .start(
                ModuleId::PackagesApk,
                "install_apk",
                &principal(),
                None,
                |sink| async move {
                    sink.append(b"secret plan\n");
                    Ok(0)
                },
            )
            .unwrap();
        wait_done(&jobs, &started.job_id).await;
        let other = Principal::local("mallory", vec!["wheel".into()]);
        let err = jobs.output(&other, &started.job_id, 0).unwrap_err();
        assert_eq!(err.code, ErrorCode::Forbidden);
        // The owner still reads it.
        let out = jobs.output(&principal(), &started.job_id, 0).unwrap();
        assert_eq!(out.lines, "secret plan\n");
    }

    #[tokio::test]
    async fn stale_logs_from_a_previous_run_are_purged_at_startup() {
        let dir = std::env::temp_dir().join(format!("ursine-jobs-stale-{}", std::process::id()));
        let jobs_dir = dir.join("jobs");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&jobs_dir).unwrap();
        let stale = jobs_dir.join("0123456789abcdef.log");
        std::fs::write(&stale, b"leftover").unwrap();
        let keep = jobs_dir.join("notes.txt");
        std::fs::write(&keep, b"keep me").unwrap();

        let clock = crate::clock::ManualClock::new(0);
        let audit = AuditLog::open(dir.join("audit.jsonl"), clock.clone()).unwrap();
        let jobs = JobRegistry::new(jobs_dir.clone(), Arc::new(audit), clock.clone()).unwrap();
        assert!(!stale.exists(), "stale log survived startup purge");
        assert!(keep.exists(), "unrelated file was removed");

        let _ = std::fs::remove_dir_all(&dir);
        drop(jobs);
    }

    #[tokio::test]
    async fn panicking_worker_is_closed_by_the_monitor() {
        let (jobs, _clock) = registry("panic");
        let started = jobs
            .start(
                ModuleId::PackagesNix,
                "install",
                &principal(),
                None,
                |_| async {
                    let out: Result<i32, ApiError> = panic!("simulated worker bug");
                    out
                },
            )
            .unwrap();
        let out = wait_done(&jobs, &started.job_id).await;
        assert_eq!(out.status, JobStatus::Done);
        assert!(out.exit_code.is_none());
        assert!(out.lines.contains("job worker failed"), "{}", out.lines);
    }
}
