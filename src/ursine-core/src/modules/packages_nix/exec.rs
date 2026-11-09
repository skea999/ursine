//! Executing `nix profile` AS the target user (never as root).
//!
//! `su -l -s /bin/sh <user> -c '<cmd>'` gives a login environment (HOME,
//! PATH, XDG) for the user's profile. Arguments are strictly validated and
//! single-quoted, so the shell string cannot be influenced by input.

#![allow(clippy::disallowed_methods)] // trusted exec boundary

use std::path::Path;
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::process::Command;
use ursine_api::{is_safe_installable_atom, ApiError, CommandOutcome};

use crate::jobs::JobSink;

/// Max stderr/stdout retained in an error or result (keeps responses sane).
const MAX_OUTPUT: usize = 64 * 1024;

/// Root helpers must never resolve through the daemon's PATH: a writable PATH
/// entry would let a local user substitute a binary executed as root.
const SU_CANDIDATES: [&str; 5] = [
    "/run/wrappers/bin/su",
    "/bin/su",
    "/usr/bin/su",
    "/sbin/su",
    "/usr/sbin/su",
];
const NIX_CANDIDATES: [&str; 4] = [
    "/nix/var/nix/profiles/default/bin/nix",
    "/run/current-system/sw/bin/nix",
    "/usr/bin/nix",
    "/bin/nix",
];

fn trusted_path(candidates: &[&'static str], label: &str) -> Result<&'static str, ApiError> {
    candidates
        .iter()
        .find(|path| Path::new(path).is_file())
        .copied()
        .ok_or_else(|| ApiError::internal(format!("{label} not found at a trusted path")))
}

pub(super) fn su_path() -> Result<&'static str, ApiError> {
    trusted_path(&SU_CANDIDATES, "su")
}

pub(super) fn nix_path() -> Result<&'static str, ApiError> {
    trusted_path(&NIX_CANDIDATES, "nix")
}

/// Flake refs / attr names we accept: no spaces, quotes, semicolons, `$`, etc.
fn is_safe_atom(s: &str) -> bool {
    is_safe_installable_atom(s)
}

/// Values are single-quoted in the `su -c` string, so anything except a quote,
/// newline or NUL is literal. This is what lets regex patterns and paths with
/// spaces reach nix without shell interpretation.
fn is_safe_quoted_value(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 4096
        && !value
            .chars()
            .any(|c| matches!(c, '\'' | '\n' | '\r' | '\0'))
}

/// Validate a username for profile path construction (no traversal).
pub(super) fn validate_username(user: &str) -> Result<(), ApiError> {
    let ok = !user.is_empty()
        && user.len() <= 32
        && user
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_lowercase() || c == '_')
        && user
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '_' | '-'));
    if ok {
        Ok(())
    } else {
        Err(ApiError::invalid("invalid user name"))
    }
}

/// Validate a nix regex pattern (`--regex`): safe inside single quotes, no shell
/// metacharacters that we refuse, no quote/newline. Spaces are allowed.
pub(super) fn validate_regex(pattern: &str) -> Result<(), ApiError> {
    if pattern.is_empty() || pattern.len() > 256 {
        return Err(ApiError::invalid("invalid regex pattern"));
    }
    let ok = pattern.chars().all(|c| {
        c.is_ascii_alphanumeric()
            || matches!(
                c,
                ' ' | '.'
                    | '*'
                    | '?'
                    | '['
                    | ']'
                    | '('
                    | ')'
                    | '{'
                    | '}'
                    | '|'
                    | '^'
                    | '$'
                    | '+'
                    | '-'
                    | '_'
                    | ':'
                    | '/'
                    | '#'
                    | '='
                    | ','
                    | '~'
                    | '@'
                    | '%'
            )
    });
    if ok {
        Ok(())
    } else {
        Err(ApiError::invalid("invalid regex pattern"))
    }
}

/// Validate a search query (nix search takes a regex; keep it conservative).
/// A query with no ASCII alphanumeric character is rejected: `nix search`
/// treats `.` as a regex matching everything.
pub(super) fn validate_query(q: &str) -> Result<(), ApiError> {
    if q.starts_with('-')
        || q.is_empty()
        || q.len() > 128
        || !q
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '+' | '-' | '#'))
        || !q.chars().any(|c| c.is_ascii_alphanumeric())
    {
        return Err(ApiError::invalid("invalid search query"));
    }
    Ok(())
}

/// Validate the target user: must exist AND be a normal user (uid >= 1000).
/// Also called by dispatch for foreign `as_user` targets (root admin CLI).
pub(crate) fn validate_target_user(user: &str) -> Result<(), ApiError> {
    if user == "root" {
        return Err(ApiError::forbidden());
    }
    let passwd = std::fs::read_to_string("/etc/passwd")
        .map_err(|e| ApiError::internal(format!("read /etc/passwd: {e}")))?;
    for line in passwd.lines() {
        let mut parts = line.split(':');
        let (Some(name), Some(_), Some(uid_field)) = (parts.next(), parts.next(), parts.next())
        else {
            continue;
        };
        if name == user {
            let uid: u32 = uid_field.parse().unwrap_or(0);
            // Normal users: 1000..60000 (Debian/Ubuntu/Alpine convention).
            // Excludes system users (<1000) and nobody-style ids (>=60000).
            return if (1000..60000).contains(&uid) {
                Ok(())
            } else {
                Err(ApiError::invalid("target is a system user"))
            };
        }
    }
    Err(ApiError::new(
        ursine_api::ErrorCode::NotFound,
        format!("user {user} does not exist"),
    ))
}

/// `nix search` emits one JSON document: truncating it would corrupt the
/// parse. Allow a bounded but complete document instead.
const MAX_SEARCH_OUTPUT: usize = 32 * 1024 * 1024;

/// How stdout/stderr are bounded while the command runs.
#[derive(Clone)]
enum OutputPolicy {
    /// Keep the first `MAX_OUTPUT` bytes (human-readable commands).
    Truncate,
    /// Reject output larger than the limit; never truncate JSON.
    Complete(usize),
    /// Stream both pipes into a job log; nothing is retained in RAM.
    Stream(Arc<JobSink>),
}

#[derive(Debug)]
struct CapturedOutput {
    status: std::process::ExitStatus,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
}

/// Run a child, draining both pipes concurrently with a hard memory bound.
/// Output is read incrementally: a broad `nix search` cannot allocate the
/// whole JSON document before the cap applies.
async fn run_command(
    mut cmd: Command,
    timeout: Duration,
    policy: OutputPolicy,
    input: Option<Vec<u8>>,
) -> Result<CapturedOutput, ApiError> {
    cmd.stdin(if input.is_some() {
        Stdio::piped()
    } else {
        Stdio::null()
    })
    .stdout(Stdio::piped())
    .stderr(Stdio::piped())
    .kill_on_drop(true)
    .process_group(0);
    let mut child = cmd
        .spawn()
        .map_err(|e| ApiError::internal(format!("spawn command: {e}")))?;
    let pid = child.id();
    let group_for_collect = pid;
    let stdin = child.stdin.take();

    let collect = async move {
        let write_in = async move {
            if let Some(mut stdin) = stdin {
                if let Err(error) = stdin.write_all(input.as_deref().unwrap_or_default()).await {
                    if error.kind() == std::io::ErrorKind::BrokenPipe {
                        return Ok(());
                    }
                    return Err(ApiError::internal(format!("write command stdin: {error}")));
                }
                if let Err(error) = stdin.shutdown().await {
                    if error.kind() != std::io::ErrorKind::BrokenPipe {
                        return Err(ApiError::internal(format!("close command stdin: {error}")));
                    }
                }
            }
            Ok::<(), ApiError>(())
        };
        let mut stdout = child
            .stdout
            .take()
            .ok_or_else(|| ApiError::internal("command stdout unavailable"))?;
        let mut stderr = child
            .stderr
            .take()
            .ok_or_else(|| ApiError::internal("command stderr unavailable"))?;
        let mut out = Vec::new();
        let mut err = Vec::new();
        let mut out_buf = [0u8; 8192];
        let mut err_buf = [0u8; 8192];

        let read_out = async {
            loop {
                let n = stdout
                    .read(&mut out_buf)
                    .await
                    .map_err(|e| ApiError::internal(format!("read command stdout: {e}")))?;
                if n == 0 {
                    break;
                }
                match &policy {
                    OutputPolicy::Stream(sink) => {
                        sink.append(out_buf.get(..n).unwrap_or_default());
                    }
                    OutputPolicy::Truncate => {
                        let room = MAX_OUTPUT.saturating_sub(out.len());
                        out.extend_from_slice(out_buf.get(..room.min(n)).unwrap_or_default());
                    }
                    OutputPolicy::Complete(limit) => {
                        if out.len() + n > *limit {
                            return Err(ApiError::invalid(format!(
                                "command output exceeds {} MiB; narrow the request",
                                *limit / (1024 * 1024)
                            )));
                        }
                        out.extend_from_slice(out_buf.get(..n).unwrap_or_default());
                    }
                }
            }
            Ok::<(), ApiError>(())
        };
        let read_err = async {
            loop {
                let n = stderr
                    .read(&mut err_buf)
                    .await
                    .map_err(|e| ApiError::internal(format!("read command stderr: {e}")))?;
                if n == 0 {
                    break;
                }
                if let OutputPolicy::Stream(sink) = &policy {
                    sink.append(err_buf.get(..n).unwrap_or_default());
                } else {
                    let room = MAX_OUTPUT.saturating_sub(err.len());
                    err.extend_from_slice(err_buf.get(..room.min(n)).unwrap_or_default());
                }
            }
            Ok::<(), ApiError>(())
        };

        if let Err(error) = tokio::try_join!(read_out, read_err, write_in) {
            let _ = child.start_kill();
            if let Some(pid) = group_for_collect {
                kill_process_group(pid);
            }
            let _ = child.wait().await;
            return Err(error);
        }
        let status = child
            .wait()
            .await
            .map_err(|e| ApiError::internal(format!("wait for command: {e}")))?;
        Ok(CapturedOutput {
            status,
            stdout: out,
            stderr: err,
        })
    };

    tokio::time::timeout(timeout, collect)
        .await
        .unwrap_or_else(|_| {
            // kill_on_drop kills `su`; the process group also holds the nix
            // child, so kill the whole group by the child pid.
            if let Some(pid) = pid {
                kill_process_group(pid);
            }
            Err(ApiError::new(
                ursine_api::ErrorCode::Internal,
                format!("command timed out after {}s", timeout.as_secs()),
            ))
        })
}

/// Kill a process group with a trusted absolute `kill` binary (no PATH lookup).
fn kill_process_group(pid: u32) {
    for path in [
        "/run/current-system/sw/bin/kill",
        "/nix/var/nix/profiles/default/bin/kill",
        "/bin/kill",
        "/usr/bin/kill",
    ] {
        if Path::new(path).is_file() {
            let _ = std::process::Command::new(path)
                .arg("-KILL")
                .arg(format!("-{pid}"))
                .status();
            return;
        }
    }
    tracing::warn!(
        pid,
        "no trusted kill binary; timed-out process group may survive"
    );
}

/// Run `nix <args...>` as `user`.
pub(super) async fn run_as_user(
    user: &str,
    args: &[(&str, &str)],
    timeout: Duration,
) -> Result<CommandOutcome, ApiError> {
    run_as_user_limited(user, "nix", args, timeout, OutputPolicy::Truncate).await
}

/// Run `nix search` as `user`, keeping the JSON document intact.
pub(super) async fn run_as_user_search(
    user: &str,
    args: &[(&str, &str)],
    timeout: Duration,
) -> Result<CommandOutcome, ApiError> {
    run_as_user_limited(
        user,
        "nix",
        args,
        timeout,
        OutputPolicy::Complete(MAX_SEARCH_OUTPUT),
    )
    .await
}

/// Run a constant shell script as `user` (used to resolve the user's profile
/// directory with the same environment `nix profile` sees). The script is a
/// fixed literal: it contains no input and no single quotes.
pub(crate) async fn run_as_user_script(
    user: &str,
    script: &str,
    timeout: Duration,
) -> Result<CommandOutcome, ApiError> {
    if user != "root" {
        validate_target_user(user)?;
    }
    if script.contains('\'') || script.contains('\n') || script.contains('\0') {
        return Err(ApiError::invalid("invalid command script"));
    }
    // Root = the calling process: run the script directly (no su; foreign
    // root targets are rejected by dispatch before this point).
    let cmd = if user == "root" {
        let mut c = Command::new("/bin/sh");
        c.arg("-c").arg(script);
        c
    } else {
        let mut c = Command::new(su_path()?);
        c.arg("-l")
            .arg("-s")
            .arg("/bin/sh")
            .arg(user)
            .arg("-c")
            .arg(script);
        c
    };
    let output = run_command(cmd, timeout, OutputPolicy::Truncate, None).await?;
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    if output.status.success() {
        Ok(CommandOutcome { stdout, stderr })
    } else {
        Err(ApiError::internal(format!(
            "command failed: {}",
            if stderr.trim().is_empty() {
                stdout.trim()
            } else {
                stderr.trim()
            }
        )))
    }
}

/// Run an explicit argv as `user`. Every element is single-quoted, so flags and
/// names are literal; the program itself must be a safe atom.
pub async fn run_as_user_argv(
    user: &str,
    program: &str,
    argv: &[&str],
    timeout: Duration,
) -> Result<CommandOutcome, ApiError> {
    let cmd = su_argv_command(user, program, argv)?;
    let output = run_command(cmd, timeout, OutputPolicy::Truncate, None).await?;
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    if output.status.success() {
        Ok(CommandOutcome { stdout, stderr })
    } else {
        Err(ApiError::internal(format!(
            "command failed: {}",
            if stderr.trim().is_empty() {
                stdout.trim()
            } else {
                stderr.trim()
            }
        )))
    }
}

/// Like [`run_as_user_argv_bounded`] but the target user IS root.
///
/// No `su`, no shell — the calling process already runs as root. Used by
/// the crontab module when the session user is root (`ursinectl`).
pub async fn run_root_argv_bounded(
    program: &str,
    argv: &[&str],
    timeout: Duration,
    max_bytes: usize,
) -> Result<CommandOutcome, ApiError> {
    let cmd = root_command(program, argv)?;
    let output = run_command(cmd, timeout, OutputPolicy::Complete(max_bytes), None).await?;
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    if output.status.success() {
        Ok(CommandOutcome { stdout, stderr })
    } else {
        Err(ApiError::internal(format!(
            "command failed: {}",
            if stderr.trim().is_empty() {
                stdout.trim()
            } else {
                stderr.trim()
            }
        )))
    }
}

/// Like [`run_as_user_argv`] but with an explicit byte budget: output over
/// `max_bytes` is rejected instead of silently truncated (crontab reads —
/// a truncated read followed by a save would destroy the tail).
pub async fn run_as_user_argv_bounded(
    user: &str,
    program: &str,
    argv: &[&str],
    timeout: Duration,
    max_bytes: usize,
) -> Result<CommandOutcome, ApiError> {
    let cmd = su_argv_command(user, program, argv)?;
    let output = run_command(cmd, timeout, OutputPolicy::Complete(max_bytes), None).await?;
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    if output.status.success() {
        Ok(CommandOutcome { stdout, stderr })
    } else {
        Err(ApiError::internal(format!(
            "command failed: {}",
            if stderr.trim().is_empty() {
                stdout.trim()
            } else {
                stderr.trim()
            }
        )))
    }
}

/// Read a per-user file as that user. Output is capped at 1 MiB plus the
/// existence marker; callers must treat larger content as invalid.
pub async fn run_as_user_read_file(user: &str, path: &str) -> Result<CommandOutcome, ApiError> {
    if user != "root" {
        validate_target_user(user)?;
    }
    if !is_safe_quoted_value(path) {
        return Err(ApiError::invalid("invalid file path"));
    }
    let script = r#"PATH=/usr/bin:/bin
if [ -e "$1" ]; then printf 1; cat -- "$1"; else printf 0; fi"#;
    let cmd = if user == "root" {
        let mut c = Command::new("/bin/sh");
        c.arg("-c").arg(script).arg("sh").arg(path);
        c
    } else {
        let mut c = Command::new(su_path()?);
        c.arg("-s")
            .arg("/bin/sh")
            .arg(user)
            .arg("-c")
            .arg(script)
            .arg("sh")
            .arg(path);
        c
    };
    let output = run_command(
        cmd,
        Duration::from_secs(30),
        OutputPolicy::Complete(1024 * 1024 + 2),
        None,
    )
    .await?;
    if !output.status.success() {
        return Err(ApiError::internal("target user could not read the config"));
    }
    Ok(CommandOutcome {
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    })
}

/// Validate a proposed niri config as the target user, using a temporary file
/// in the user's config directory. Never runs user-controlled config as root.
pub async fn run_as_user_validate(
    user: &str,
    path: &str,
    niri: &str,
    content: &str,
) -> Result<(bool, String, String), ApiError> {
    validate_target_user(user)?;
    if !is_safe_quoted_value(path) || !is_safe_atom(niri) {
        return Err(ApiError::invalid("invalid niri command"));
    }
    let script = r#"set -eu
PATH=/usr/bin:/bin
umask 077
dir=${1%/*}
mkdir -p -- "$dir"
tmp=$(mktemp "$dir/.niri-validate.XXXXXX")
trap 'rm -f -- "$tmp"' EXIT
cat > "$tmp"
"$2" validate -c "$tmp"
"#;
    let output = run_user_script_capture(user, script, &[path, niri], content).await?;
    Ok((
        output.status.success(),
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    ))
}

/// Rotation core shared by every write script: `ursine_rotate <file>
/// <copies>` shifts `slot(i-1) → slot(i)` for `i = copies-1 … 1`
/// (slot 0 = `<file>.ursine-backup`), drops stale slots a higher count
/// left behind plus the pre-uniform dot-prefixed slots
/// (`.<basename>.ursine-backup*`), then copies the live file into slot 0.
/// `copies` arrives clamped to 1..=10 from Rust; 0/garbage → 42.
const ROTATE_FN: &str = r#"ursine_rotate() {
  f=$1
  n=$2
  case $n in ''|*[!0-9]*|0) return 42;; esac
  dir=${f%/*}
  base=$f.ursine-backup
  i=$((n - 1))
  while [ "$i" -ge 1 ]; do
    if [ "$i" -eq 1 ]; then src=$base; else src=$base.$((i - 1)); fi
    dst=$base.$i
    if [ -e "$src" ]; then
      if [ -e "$dst" ]; then rm -rf -- "$dst" || return 42; fi
      mv -- "$src" "$dst" || return 42
    fi
    i=$((i - 1))
  done
  k=$n
  while [ -e "$base.$k" ]; do rm -rf -- "$base.$k" || return 42; k=$((k + 1)); done
  old=$dir/.${f##*/}.ursine-backup
  if [ -e "$old" ]; then rm -rf -- "$old" "$old".[0-9]* || return 42; fi
  if [ -e "$f" ]; then
    cp -p -- "$f" "$base" || return 42
    sync
  fi
}
"#;

/// Validate and atomically replace a niri config as the target user.
///
/// Args: `$1` config path, `$2` niri binary, `$3` backup copies. The
/// previous file rotates through `<config>.ursine-backup[.N]` — at most
/// `backup_configs.copies` copies; invalid content never replaces the
/// live file. Rotation runs AFTER a successful `niri validate` so a
/// rejected save never evicts good history; rotation failure is exit 42
/// (distinct from validation failure).
fn niri_write_script() -> String {
    let mut script = String::from("set -eu\nPATH=/usr/bin:/bin\numask 077\n");
    script.push_str(ROTATE_FN);
    script.push_str(
        r#"dir=${1%/*}
mkdir -p -- "$dir"
tmp=$(mktemp "$dir/.niri-write.XXXXXX")
trap 'rm -f -- "$tmp"' EXIT
cat > "$tmp"
sync
"$2" validate -c "$tmp"
ursine_rotate "$1" "$3" || exit 42
mv -- "$tmp" "$1"
trap - EXIT
"#,
    );
    script
}

pub async fn run_as_user_write(
    user: &str,
    path: &str,
    niri: &str,
    content: &str,
    copies: usize,
) -> Result<CommandOutcome, ApiError> {
    validate_target_user(user)?;
    if !is_safe_quoted_value(path) || !is_safe_atom(niri) {
        return Err(ApiError::invalid("invalid niri command"));
    }
    let script = niri_write_script();
    let copies = copies.to_string();
    let output = run_user_script_capture(user, &script, &[path, niri, &copies], content).await?;
    if !output.status.success() {
        if output.status.code() == Some(42) {
            return Err(ApiError::internal(format!(
                "backup rotation failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            )));
        }
        return Err(ApiError::invalid(
            "niri rejected the configuration; run Validate for details",
        ));
    }
    Ok(CommandOutcome {
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    })
}

/// Atomically replace `path` with `content` as `user`.
///
/// Args: `$1` target path, `$2` backup copies. Root runs the same script
/// directly, no su. The previous file rotates through
/// `<path>.ursine-backup[.N]` — the same uniform slots as every other
/// writer; no validation step (callers own the content).
fn atomic_write_script() -> String {
    let mut script = String::from("set -eu\nPATH=/usr/bin:/bin\numask 077\n");
    script.push_str(ROTATE_FN);
    script.push_str(
        r#"dir=${1%/*}
mkdir -p -- "$dir"
tmp=$(mktemp "$dir/.ursine-write.XXXXXX")
trap 'rm -f -- "$tmp"' EXIT
cat > "$tmp"
sync
ursine_rotate "$1" "$2" || exit 42
mv -- "$tmp" "$1"
trap - EXIT
"#,
    );
    script
}

pub async fn run_as_user_atomic_write(
    user: &str,
    path: &str,
    content: &str,
    copies: usize,
) -> Result<CommandOutcome, ApiError> {
    if user != "root" {
        validate_target_user(user)?;
    }
    if !is_safe_quoted_value(path) {
        return Err(ApiError::invalid("invalid file path"));
    }
    let script = atomic_write_script();
    let copies = copies.to_string();
    let output = run_user_script_capture(user, &script, &[path, &copies], content).await?;
    if !output.status.success() {
        if output.status.code() == Some(42) {
            return Err(ApiError::internal(format!(
                "backup rotation failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            )));
        }
        return Err(ApiError::internal(format!(
            "write failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    Ok(CommandOutcome {
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    })
}

async fn run_user_script_capture(
    user: &str,
    script: &str,
    args: &[&str],
    input: &str,
) -> Result<CapturedOutput, ApiError> {
    // Root = the calling process: same script, direct exec, no su.
    let mut cmd = if user == "root" {
        let mut c = Command::new("/bin/sh");
        c.arg("-c").arg(script).arg("sh");
        c
    } else {
        let mut c = Command::new(su_path()?);
        c.arg("-s")
            .arg("/bin/sh")
            .arg(user)
            .arg("-c")
            .arg(script)
            .arg("sh");
        c
    };
    for arg in args {
        if !is_safe_quoted_value(arg) {
            return Err(ApiError::invalid("invalid command argument"));
        }
        cmd.arg(arg);
    }
    run_command(
        cmd,
        Duration::from_secs(60),
        OutputPolicy::Truncate,
        Some(input.as_bytes().to_vec()),
    )
    .await
}

async fn run_as_user_limited(
    user: &str,
    program: &str,
    args: &[(&str, &str)],
    timeout: Duration,
    stdout_policy: OutputPolicy,
) -> Result<CommandOutcome, ApiError> {
    let cmd = su_pairs_command(user, program, args)?;
    let output = run_command(cmd, timeout, stdout_policy, None).await?;
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();

    if output.status.success() {
        Ok(CommandOutcome { stdout, stderr })
    } else {
        let detail = if stderr.trim().is_empty() {
            stdout.trim().to_string()
        } else {
            stderr.trim().to_string()
        };
        Err(ApiError::new(
            ursine_api::ErrorCode::Internal,
            format!("nix command failed: {detail}"),
        ))
    }
}

/// Build `su -l -s /bin/sh <user> -c '<program flag 'value' …>'`.
/// Flags are atoms; values are single-quoted (regex patterns and paths with
/// spaces stay literal, shell metacharacters never reach the shell).
fn su_pairs_command(user: &str, program: &str, args: &[(&str, &str)]) -> Result<Command, ApiError> {
    if !is_safe_atom(program) {
        return Err(ApiError::invalid("invalid program"));
    }
    // Root = the calling process: direct exec, no su, no shell (pairs
    // flatten to a plain argv; empty values are flags without a value).
    if user == "root" {
        let mut cmd = Command::new(program);
        for (flag, value) in args {
            if !is_safe_atom(flag) || (!value.is_empty() && !is_safe_quoted_value(value)) {
                return Err(ApiError::invalid("invalid command argument"));
            }
            cmd.arg(flag);
            if !value.is_empty() {
                cmd.arg(value);
            }
        }
        return Ok(cmd);
    }
    validate_target_user(user)?;
    let mut cmdline = String::from(program);
    for (flag, value) in args {
        if !is_safe_atom(flag) || (!value.is_empty() && !is_safe_quoted_value(value)) {
            return Err(ApiError::invalid("invalid command argument"));
        }
        cmdline.push(' ');
        cmdline.push_str(flag);
        if !value.is_empty() {
            cmdline.push_str(" '");
            cmdline.push_str(value);
            cmdline.push('\'');
        }
    }
    let mut cmd = Command::new(su_path()?);
    cmd.arg("-l")
        .arg("-s")
        .arg("/bin/sh")
        .arg(user)
        .arg("-c")
        .arg(&cmdline);
    Ok(cmd)
}

/// Build `su -l -s /bin/sh <user> -c '<program 'arg' …>'` for an explicit
/// argv; every element is single-quoted, so flags and names are literal.
fn su_argv_command(user: &str, program: &str, argv: &[&str]) -> Result<Command, ApiError> {
    // Root is the calling process (daemon/CLI): direct exec — no `su`,
    // no shell. This is what makes the root-only CLI able to run nix/
    // scanners/niri operations that default to the principal being root.
    // Foreign root targets stay rejected: dispatch validates them with
    // `validate_target_user` before any handler runs.
    if user == "root" {
        if !is_safe_atom(program) {
            return Err(ApiError::invalid("invalid program"));
        }
        let mut cmd = Command::new(program);
        cmd.args(argv);
        return Ok(cmd);
    }
    validate_target_user(user)?;
    if !is_safe_atom(program) {
        return Err(ApiError::invalid("invalid program"));
    }
    let mut cmdline = String::from(program);
    for arg in argv {
        if !is_safe_quoted_value(arg) {
            return Err(ApiError::invalid("invalid command argument"));
        }
        cmdline.push_str(" '");
        cmdline.push_str(arg);
        cmdline.push('\'');
    }
    let mut cmd = Command::new(su_path()?);
    cmd.arg("-l")
        .arg("-s")
        .arg("/bin/sh")
        .arg(user)
        .arg("-c")
        .arg(&cmdline);
    Ok(cmd)
}

/// Exit code reported to a job: `-1` when the process died on a signal.
fn job_exit_code(output: &CapturedOutput) -> i32 {
    output.status.code().unwrap_or(-1)
}

/// Run `nix <args…>` as `user`, streaming terminal output into a job log.
/// Non-zero exits are returned as codes, not errors: the log carries detail.
pub async fn run_as_user_nix_log(
    user: &str,
    args: &[(&str, &str)],
    timeout: Duration,
    sink: Arc<JobSink>,
) -> Result<i32, ApiError> {
    let cmd = su_pairs_command(user, "nix", args)?;
    let output = run_command(cmd, timeout, OutputPolicy::Stream(sink), None).await?;
    Ok(job_exit_code(&output))
}

/// Run an explicit argv as `user`, streaming into a job log; returns the
/// exit code (`-1` on signal kill).
pub async fn run_as_user_argv_log(
    user: &str,
    program: &str,
    argv: &[&str],
    timeout: Duration,
    sink: Arc<JobSink>,
) -> Result<i32, ApiError> {
    let cmd = su_argv_command(user, program, argv)?;
    let output = run_command(cmd, timeout, OutputPolicy::Stream(sink), None).await?;
    Ok(job_exit_code(&output))
}

/// Run a trusted program as root with literal args and stdin input.
pub async fn run_root_program_quoted_stdin(
    program: &str,
    args: &[&str],
    timeout: Duration,
    input: &str,
) -> Result<CommandOutcome, ApiError> {
    if !is_safe_atom(program) {
        return Err(ApiError::invalid("invalid program"));
    }
    let mut cmd = Command::new(program);
    for arg in args {
        if arg.len() > 4096 || arg.contains('\0') || arg.contains('\n') {
            return Err(ApiError::invalid("invalid command argument"));
        }
        cmd.arg(arg);
    }
    let output = run_command(
        cmd,
        timeout,
        OutputPolicy::Truncate,
        Some(format!("{input}\n").into_bytes()),
    )
    .await?;
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    if output.status.success() {
        Ok(CommandOutcome { stdout, stderr })
    } else {
        Err(ApiError::new(
            ursine_api::ErrorCode::Internal,
            format!(
                "command failed: {}",
                if stderr.trim().is_empty() {
                    stdout.trim()
                } else {
                    stderr.trim()
                }
            ),
        ))
    }
}

/// Run a trusted program as root with literal args (no shell).
pub async fn run_root_program_quoted(
    program: &str,
    args: &[&str],
    timeout: Duration,
) -> Result<CommandOutcome, ApiError> {
    if !is_safe_atom(program) {
        return Err(ApiError::invalid("invalid program"));
    }
    let mut cmd = Command::new(program);
    for arg in args {
        if arg.len() > 4096 || arg.contains('\0') || arg.contains('\n') {
            return Err(ApiError::invalid("invalid command argument"));
        }
        cmd.arg(arg);
    }
    let output = run_command(cmd, timeout, OutputPolicy::Truncate, None).await?;
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    if output.status.success() {
        Ok(CommandOutcome { stdout, stderr })
    } else {
        Err(ApiError::new(
            ursine_api::ErrorCode::Internal,
            format!(
                "command failed: {}",
                if stderr.trim().is_empty() {
                    stdout.trim()
                } else {
                    stderr.trim()
                }
            ),
        ))
    }
}

/// Build a root argv command (no shell). Every element is an atom; a
/// comma-separated list (`usermod -G`) is still atom-only per element.
fn root_command(program: &str, args: &[&str]) -> Result<Command, ApiError> {
    if !is_safe_atom(program) {
        return Err(ApiError::invalid("invalid program"));
    }
    let mut cmd = Command::new(program);
    for arg in args {
        let safe = arg.is_empty() || arg.split(',').all(is_safe_atom);
        if !safe {
            return Err(ApiError::invalid("invalid command argument"));
        }
        cmd.arg(arg);
    }
    Ok(cmd)
}

/// Run a validated argv as ROOT (no shell is involved). Used for store-wide
/// operations and for profile commands users cannot lock themselves.
/// Run a validated argv as root (no shell is involved) with a
/// per-argument guard; empty arguments are always allowed (nmcli clears
/// properties with `""`).
async fn run_root_program_guarded(
    program: &str,
    args: &[&str],
    timeout: Duration,
    guard: impl Fn(&str) -> bool,
) -> Result<CommandOutcome, ApiError> {
    if !is_safe_atom(program) {
        return Err(ApiError::invalid("invalid program"));
    }
    let mut cmd = Command::new(program);
    for arg in args {
        if !guard(arg) {
            return Err(ApiError::invalid("invalid command argument"));
        }
        cmd.arg(arg);
    }
    let output = run_command(cmd, timeout, OutputPolicy::Truncate, None).await?;
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    if output.status.success() {
        Ok(CommandOutcome { stdout, stderr })
    } else {
        let detail = if stderr.trim().is_empty() {
            stdout.trim().to_string()
        } else {
            stderr.trim().to_string()
        };
        Err(ApiError::new(
            ursine_api::ErrorCode::Internal,
            format!("command failed: {detail}"),
        ))
    }
}

pub async fn run_root_program(
    program: &str,
    args: &[&str],
    timeout: Duration,
) -> Result<CommandOutcome, ApiError> {
    run_root_program_guarded(program, args, timeout, |arg| {
        arg.is_empty() || arg.split(',').all(is_safe_atom)
    })
    .await
}

/// Run a validated argv as root (no shell is involved).
///
/// Like [`run_root_program`] but the per-argument guard is the quoted
/// value rule (spaces allowed — nothing interprets them without a
/// shell): for nmcli connection names such as `Home Wi-Fi`.
pub async fn run_root_program_value(
    program: &str,
    args: &[&str],
    timeout: Duration,
) -> Result<CommandOutcome, ApiError> {
    run_root_program_guarded(program, args, timeout, |arg| {
        arg.is_empty() || is_safe_quoted_value(arg)
    })
    .await
}

/// Run a validated argv as ROOT, streaming terminal output into a job log.
/// Returns the exit code (`-1` on signal kill); non-zero is not an error.
pub async fn run_root_program_log(
    program: &str,
    args: &[&str],
    timeout: Duration,
    sink: Arc<JobSink>,
) -> Result<i32, ApiError> {
    let cmd = root_command(program, args)?;
    let output = run_command(cmd, timeout, OutputPolicy::Stream(sink), None).await?;
    Ok(job_exit_code(&output))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Run the atomic write script for real: `copies = 3` must keep
    /// exactly three generations (live + 3 slots = 4 versions total),
    /// `copies = 1` must keep one slot, and a rejected copies value must
    /// exit 42 without touching anything.
    #[test]
    fn atomic_write_script_rotates_to_the_configured_count() {
        use std::io::Write as _;
        use std::process::{Command, Stdio};

        let dir = std::env::temp_dir().join(format!("ursine-rotate-sh-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let target = dir.join("conf.ini");
        let script = atomic_write_script();

        let run = |content: &str, copies: &str| -> i32 {
            let mut child = Command::new("/bin/sh")
                .arg("-c")
                .arg(&script)
                .arg("sh")
                .arg(&target)
                .arg(copies)
                .stdin(Stdio::piped())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .unwrap();
            child
                .stdin
                .as_mut()
                .unwrap()
                .write_all(content.as_bytes())
                .unwrap();
            child.wait().unwrap().code().unwrap()
        };

        // Five saves, cap 3 → live holds v5, slots hold v4..v2, v1 gone.
        assert_eq!(run("v1\n", "3"), 0, "first save");
        for v in 2..=5 {
            assert_eq!(run(&format!("v{v}\n"), "3"), 0, "save {v}");
        }
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "v5\n");
        for (slot, expect) in [(0, "v4\n"), (1, "v3\n"), (2, "v2\n")] {
            let path = crate::backup_configs::slot_path(&target, slot);
            assert_eq!(
                std::fs::read_to_string(&path).unwrap(),
                expect,
                "slot {slot}"
            );
        }
        assert!(
            !crate::backup_configs::slot_path(&target, 3).exists(),
            "v1 must have rotated out"
        );

        // copies = 1: only the newest previous version survives.
        assert_eq!(run("only\n", "1"), 0);
        assert_eq!(
            std::fs::read_to_string(crate::backup_configs::slot_path(&target, 0)).unwrap(),
            "v5\n"
        );
        assert!(
            !crate::backup_configs::slot_path(&target, 1).exists(),
            "cap 1 must drop stale .1/.2 left by cap 3"
        );

        // Garbage/zero copies: exit 42, live file untouched.
        assert_eq!(run("nope\n", "0"), 42, "copies=0 must be rejected");
        assert_eq!(run("nope\n", "x9"), 42, "garbage must be rejected");
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "only\n");

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The niri script must validate before rotating: a rejected config
    /// must not evict backup history, and a rotation failure must be 42.
    #[test]
    fn niri_write_script_validates_before_rotating() {
        let script = niri_write_script();
        let validate = script.find("validate").expect("must validate");
        let rotate = script
            .find("ursine_rotate \"$1\"")
            .expect("must rotate after");
        assert!(validate < rotate, "validate must precede rotation");
        assert!(script.contains("|| exit 42"), "rotation failure = 42");
        // Both scripts share one rotation implementation.
        assert_eq!(script.matches(ROTATE_FN).count(), 1);
        assert_eq!(atomic_write_script().matches(ROTATE_FN).count(), 1);
    }

    #[tokio::test]
    async fn value_guard_allows_spaces_and_empty_but_not_quotes() {
        // Direct exec: no shell, so spaces are literal; empty clears a
        // property; a quote would break other quoted-value callers.
        let ok = run_root_program_value("/bin/true", &["a b", ""], Duration::from_secs(5)).await;
        assert!(ok.is_ok(), "{ok:?}");
        let bad = run_root_program_value("/bin/true", &["it's"], Duration::from_secs(5)).await;
        assert!(bad.is_err(), "{bad:?}");
    }

    #[test]
    fn accepts_typical_installables() {
        for arg in [
            "hello",
            "nixpkgs#hello",
            "github:owner/repo#pkg",
            "nixpkgs#python3Packages.requests",
        ] {
            assert!(
                ursine_api::validate_installable(arg).is_ok(),
                "{arg} should pass"
            );
        }
    }

    #[test]
    fn rejects_option_like_installables() {
        for arg in ["--all", "-p", "-x"] {
            assert!(
                ursine_api::validate_installable(arg).is_err(),
                "{arg:?} must not become a CLI flag"
            );
        }
    }

    #[test]
    fn rejects_shell_metacharacters() {
        for arg in [
            "hello; rm -rf /",
            "a'b",
            "a b",
            "$(id)",
            "`id`",
            "a|b",
            "a&b",
            "a\nb",
            "",
        ] {
            assert!(
                ursine_api::validate_installable(arg).is_err(),
                "{arg:?} must be rejected"
            );
        }
    }

    #[test]
    fn regex_validation_allows_common_patterns() {
        assert!(validate_regex(".*vim.*").is_ok());
        assert!(validate_regex("python3(11|12)?").is_ok());
        assert!(validate_regex("hello world").is_ok());
        assert!(validate_regex("bad'quote").is_err());
        assert!(validate_regex("bad;rm").is_err());
        assert!(validate_regex("back`tick").is_err());
        assert!(validate_regex("").is_err());
    }

    #[test]
    fn username_validation_blocks_traversal() {
        assert!(validate_username("skea999").is_ok());
        assert!(validate_username("_svc").is_ok());
        assert!(validate_username("Root").is_err());
        assert!(validate_username("../root").is_err());
        assert!(validate_username("a/b").is_err());
        assert!(validate_username("").is_err());
    }

    #[test]
    fn rejects_option_like_queries() {
        for query in ["--all", "-p", "-x"] {
            assert!(
                validate_query(query).is_err(),
                "{query:?} must not become a CLI flag"
            );
        }
    }

    #[test]
    fn rejects_bad_queries() {
        assert!(validate_query("hello").is_ok());
        assert!(validate_query("python3.12").is_ok());
        assert!(validate_query("hello world").is_err());
        assert!(validate_query("a;b").is_err());
    }

    #[test]
    fn rejects_wildcard_only_queries() {
        for query in [".", "...", "_", "+", "-", "#", ".-_+#"] {
            assert!(
                validate_query(query).is_err(),
                "{query:?} must not match everything"
            );
        }
    }

    #[test]
    fn quoted_values_allow_regex_and_spaces() {
        assert!(is_safe_quoted_value("^python3(11|12)? .*"));
        assert!(is_safe_quoted_value("hello world"));
        assert!(!is_safe_quoted_value("bad'quote"));
        assert!(!is_safe_quoted_value("bad\nnewline"));
    }

    #[tokio::test]
    async fn streaming_complete_rejects_oversized_output() {
        let mut cmd = Command::new("sh");
        cmd.arg("-c").arg("head -c 4096 /dev/zero");
        let error = run_command(
            cmd,
            Duration::from_secs(5),
            OutputPolicy::Complete(1024),
            None,
        )
        .await
        .unwrap_err();
        assert!(error.message.contains("exceeds"), "{error}");
    }

    #[tokio::test]
    async fn streaming_truncate_keeps_bounded_output() {
        let mut cmd = Command::new("sh");
        cmd.arg("-c").arg("head -c 200000 /dev/zero");
        let output = run_command(cmd, Duration::from_secs(5), OutputPolicy::Truncate, None)
            .await
            .unwrap();
        assert!(output.stdout.len() <= MAX_OUTPUT);
    }

    #[tokio::test]
    async fn timeout_returns_without_waiting_for_the_child() {
        let start = std::time::Instant::now();
        let mut cmd = Command::new("sh");
        cmd.arg("-c").arg("sleep 5");
        let error = run_command(
            cmd,
            Duration::from_millis(100),
            OutputPolicy::Truncate,
            None,
        )
        .await
        .unwrap_err();
        assert!(error.message.contains("timed out"), "{error}");
        assert!(start.elapsed() < Duration::from_secs(3));
    }

    #[test]
    fn target_user_rejects_root_and_system_users() {
        assert!(validate_target_user("root").is_err());
        // …but the su chokepoint execs root DIRECTLY (no su): the root
        // CLI defaults every untargeted profile op to its own account.
        let cmd = su_argv_command("root", "nix", &["profile", "list"]).unwrap();
        // tokio's Command has no accessors; its Debug shows the argv:
        // `Command { std: "nix" "profile" "list", … }` — an su wrapper
        // would instead start with the su binary path.
        let dbg = format!("{cmd:?}");
        assert!(dbg.contains("\"nix\" \"profile\" \"list\""), "{dbg}");
        assert!(!dbg.contains("su_path") && !dbg.contains("/su\""), "{dbg}");
        // Unknown non-root users are still rejected before any exec.
        assert!(su_argv_command("no-such-user", "nix", &["profile", "list"]).is_err());
        // The pairs builder (nix list/search/upgrade) follows the same
        // root shortcut: pairs flatten to a plain argv, no su.
        let cmd = su_pairs_command(
            "root",
            "nix",
            &[("profile", ""), ("list", ""), ("--json", "")],
        )
        .unwrap();
        let dbg = format!("{cmd:?}");
        assert!(
            dbg.contains("\"nix\" \"profile\" \"list\" \"--json\""),
            "{dbg}"
        );
        assert!(su_pairs_command("no-such-user", "nix", &[("profile", "")]).is_err());
        // System users have low uids on every normal system.
        if std::fs::read_to_string("/etc/passwd").is_ok() {
            assert!(validate_target_user("bin").is_err());
            assert!(validate_target_user("definitely-not-a-user").is_err());
        }
    }
}
