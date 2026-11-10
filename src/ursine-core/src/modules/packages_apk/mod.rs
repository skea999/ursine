//! packages.apk — Alpine package management through the system `apk` binary.
//!
//! Every command runs as root with an absolute trusted path and argv arguments
//! only. The GUI never parses shell output; it receives typed rows.

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use serde_json::json;
use ursine_api::{Action, ApiError, ApkPackage, ApkRepository, ApkUpdate, ModuleId};

use super::ModuleCtx;
use crate::jobs::JobSink;
use crate::modules::packages_nix::exec;

pub(crate) const APK_CANDIDATES: [&str; 3] = ["/sbin/apk", "/usr/sbin/apk", "/usr/bin/apk"];
const TIMEOUT: Duration = Duration::from_secs(300);
const MAX_ROWS: usize = 300;

pub async fn handle(action: &Action, ctx: &ModuleCtx<'_>) -> Result<serde_json::Value, ApiError> {
    match action {
        Action::ListApk => Ok(json!({"packages": installed().await?})),
        Action::SearchApk { query } => Ok(json!({"packages": search(query).await?})),
        Action::RefreshApk => Ok(json!({"output": run(&["update"]).await?})),
        Action::ListApkRepositories => Ok(json!({"repositories": repositories()?})),
        Action::AddApkRepository { url, tag } => {
            validate_repo_url(url)?;
            // An empty tag (CLI `tag=`) means "no tag": unmask.
            let tag = tag.clone().filter(|value| !value.is_empty());
            if let Some(tag) = &tag {
                validate_repo_tag(tag)?;
            }
            let path = repositories_path();
            let content = read_repositories(path)?;
            let updated = upsert_repository(&content, url, tag.as_deref());
            write_repositories(
                path,
                &updated,
                crate::backup_configs::copies_from(&ctx.config),
            )
            .await?;
            // Refresh indexes so the new (or re-tagged) repository is
            // live; a failure here means the config is saved but the
            // cache is stale — surface it, the user can Refresh.
            run(&["update"]).await?;
            Ok(json!({"repositories": repositories()?}))
        }
        Action::RemoveApkRepository { url } => {
            validate_repo_url(url)?;
            let path = repositories_path();
            let content = read_repositories(path)?;
            let updated = remove_repository(&content, url)?;
            write_repositories(
                path,
                &updated,
                crate::backup_configs::copies_from(&ctx.config),
            )
            .await?;
            run(&["update"]).await?;
            Ok(json!({"repositories": repositories()?}))
        }
        Action::PreviewApkInstall { package } => {
            validate_package(package)?;
            Ok(json!({"output": run(&["add", "--simulate", package]).await?}))
        }
        Action::InstallApk { package } => {
            validate_package(package)?;
            start_job(ctx, "install_apk", &["add", package], true)
        }
        Action::UpgradeApkPackage { package } => {
            validate_package(package)?;
            start_job(ctx, "upgrade_apk_package", &["add", package], true)
        }
        Action::RemoveApk { package } => {
            validate_package(package)?;
            start_job(ctx, "remove_apk", &["del", package], true)
        }
        Action::UpgradeApk => start_job(ctx, "upgrade_apk", &["upgrade"], false),
        Action::ListApkUpdates => Ok(json!({"updates": updates().await?})),
        _ => Err(ApiError::invalid(
            "action is not valid for module packages_apk",
        )),
    }
}

/// Start a background `apk` job; args are validated by the caller. The job
/// streams the exact terminal output into the job log (no `--no-progress`:
/// progress text is what the user asked to see).
///
/// `mutates_world` marks `apk add`/`del` (they rewrite `/etc/apk/world`):
/// the world file rotates first so the previous pin set is recoverable.
fn start_job(
    ctx: &ModuleCtx<'_>,
    action: &'static str,
    args: &[&str],
    mutates_world: bool,
) -> Result<serde_json::Value, ApiError> {
    let program = apk_path()?;
    if mutates_world {
        // World rotation runs inline: it is two renames plus a small
        // copy — cheaper than the scheduling overhead of a task. A job
        // that fails after this point leaves one duplicate slot (the
        // chain self-heals on the next mutation); fail-closed beats a
        // lost pin set on a successful mutation.
        crate::backup_configs::rotate(
            Path::new("/etc/apk/world"),
            crate::backup_configs::copies_from(&ctx.config),
            None,
        )?;
    }
    let args: Vec<String> = args.iter().copied().map(str::to_string).collect();
    let started = ctx.jobs.start(
        ModuleId::PackagesApk,
        action,
        ctx.principal,
        None,
        move |sink: Arc<JobSink>| async move {
            let refs: Vec<&str> = args.iter().map(String::as_str).collect();
            exec::run_root_program_log(program, &refs, TIMEOUT, sink).await
        },
    )?;
    Ok(json!(started))
}

const REPOSITORIES_PATH: &str = "/etc/apk/repositories";

fn repositories_path() -> &'static str {
    REPOSITORIES_PATH
}

fn read_repositories(path: &str) -> Result<String, ApiError> {
    let content = std::fs::read_to_string(path)
        .map_err(|e| ApiError::internal(format!("read {path}: {e}")))?;
    if content.len() > 64 * 1024 {
        return Err(ApiError::internal(
            "repositories file is larger than 64 KiB",
        ));
    }
    Ok(content)
}

/// Split one repositories line into its optional `@tag` and the rest
/// (URL plus any component tokens).
fn split_repo_line(line: &str) -> (Option<String>, String) {
    let trimmed = line.trim();
    if let Some(rest) = trimmed.strip_prefix('@') {
        let (tag, rest) = match rest.split_once(char::is_whitespace) {
            Some((tag, rest)) => (tag.to_string(), rest.trim().to_string()),
            None => (rest.to_string(), String::new()),
        };
        return (Some(tag), rest);
    }
    (None, trimmed.to_string())
}

fn repositories() -> Result<Vec<ApkRepository>, ApiError> {
    Ok(repositories_from(&read_repositories(repositories_path())?))
}

fn repositories_from(content: &str) -> Vec<ApkRepository> {
    content
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .filter_map(|line| {
            let (tag, url) = split_repo_line(line);
            if url.is_empty() {
                return None;
            }
            Some(ApkRepository { url, tag })
        })
        .collect()
}

/// Add the line, or update the tag of an existing URL (`None`
/// unmasks). Comments and unrelated lines are preserved verbatim.
fn upsert_repository(content: &str, url: &str, tag: Option<&str>) -> String {
    let mut out: Vec<String> = Vec::new();
    let mut replaced = false;
    for line in content.lines() {
        let trimmed = line.trim();
        let matched =
            !trimmed.is_empty() && !trimmed.starts_with('#') && split_repo_line(trimmed).1 == url;
        if matched {
            // Replace the first match, collapse later duplicates.
            if !replaced {
                out.push(render_repo_line(url, tag));
                replaced = true;
            }
        } else {
            out.push(line.to_string());
        }
    }
    if !replaced {
        out.push(render_repo_line(url, tag));
    }
    let mut text = out.join("\n");
    text.push('\n');
    text
}

fn render_repo_line(url: &str, tag: Option<&str>) -> String {
    tag.map_or_else(|| url.to_string(), |tag| format!("@{tag} {url}"))
}

/// Drop the line for `url`; errors when it is not configured.
fn remove_repository(content: &str, url: &str) -> Result<String, ApiError> {
    let mut out: Vec<String> = Vec::new();
    let mut removed = false;
    for line in content.lines() {
        let trimmed = line.trim();
        let matched =
            !trimmed.is_empty() && !trimmed.starts_with('#') && split_repo_line(trimmed).1 == url;
        if matched {
            removed = true;
        } else {
            out.push(line.to_string());
        }
    }
    if !removed {
        return Err(ApiError::invalid("repository is not configured"));
    }
    let mut text = out.join("\n");
    text.push('\n');
    Ok(text)
}

async fn write_repositories(path: &str, content: &str, copies: usize) -> Result<(), ApiError> {
    exec::run_as_user_atomic_write("root", path, content, copies)
        .await
        .map_err(|error| ApiError::new(error.code, error.message))?;
    Ok(())
}

/// Repository URLs reach the file as one line: absolute paths or
/// http(s), no whitespace/control, no leading `-`.
fn validate_repo_url(url: &str) -> Result<(), ApiError> {
    let ok = !url.is_empty()
        && url.len() <= 512
        && !url.starts_with('-')
        && (url.starts_with("http://") || url.starts_with("https://") || url.starts_with('/'))
        && url.chars().all(|c| !c.is_whitespace() && !c.is_control());
    if ok {
        Ok(())
    } else {
        Err(ApiError::invalid(
            "invalid repository URL (http(s):// or absolute path, no spaces)",
        ))
    }
}

fn validate_repo_tag(tag: &str) -> Result<(), ApiError> {
    if tag.is_empty()
        || tag.len() > 32
        || !tag
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
    {
        return Err(ApiError::invalid(
            "invalid repository tag (letters, digits, . _ -)",
        ));
    }
    Ok(())
}

async fn installed() -> Result<Vec<ApkPackage>, ApiError> {
    let versioned = run(&["info", "-v"]).await?;
    let names = run(&["info"]).await?;
    Ok(parse_installed(&versioned, &names, &protected_names()?))
}

fn protected_names() -> Result<Vec<String>, ApiError> {
    let content = std::fs::read_to_string("/etc/apk/world")
        .map_err(|e| ApiError::internal(format!("read /etc/apk/world: {e}")))?;
    Ok(content
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(|line| {
            line.split(['=', '<', '>', '~'])
                .next()
                .unwrap_or(line)
                .to_string()
        })
        .collect())
}

fn parse_installed(versioned: &str, names: &str, protected: &[String]) -> Vec<ApkPackage> {
    let names: Vec<&str> = names.lines().filter(|line| !line.is_empty()).collect();
    let mut rows = Vec::new();
    for line in versioned
        .lines()
        .filter(|line| !line.is_empty())
        .take(MAX_ROWS)
    {
        let name = names
            .iter()
            .filter(|name| line.starts_with(&format!("{name}-")))
            .max_by_key(|name| name.len())
            .copied()
            .unwrap_or(line);
        let version = line
            .strip_prefix(name)
            .and_then(|rest| rest.strip_prefix('-'))
            .unwrap_or_default()
            .to_string();
        rows.push(ApkPackage {
            id: line.to_string(),
            name: name.to_string(),
            version,
            description: None,
            protected: protected.iter().any(|entry| entry == name),
        });
    }
    rows
}

async fn updates() -> Result<Vec<ApkUpdate>, ApiError> {
    let output = run(&["version", "-l", "<"]).await?;
    Ok(parse_updates(&output))
}

fn parse_updates(output: &str) -> Vec<ApkUpdate> {
    output
        .lines()
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            let installed = fields.next()?;
            if fields.next()? != "<" {
                return None;
            }
            let available = fields.next()?;
            let (name, installed_version) = split_id(installed);
            let available_version = available
                .strip_prefix(&format!("{name}-"))
                .unwrap_or(available)
                .to_string();
            Some(ApkUpdate {
                name,
                installed: installed_version,
                available: available_version,
            })
        })
        .take(MAX_ROWS)
        .collect()
}

async fn search(query: &str) -> Result<Vec<ApkPackage>, ApiError> {
    let query = query.trim();
    if query.is_empty()
        || query.len() > 128
        || query.starts_with('-')
        || !query
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '+' | '-'))
    {
        return Err(ApiError::invalid("invalid apk search query"));
    }
    let output = run(&["search", "-v", query]).await?;
    let mut rows = Vec::new();
    for line in output
        .lines()
        .filter(|line| !line.is_empty())
        .take(MAX_ROWS)
    {
        let (id, description) = line
            .split_once(" - ")
            .map_or((line, None), |(id, description)| {
                (id, Some(description.to_string()))
            });
        let (name, version) = split_id(id);
        rows.push(ApkPackage {
            id: id.to_string(),
            name,
            version,
            description,
            protected: false,
        });
    }
    Ok(rows)
}

fn split_id(id: &str) -> (String, String) {
    for (index, character) in id.char_indices() {
        if character != '-' {
            continue;
        }
        let Some(rest) = id.get(index + 1..) else {
            continue;
        };
        if rest.chars().next().is_some_and(|c| c.is_ascii_digit()) {
            let Some(name) = id.get(..index) else {
                continue;
            };
            return (name.to_string(), rest.to_string());
        }
    }
    (id.to_string(), String::new())
}

fn validate_package(package: &str) -> Result<(), ApiError> {
    if package.is_empty()
        || package.len() > 256
        || package.starts_with('-')
        || !package
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '+' | '-'))
    {
        return Err(ApiError::invalid("invalid apk package name"));
    }
    Ok(())
}

fn apk_path() -> Result<&'static str, ApiError> {
    APK_CANDIDATES
        .iter()
        .find(|path| Path::new(path).is_file())
        .copied()
        .ok_or_else(|| {
            ApiError::unavailable("apk is not available", "boot a FeralOS/Alpine system")
        })
}

async fn run(args: &[&str]) -> Result<String, ApiError> {
    let program = apk_path()?;
    exec::run_root_program(program, args, TIMEOUT)
        .await
        .map(|outcome| outcome.stdout)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_masked_repository_lines() {
        let parsed = repositories_from("@edge http://a/edge/main\nhttp://b/v3/main\n# note\n");
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].tag.as_deref(), Some("edge"));
        assert_eq!(parsed[0].url, "http://a/edge/main");
        assert_eq!(parsed[1].tag, None);
        assert_eq!(parsed[1].url, "http://b/v3/main");
    }

    #[test]
    fn upsert_adds_updates_and_unmasks() {
        let base = "# keep\nhttp://a/main\n";
        let added = upsert_repository(base, "http://a/edge", Some("edge"));
        assert!(added.contains("# keep"), "{added}");
        assert!(added.contains("@edge http://a/edge"), "{added}");
        // Update the tag of the existing URL.
        let retagged = upsert_repository(&added, "http://a/edge", Some("beta"));
        assert!(retagged.contains("@beta http://a/edge"), "{retagged}");
        assert!(!retagged.contains("@edge"), "{retagged}");
        // Unmask: same URL, no tag.
        let unmasked = upsert_repository(&retagged, "http://a/edge", None);
        assert!(unmasked.contains("\nhttp://a/edge\n"), "{unmasked}");
        assert!(!unmasked.contains('@'), "{unmasked}");
        // Idempotent unmasked add.
        let again = upsert_repository(&unmasked, "http://a/edge", None);
        assert_eq!(again.matches("http://a/edge").count(), 1, "{again}");
    }

    #[test]
    fn remove_drops_only_the_matching_line() {
        let base = "# c\n@edge http://a/edge\nhttp://a/main\n";
        let out = remove_repository(base, "http://a/edge").unwrap();
        assert!(!out.contains("edge"), "{out}");
        assert!(out.contains("http://a/main"), "{out}");
        assert!(out.contains("# c"), "{out}");
        assert!(remove_repository(base, "http://nope").is_err());
    }

    #[test]
    fn validates_repository_inputs() {
        assert!(validate_repo_url("https://dl-cdn.alpinelinux.org/alpine/v3.20/main").is_ok());
        assert!(validate_repo_url("/media/usb/repo").is_ok());
        assert!(validate_repo_url("ftp://x").is_err());
        assert!(validate_repo_url("/has space").is_err());
        assert!(validate_repo_url("-rf").is_err());
        assert!(validate_repo_tag("edge").is_ok());
        assert!(validate_repo_tag("my-repo_1").is_ok());
        assert!(validate_repo_tag("bad tag").is_err());
        assert!(validate_repo_tag("").is_err());
    }

    #[test]
    fn validates_queries_and_packages() {
        assert!(validate_package("niri").is_ok());
        assert!(validate_package("niri-25.11-r2").is_ok());
        assert!(validate_package("-x").is_err());
        assert!(validate_package("a/b").is_err());
    }

    #[test]
    fn parses_installed_names_and_versions() {
        let rows = parse_installed(
            "niri-25.11-r2\nniri-portalsconf-25.11-r2\n",
            "niri\nniri-portalsconf\n",
            &["niri".to_string()],
        );
        assert_eq!(rows[0].name, "niri");
        assert_eq!(rows[0].version, "25.11-r2");
        assert_eq!(rows[1].name, "niri-portalsconf");
        assert!(rows[0].protected);
        assert!(!rows[1].protected);
    }

    #[test]
    fn parses_update_list() {
        let updates = parse_updates(concat!(
            "Installed:                                Available:\n",
            "niri-25.11-r2 < 25.11-r3\n",
            "apk-tools-3.0.0-r1 < apk-tools-3.0.1-r0\n",
        ));
        assert_eq!(updates.len(), 2);
        assert_eq!(updates[0].name, "niri");
        assert_eq!(updates[0].installed, "25.11-r2");
        assert_eq!(updates[0].available, "25.11-r3");
        assert_eq!(updates[1].name, "apk-tools");
        assert_eq!(updates[1].available, "3.0.1-r0");
    }

    #[test]
    fn parses_search_description() {
        let (id, description) = "niri-25.11-r2 - compositor".split_once(" - ").unwrap();
        assert_eq!(id, "niri-25.11-r2");
        assert_eq!(split_id(id), ("niri".into(), "25.11-r2".into()));
        assert_eq!(description, "compositor");
        assert_eq!(
            split_id("niri-portalsconf-25.11-r2"),
            ("niri-portalsconf".into(), "25.11-r2".into())
        );
    }
}
