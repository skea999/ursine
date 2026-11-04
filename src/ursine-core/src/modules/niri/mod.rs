//! niri — schema-driven editing of managed `config.kdl` sections.
//!
//! All file I/O and validation run as the target user through `su`. The root
//! daemon never opens, chowns, validates or replaces a user-controlled path
//! itself; a compromised home cannot become a root write or read primitive.

mod kdl;
mod validate_errors;

use std::path::{Path, PathBuf};
use std::time::Duration;

use ::kdl::KdlDocument;
use serde_json::json;
use ursine_api::{Action, ApiError, NiriConfigView, NiriValues};

use super::ModuleCtx;
use crate::modules::packages_nix::exec;

const MAX_CONFIG: usize = 1024 * 1024;
/// Oldest niri release the managed sections are tested against (`CalVer`
/// `YY.MM`; compared as `(major, minor)` against `niri --version`).
const NIRI_MIN_SUPPORTED: &str = "25.05";
/// Tuple form of [`NIRI_MIN_SUPPORTED`] — a compile-time constant so an
/// unparsable floor string can never widen the range.
const NIRI_MIN_SUPPORTED_TUPLE: (u32, u32) = (25, 5);
static NIRI_MUTATION: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

const NIRI_CANDIDATES: [&str; 4] = [
    "/usr/bin/niri",
    "/usr/local/bin/niri",
    "/run/current-system/sw/bin/niri",
    "/nix/var/nix/profiles/default/bin/niri",
];

pub async fn handle(action: &Action, ctx: &ModuleCtx<'_>) -> Result<serde_json::Value, ApiError> {
    match action {
        Action::GetNiriConfig => Ok(json!(read_view(ctx).await?)),
        Action::ValidateNiriConfig { proposed } => {
            let path = config_path(ctx)?;
            let content = match proposed {
                Some(content) => content.clone(),
                None => read_raw(ctx.as_user, &path).await?.1,
            };
            let (valid, stdout, stderr) = exec::run_as_user_validate(
                ctx.as_user,
                &path.to_string_lossy(),
                niri_path()?,
                &content,
            )
            .await?;
            let errors = if valid {
                Vec::new()
            } else {
                validate_errors::parse_validate_errors(&stderr, &content)
            };
            Ok(json!({"valid": valid, "stdout": stdout, "stderr": stderr, "errors": errors}))
        }
        Action::SetNiriSection { section, value } => {
            let _guard = NIRI_MUTATION.lock().await;
            let path = config_path(ctx)?;
            let current = read_raw(ctx.as_user, &path).await?.1;
            let mut doc: KdlDocument = current
                .parse()
                .map_err(|_| ApiError::invalid("niri config could not be parsed"))?;
            kdl::apply_section(&mut doc, section, value)?;
            let updated = doc.to_string();
            if updated.len() > MAX_CONFIG {
                return Err(ApiError::invalid("niri config would exceed the size limit"));
            }
            let write_path = resolve_write_path(&path)?;
            exec::run_as_user_write(
                ctx.as_user,
                &write_path.to_string_lossy(),
                niri_path()?,
                &updated,
            )
            .await?;
            Ok(json!(read_view(ctx).await?))
        }
        _ => Err(ApiError::invalid("action is not valid for module niri")),
    }
}

async fn read_view(ctx: &ModuleCtx<'_>) -> Result<NiriConfigView, ApiError> {
    let path = config_path(ctx)?;
    let (exists, content) = read_raw(ctx.as_user, &path).await?;
    let values = if exists {
        content
            .parse::<KdlDocument>()
            .map_or_else(|_| NiriValues::default(), |doc| kdl::parse_values(&doc))
    } else {
        NiriValues::default()
    };
    let version = match niri_path() {
        Ok(niri) => {
            exec::run_as_user_argv(ctx.as_user, niri, &["--version"], Duration::from_secs(10))
                .await
                .ok()
                .map(|out| out.stdout.trim().to_string())
                .filter(|version| !version.is_empty())
        }
        Err(_) => None,
    };
    let version_parsed = version.as_deref().and_then(parse_niri_version);
    let version_supported = version_parsed
        .as_ref()
        .map(|(_, tuple)| *tuple >= min_supported_tuple());
    Ok(NiriConfigView {
        path: path.display().to_string(),
        exists,
        content,
        version,
        version_parsed: version_parsed.map(|(raw, _)| raw),
        version_supported,
        min_supported: NIRI_MIN_SUPPORTED.to_string(),
        values,
    })
}

/// Extract `major.minor[.patch]` from `niri --version` output such as
/// `niri 25.11 (unknown commit)` or `niri 0.1.9`. Returns the raw version
/// token plus the `(major, minor)` pair used for range comparison.
fn parse_niri_version(output: &str) -> Option<(String, (u32, u32))> {
    output.split_whitespace().find_map(|token| {
        let trimmed = token.trim_start_matches('v');
        if trimmed.is_empty()
            || !trimmed
                .bytes()
                .all(|byte| byte.is_ascii_digit() || byte == b'.')
        {
            return None;
        }
        let mut parts = trimmed.split('.');
        let major: u32 = parts.next()?.parse().ok()?;
        let minor: u32 = parts.next()?.parse().ok()?;
        if let Some(patch) = parts.next() {
            patch.parse::<u32>().ok()?;
        }
        if parts.next().is_some() {
            return None;
        }
        Some((trimmed.to_string(), (major, minor)))
    })
}

fn min_supported_tuple() -> (u32, u32) {
    NIRI_MIN_SUPPORTED_TUPLE
}

/// Returns `(exists, content)`. The first byte is a marker emitted by the
/// user-side reader so callers can distinguish a missing file from an empty one.
async fn read_raw(user: &str, path: &Path) -> Result<(bool, String), ApiError> {
    let outcome = exec::run_as_user_read_file(user, &path.to_string_lossy()).await?;
    if outcome.stdout.len() > MAX_CONFIG + 1 {
        return Err(ApiError::invalid("niri config is too large"));
    }
    let mut chars = outcome.stdout.chars();
    match chars.next() {
        Some('1') => Ok((true, chars.collect())),
        Some('0') if chars.next().is_none() => Ok((false, String::new())),
        _ => Err(ApiError::internal("unexpected target-user read response")),
    }
}

fn resolve_write_path(path: &Path) -> Result<PathBuf, ApiError> {
    let meta = match std::fs::symlink_metadata(path) {
        Ok(meta) => meta,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(path.to_path_buf()),
        Err(error) => {
            return Err(ApiError::internal(format!(
                "inspect niri config path: {error}"
            )))
        }
    };
    if !meta.file_type().is_symlink() {
        return Ok(path.to_path_buf());
    }
    let target = std::fs::canonicalize(path)
        .map_err(|_| ApiError::invalid("niri config symlink target is missing"))?;
    let target_meta = std::fs::metadata(&target)
        .map_err(|_| ApiError::invalid("niri config symlink target is unreadable"))?;
    if !target_meta.is_file() {
        return Err(ApiError::invalid(
            "niri config symlink target is not a file",
        ));
    }
    Ok(target)
}

fn config_path(ctx: &ModuleCtx<'_>) -> Result<PathBuf, ApiError> {
    if let Some(path) = std::env::var_os("URSINE_NIRI_CONFIG") {
        return Ok(PathBuf::from(path));
    }
    Ok(home_for_user(ctx.as_user)?.join(".config/niri/config.kdl"))
}

fn home_for_user(user: &str) -> Result<PathBuf, ApiError> {
    let raw = std::fs::read_to_string("/etc/passwd")
        .map_err(|e| ApiError::internal(format!("read /etc/passwd: {e}")))?;
    let home = raw.lines().find_map(|line| {
        let mut fields = line.split(':');
        if fields.next()? != user {
            return None;
        }
        let _password = fields.next()?;
        let _uid = fields.next()?;
        let _gid = fields.next()?;
        let _gecos = fields.next()?;
        fields.next().map(PathBuf::from)
    });
    let home = home.ok_or_else(|| {
        ApiError::new(
            ursine_api::ErrorCode::NotFound,
            format!("no password entry for {user}"),
        )
    })?;
    if !home.is_absolute() {
        return Err(ApiError::invalid("user home is not absolute"));
    }
    Ok(home)
}

fn niri_path() -> Result<&'static str, ApiError> {
    NIRI_CANDIDATES
        .iter()
        .find(|path| Path::new(path).is_file())
        .copied()
        .ok_or_else(|| ApiError::unavailable("niri not installed", "apk add niri"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_calver_version_output() {
        assert_eq!(
            parse_niri_version("niri 25.11 (unknown commit)"),
            Some(("25.11".to_string(), (25, 11)))
        );
        assert_eq!(
            parse_niri_version("niri 0.1.9"),
            Some(("0.1.9".to_string(), (0, 1)))
        );
        assert_eq!(
            parse_niri_version("25.05"),
            Some(("25.05".to_string(), (25, 5)))
        );
    }

    #[test]
    fn rejects_non_version_output() {
        assert_eq!(parse_niri_version(""), None);
        assert_eq!(parse_niri_version("niri not installed"), None);
        assert_eq!(parse_niri_version("25.11.1.4"), None);
        assert_eq!(parse_niri_version("v25.11-beta"), None);
    }

    #[test]
    fn range_compares_calver_tuples() {
        let min = min_supported_tuple();
        assert_eq!(min, (25, 5));
        assert!((25, 11) >= min);
        assert!((26, 1) >= min);
        assert!((24, 9) < min);
        assert!((0, 1) < min);
    }

    #[test]
    fn min_constant_matches_its_version_string() {
        assert_eq!(
            parse_niri_version(NIRI_MIN_SUPPORTED).map(|(_, tuple)| tuple),
            Some(NIRI_MIN_SUPPORTED_TUPLE)
        );
    }
}
