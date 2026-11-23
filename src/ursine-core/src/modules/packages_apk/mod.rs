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

pub async fn handle(action: &Action, ctx: &ModuleCtx<'_>) -> Result<serde_json::Value, ApiError> {
    match action {
        Action::ListApk => Ok(json!({"packages": installed().await?})),
        Action::SearchApk { query } => Ok(json!({"packages": search(query).await?})),
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
            let copies = crate::backup_configs::copies_from(&ctx.config);
            let path = repositories_path();
            let content = read_repositories(path)?;
            match remove_repository(&content, url) {
                Ok(updated) => write_repositories(path, &updated, copies).await?,
                Err(not_found) => {
                    // Lines in repositories.d are rewritten in their
                    // own fragment file, never in the main file.
                    let mut removed = false;
                    for fragment in fragment_paths() {
                        let Ok(content) = std::fs::read_to_string(&fragment) else {
                            continue;
                        };
                        if content.len() > 64 * 1024 {
                            continue;
                        }
                        let Ok(updated) = remove_repository(&content, url) else {
                            continue;
                        };
                        let fragment = fragment.to_string_lossy().into_owned();
                        write_repositories(&fragment, &updated, copies).await?;
                        removed = true;
                        break;
                    }
                    if !removed {
                        return Err(not_found);
                    }
                }
            }
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
            // The installed check runs inline so an absent package is an
            // answer, not an install: the action upgrades, it never adds.
            let args = upgrade_plan(package, is_installed(package).await?)?;
            start_job(ctx, "upgrade_apk_package", &args, false)
        }
        Action::RemoveApk { package } => {
            validate_package(package)?;
            start_job(ctx, "remove_apk", &["del", package], true)
        }
        Action::ApkRemoveRegex { pattern, dry_run } => {
            start_regex_job(ctx, pattern, *dry_run, "del", "remove_regex", true).await
        }
        Action::ApkUpgradeRegex { pattern, dry_run } => {
            start_regex_job(ctx, pattern, *dry_run, "upgrade", "upgrade_regex", false).await
        }
        Action::UpgradeApk => start_job(ctx, "upgrade_apk", &["upgrade"], false),
        Action::ListApkUpdates { refresh } => {
            // A poll must not write: `apk update` rewrites the index cache
            // and needs the network, so it runs only when the caller asks
            // for a fresh index. Without it the diff is answered from the
            // cache the last refresh (or repository change) left behind —
            // the automatic check stays read-only, the explicit "Check for
            // updates" button passes `refresh = true`.
            Ok(json!({"updates": updates(*refresh).await?}))
        }
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
/// Fragment files (`FeralOS` splits repository definitions here); they
/// join the LIST and can hold lines that Remove rewrites in place.
const REPOSITORIES_DIR: &str = "/etc/apk/repositories.d";

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
    let mut all = repositories_from(&read_repositories(repositories_path())?);
    for path in fragment_paths() {
        let Ok(content) = std::fs::read_to_string(&path) else {
            continue;
        };
        if content.len() > 64 * 1024 {
            continue;
        }
        all.extend(repositories_from(&content));
    }
    Ok(all)
}

/// Sorted regular files under `/etc/apk/repositories.d` (a missing
/// directory simply yields none).
fn fragment_paths() -> Vec<std::path::PathBuf> {
    let Ok(entries) = std::fs::read_dir(REPOSITORIES_DIR) else {
        return Vec::new();
    };
    let mut paths: Vec<std::path::PathBuf> = entries
        .flatten()
        .map(|entry| entry.path())
        // Real files only: the atomic rewrite replaces the path, so a
        // symlinked fragment must not be listed (its removal could not
        // be performed in place).
        .filter(|path| path.is_file() && !path.is_symlink())
        .collect();
    paths.sort();
    paths
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
            let label = tag.as_deref().map_or_else(
                || origin_label(&url, None).unwrap_or_else(|| url.clone()),
                |tag| format!("@{tag}"),
            );
            Some(ApkRepository { url, label, tag })
        })
        .collect()
}

/// Add the line, or update the tag of an existing URL (`None`
/// unmasks). Comments and unrelated lines are preserved verbatim.
fn upsert_repository(content: &str, url: &str, tag: Option<&str>) -> String {
    let mut out: Vec<String> = Vec::new();
    let mut replaced = false;
    for line in content.lines() {
        if repo_line_matches(line, url) {
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
    finish_repository(&out)
}

/// True when `line` configures `url` (blank lines and comments never do).
fn repo_line_matches(line: &str, url: &str) -> bool {
    let trimmed = line.trim();
    !trimmed.is_empty() && !trimmed.starts_with('#') && split_repo_line(trimmed).1 == url
}

/// Rebuild the file text; every repositories file ends with one newline.
fn finish_repository(lines: &[String]) -> String {
    let mut text = lines.join("\n");
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
        if repo_line_matches(line, url) {
            removed = true;
        } else {
            out.push(line.to_string());
        }
    }
    if !removed {
        return Err(ApiError::invalid("repository is not configured"));
    }
    Ok(finish_repository(&out))
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
    // ONE `apk policy <names…>` call answers "which repository did this
    // installed version come from" for every package (the installed
    // block lists `lib/apk/db/installed` next to the repo URL).
    let name_list: Vec<&str> = names.lines().filter(|line| !line.is_empty()).collect();
    let policy = if name_list.is_empty() {
        String::new()
    } else {
        let mut args = vec!["policy"];
        args.extend(name_list.iter().copied());
        run(&args).await.unwrap_or_default()
    };
    let origins = parse_policy_origins(&policy);
    // Descriptions live in the apk database (`T:` lines) — one read.
    let descriptions = read_apk_descriptions();
    // Mask tags + display labels come from the repositories file
    // (the same parser the repositories list action uses).
    let repositories = repositories().unwrap_or_default();
    let mut packages = parse_installed(&versioned, &names, &protected_names()?);
    for package in &mut packages {
        package.origin = origins.get(&package.name).cloned();
        package.description = descriptions.get(&package.name).cloned();
        package.origin_label = package.origin.as_deref().and_then(|url| {
            let tag = repositories
                .iter()
                .find(|repo| repo.url == url)
                .and_then(|repo| repo.tag.as_deref());
            origin_label(url, tag)
        });
    }
    Ok(packages)
}

/// Display prefix for an origin URL: the last two path segments
/// (`v3.24 main`), prefixed with the repository's mask tag (`@edge`)
/// when the repositories file has one for this URL.
fn origin_label(url: &str, tag: Option<&str>) -> Option<String> {
    let after_scheme = url.split_once("://").map_or(url, |(_, rest)| rest);
    let path = after_scheme.split_once('/').map_or("", |(_, rest)| rest);
    // Never let a query/fragment ride into the display label.
    let path = path.split(['?', '#']).next().unwrap_or(path);
    let segments: Vec<&str> = path.split('/').filter(|part| !part.is_empty()).collect();
    let mut reversed = segments.iter().rev();
    let repo = match (reversed.next(), reversed.next()) {
        (Some(last), Some(previous)) => format!("{previous} {last}"),
        (Some(single), None) => (*single).to_string(),
        (None, _) => String::new(),
    };
    match (tag, repo.is_empty()) {
        (Some(tag), true) => Some(format!("@{tag}")),
        (Some(tag), false) => Some(format!("@{tag} {repo}")),
        (None, false) => Some(repo),
        (None, true) => None,
    }
}

/// `/lib/apk/db/installed` → package → description (`T:` field).
fn read_apk_descriptions() -> std::collections::HashMap<String, String> {
    let mut map = std::collections::HashMap::new();
    let Ok(content) = std::fs::read_to_string("/lib/apk/db/installed") else {
        return map;
    };
    let mut current: Option<String> = None;
    for line in content.lines() {
        if let Some(name) = line.strip_prefix("P:") {
            current = Some(name.to_string());
        } else if let Some(description) = line.strip_prefix("T:") {
            if let Some(name) = &current {
                map.entry(name.clone())
                    .or_insert_with(|| description.to_string());
            }
        }
    }
    map
}

/// `apk policy <names…>` → package → repository URL (https).
/// Only the version block that contains `lib/apk/db/installed` counts,
/// and only an http(s) URL inside that block is an origin (kept in
/// full — the frontends render it as the repository link).
fn parse_policy_origins(output: &str) -> std::collections::HashMap<String, String> {
    let mut map = std::collections::HashMap::new();
    let mut current: Option<String> = None;
    let mut installed_version = false;
    for line in output.lines() {
        if !line.starts_with(char::is_whitespace) {
            if let Some(name) = line.strip_suffix(" policy:") {
                current = Some(name.to_string());
                installed_version = false;
            }
            continue;
        }
        let trimmed = line.trim();
        if trimmed == "lib/apk/db/installed" {
            installed_version = true;
        } else if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
            if installed_version {
                if let Some(name) = &current {
                    // Full https URL: the GUI detail shows it as the
                    // repository link (flatpak parity).
                    map.entry(name.clone())
                        .or_insert_with(|| trimmed.to_string());
                }
            }
        } else if trimmed.ends_with(':') && !trimmed.contains(' ') {
            // A different version header: the installed flag belongs to
            // the previous block only.
            installed_version = false;
        }
    }
    map
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
    // Set lookups keep the parse linear: the old per-line scan formatted
    // `"{name}-"` for EVERY installed name (millions of allocations on
    // big systems — the UI freeze the user hit).
    let name_set: std::collections::HashSet<&str> =
        names.lines().filter(|line| !line.is_empty()).collect();
    let protected: std::collections::HashSet<&str> = protected.iter().map(String::as_str).collect();
    let mut rows = Vec::new();
    for line in versioned.lines().filter(|line| !line.is_empty()) {
        // Fast path: the same `name-version` split the search parser
        // uses, validated against the installed set.
        let (split_name, split_version) = split_id(line);
        let (name, version) = if name_set.contains(split_name.as_str()) {
            (split_name, split_version)
        } else {
            // Fallback: longest installed name prefixing the line —
            // byte comparisons only, no per-candidate formatting.
            name_set
                .iter()
                .filter(|name| {
                    line.starts_with(*name) && line.as_bytes().get(name.len()) == Some(&b'-')
                })
                .max_by_key(|name| name.len())
                .copied()
                .map_or_else(
                    // Legacy behaviour: the whole line becomes the name.
                    || (line.to_string(), String::new()),
                    |name| {
                        (
                            name.to_string(),
                            line.get(name.len() + 1..).unwrap_or_default().to_string(),
                        )
                    },
                )
        };
        let is_protected = protected.contains(name.as_str());
        rows.push(ApkPackage {
            id: line.to_string(),
            name,
            version,
            description: None,
            protected: is_protected,
            origin: None,
            origin_label: None,
        });
    }
    rows
}

/// The installed-versus-index diff, after the optional index refresh.
///
/// Both steps run through the quoted-value runner: the `<` separator is
/// outside the strict atom charset the generic runner enforces (no shell is
/// involved, nothing interpolates the argument).
async fn updates(refresh: bool) -> Result<Vec<ApkUpdate>, ApiError> {
    let program = apk_path()?;
    let mut output = String::new();
    for args in update_check_args(refresh) {
        output = exec::run_root_program_value(program, args, TIMEOUT)
            .await?
            .stdout;
    }
    Ok(parse_updates(&output))
}

/// The argv of one update check, in order: the index refresh only when the
/// caller asked for it, then the installed-versus-index diff.
///
/// Pure so both entries of #3 are assertable without an `apk` binary: a poll
/// (`refresh = false`) must produce exactly one step and never `update`.
fn update_check_args(refresh: bool) -> Vec<&'static [&'static str]> {
    let mut steps: Vec<&'static [&'static str]> = Vec::new();
    if refresh {
        steps.push(&["update"]);
    }
    steps.push(&["version", "-l", "<"]);
    steps
}

/// What an `upgrade_apk_package` request runs, or the answer it gives.
///
/// `apk add --upgrade <pkg>` is deliberately not used: `add` re-solves the
/// whole world file, so an installed package the world does not name is
/// removed as a side effect (observed: nine packages gone after an unrelated
/// `apk add`). `apk upgrade <pkg>` upgrades exactly the named installed
/// package in place and never removes another one; it is also the verb the
/// regex upgrade action already uses.
fn upgrade_plan(package: &str, installed: bool) -> Result<Vec<&str>, ApiError> {
    if !installed {
        return Err(ApiError::new(
            ursine_api::ErrorCode::NotFound,
            format!("apk package {package} is not installed"),
        )
        .with_fix(format!(
            "install it with install_apk package={package}, or look up the name with search_apk"
        )));
    }
    Ok(vec!["upgrade", package])
}

/// Whether `apk info -e <pkg>` reports the package installed. apk exits 1 for
/// "not installed", which is an answer; any other failure is the tool itself
/// and is surfaced instead of being read as "absent".
async fn is_installed(package: &str) -> Result<bool, ApiError> {
    let program = apk_path()?;
    let outcome =
        exec::run_root_program_tolerant(program, &["info", "-e", package], TIMEOUT, None).await?;
    match outcome.code {
        0 => Ok(true),
        1 => Ok(false),
        code => Err(ApiError::unavailable(
            format!(
                "apk info -e {package} exited with {code}: {}",
                outcome.stderr.trim()
            ),
            "retry once; if apk keeps failing, run `apk update` on the machine and check the repositories",
        )),
    }
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
        .collect()
}

async fn search(query: &str) -> Result<Vec<ApkPackage>, ApiError> {
    let query = exec::validate_search_query(query, "apk")?;
    let output = run(&["search", "-v", query]).await?;
    let mut rows = Vec::new();
    for line in output.lines().filter(|line| !line.is_empty()) {
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
            origin: None,
            origin_label: None,
        });
    }
    Ok(rows)
}

/// Both regex actions pass the matched names to one job: a dry run answers
/// with the names, an empty set answers with empty output.
async fn start_regex_job(
    ctx: &ModuleCtx<'_>,
    pattern: &str,
    dry_run: bool,
    verb: &str,
    action: &'static str,
    destructive: bool,
) -> Result<serde_json::Value, ApiError> {
    let names = matched_names(pattern).await?;
    if let Some(answer) = exec::regex_noop(dry_run, &names) {
        return Ok(answer);
    }
    let mut args: Vec<&str> = vec![verb];
    args.extend(names.iter().map(String::as_str));
    start_job(ctx, action, &args, destructive)
}

/// Installed package names matching `pattern` (validate + compile once).
async fn matched_names(pattern: &str) -> Result<Vec<String>, ApiError> {
    let packages = installed().await?;
    matching_names(pattern, packages)
}

/// Validate + compile once, then keep the installed names the regex
/// matches (pure — the test suite drives it directly).
fn matching_names(pattern: &str, packages: Vec<ApkPackage>) -> Result<Vec<String>, ApiError> {
    let re = exec::compile_regex(pattern)?;
    Ok(packages
        .into_iter()
        .filter(|package| re.is_match(&package.name))
        .map(|package| package.name)
        .collect())
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
    // Complete output: installed lists must never truncate (user
    // decision: unlimited lists).
    exec::run_root_program_complete(program, args, TIMEOUT)
        .await
        .map(|outcome| outcome.stdout)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn origin_label_shows_repo_path_and_mask() {
        assert_eq!(
            origin_label("https://dl-cdn.alpinelinux.org/alpine/v3.24/main", None),
            Some("v3.24 main".to_string())
        );
        assert_eq!(
            origin_label(
                "https://dl-cdn.alpinelinux.org/alpine/v3.24/testing",
                Some("edge")
            ),
            Some("@edge v3.24 testing".to_string())
        );
        assert_eq!(
            origin_label("https://example.com/repo", None),
            Some("repo".to_string())
        );
        assert_eq!(origin_label("https://example.com", None), None);
        assert_eq!(
            origin_label("https://example.com", Some("edge")),
            Some("@edge".to_string())
        );
        assert_eq!(
            origin_label("https://mirror.example/alpine/v3.24/main?foo=1#x", None),
            Some("v3.24 main".to_string())
        );
    }

    #[test]
    fn parse_installed_keeps_every_row() {
        // The old300-row cap silently dropped the tail of big systems.
        let names: Vec<String> = (0..301).map(|i| format!("a{i}")).collect();
        let versioned: Vec<String> = (0..301).map(|i| format!("a{i}-1.0")).collect();
        let rows = parse_installed(&versioned.join("\n"), &names.join("\n"), &[]);
        assert_eq!(rows.len(), 301);
        assert_eq!(rows.last().map(|row| row.name.as_str()), Some("a300"));
    }

    #[test]
    fn regex_matches_installed_names() {
        let package = |name: &str| ApkPackage {
            id: format!("{name}-1"),
            name: name.to_string(),
            version: "1".to_string(),
            description: None,
            protected: false,
            origin: None,
            origin_label: None,
        };
        let packages = vec![package("libfoo"), package("bash"), package("libbar")];
        assert_eq!(
            matching_names("^lib", packages.clone()).unwrap(),
            ["libfoo", "libbar"]
        );
        assert_eq!(matching_names("bash$", packages.clone()).unwrap(), ["bash"]);
        assert!(matching_names("^nothing$", packages.clone())
            .unwrap()
            .is_empty());
        // The shared validator rejects unsafe patterns before compiling.
        assert!(matching_names("bad'quote", packages).is_err());
        assert!(matching_names("bad;rm", vec![package("x")]).is_err());
    }

    #[test]
    fn parses_masked_repository_lines() {
        let parsed = repositories_from("@edge http://a/edge/main\nhttp://b/v3/main\n# note\n");
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].tag.as_deref(), Some("edge"));
        assert_eq!(parsed[0].url, "http://a/edge/main");
        assert_eq!(parsed[0].label, "@edge");
        assert_eq!(parsed[1].tag, None);
        assert_eq!(parsed[1].url, "http://b/v3/main");
        assert_eq!(parsed[1].label, "v3 main");
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
    fn update_check_refreshes_only_on_request() {
        // ISSUES.md #3: a poll of the read must not write the index cache.
        let cached = update_check_args(false);
        assert_eq!(cached, vec![&["version", "-l", "<"][..]]);
        assert!(!cached.iter().any(|args| args.contains(&"update")));
        let refreshed = update_check_args(true);
        assert_eq!(
            refreshed,
            vec![&["update"][..], &["version", "-l", "<"][..]]
        );
    }

    #[test]
    fn upgrade_plan_upgrades_installed_and_answers_not_found() {
        // ISSUES.md #30: `apk add` keeps the installed version and re-solves
        // the world file; the upgrade verb upgrades that one package.
        assert_eq!(upgrade_plan("niri", true).unwrap(), ["upgrade", "niri"]);
        assert!(!upgrade_plan("niri", true).unwrap().contains(&"add"));
        // An absent package is an answer, not an install.
        let absent = upgrade_plan("tree", false).unwrap_err();
        assert_eq!(absent.code, ursine_api::ErrorCode::NotFound);
        assert!(absent.message.contains("tree"), "{}", absent.message);
        assert!(
            absent
                .fix
                .as_deref()
                .is_some_and(|fix| fix.contains("install_apk")),
            "{absent:?}"
        );
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
