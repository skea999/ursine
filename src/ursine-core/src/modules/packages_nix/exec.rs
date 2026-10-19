//! Executing `nix profile` AS the target user (never as root).
//!
//! `su -l -s /bin/sh <user> -c '<cmd>'` gives a login environment (HOME,
//! PATH, XDG) for the user's profile. Arguments are strictly validated and
//! single-quoted, so the shell string cannot be influenced by input.

use std::time::Duration;

use tokio::process::Command;
use ursine_api::{ApiError, CommandOutcome};

/// Max stderr/stdout retained in an error or result (keeps responses sane).
const MAX_OUTPUT: usize = 64 * 1024;

/// Flake refs / attr names we accept: no spaces, quotes, semicolons, `$`, etc.
fn is_safe_atom(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 512
        && s.chars().all(|c| {
            c.is_ascii_alphanumeric()
                || matches!(c, '.' | '_' | '+' | '-' | ':' | '/' | '#' | '?' | '=')
        })
}

/// Validate a package/installable argument.
pub fn validate_installable(arg: &str) -> Result<(), ApiError> {
    if !is_safe_atom(arg) {
        return Err(ApiError::invalid("invalid package reference"));
    }
    Ok(())
}

/// Validate a search query (nix search takes a regex; keep it conservative).
pub fn validate_query(q: &str) -> Result<(), ApiError> {
    if q.is_empty()
        || q.len() > 128
        || !q
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '+' | '-' | '#'))
    {
        return Err(ApiError::invalid("invalid search query"));
    }
    Ok(())
}

/// Validate the target user: must exist AND be a normal user (uid >= 1000).
pub fn validate_target_user(user: &str) -> Result<(), ApiError> {
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
        "unknown user",
    ))
}

/// Run `nix <args...>` as `user`, capturing output. Timeout kills the child.
pub async fn run_as_user(
    user: &str,
    args: &[(&str, &str)],
    timeout: Duration,
) -> Result<CommandOutcome, ApiError> {
    validate_target_user(user)?;

    // Build the nix command line: every atom is validated upstream, and we
    // single-quote anyway (defense in depth).
    let mut nix_cmd = String::from("nix");
    for (flag, value) in args {
        if !is_safe_atom(flag) || (!value.is_empty() && !is_safe_atom(value)) {
            return Err(ApiError::invalid("invalid command argument"));
        }
        nix_cmd.push(' ');
        nix_cmd.push_str(flag);
        if !value.is_empty() {
            nix_cmd.push_str(" '");
            nix_cmd.push_str(value);
            nix_cmd.push('\'');
        }
    }

    let mut cmd = Command::new("su");
    cmd.arg("-l")
        .arg("-s")
        .arg("/bin/sh")
        .arg(user)
        .arg("-c")
        .arg(&nix_cmd)
        .kill_on_drop(true);

    let output = tokio::time::timeout(timeout, cmd.output())
        .await
        .map_err(|_| {
            ApiError::new(
                ursine_api::ErrorCode::Internal,
                format!("nix command timed out after {}s", timeout.as_secs()),
            )
        })?
        .map_err(|e| ApiError::internal(format!("spawn su/nix: {e}")))?;

    let mut stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let mut stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    truncate(&mut stdout, MAX_OUTPUT);
    truncate(&mut stderr, MAX_OUTPUT);

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

fn truncate(s: &mut String, max: usize) {
    if s.len() > max {
        // Cut at a char boundary to keep UTF-8 valid.
        let mut cut = max;
        while !s.is_char_boundary(cut) {
            cut -= 1;
        }
        s.truncate(cut);
        s.push_str("\n…(truncated)");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_typical_installables() {
        for arg in [
            "hello",
            "nixpkgs#hello",
            "github:owner/repo#pkg",
            "nixpkgs#python3Packages.requests",
        ] {
            assert!(validate_installable(arg).is_ok(), "{arg} should pass");
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
                validate_installable(arg).is_err(),
                "{arg:?} must be rejected"
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
    fn target_user_rejects_root_and_system_users() {
        assert!(validate_target_user("root").is_err());
        // System users have low uids on every normal system.
        if std::fs::read_to_string("/etc/passwd").is_ok() {
            assert!(validate_target_user("bin").is_err());
            assert!(validate_target_user("definitely-not-a-user").is_err());
        }
    }
}
