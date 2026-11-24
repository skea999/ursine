//! packages.flatpak — Flatpak applications for system and user installations.
//!
//! System operations run as root; user operations run as the authenticated
//! target user through `su`. No shell is involved.

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use serde_json::json;
use ursine_api::{
    Action, ApiError, FlatpakInfo, FlatpakPackage, FlatpakPatternMatch, FlatpakRefKind,
    FlatpakRemote, ModuleId,
};

use super::ModuleCtx;
use crate::jobs::JobSink;
use crate::modules::packages_nix::exec;

const FLATPAK_CANDIDATES: [&str; 3] = [
    "/usr/bin/flatpak",
    "/usr/local/bin/flatpak",
    "/var/lib/flatpak/exports/bin/flatpak",
];
const TIMEOUT: Duration = Duration::from_mins(15);
/// Shared column order for `flatpak list` / `flatpak remote-ls` rows —
/// [`parse_list`] reads the fields positionally in exactly this order.
const LIST_COLUMNS: &str = "application,name,version,branch,origin,description";

pub async fn handle(action: &Action, ctx: &ModuleCtx<'_>) -> Result<serde_json::Value, ApiError> {
    match action {
        Action::ListFlatpak { system } => {
            let packages = list(*system, ctx.as_user).await?;
            // Best-effort orphan count for the Maintenance row: a
            // failing probe must never fail the list.
            let orphans = unused_count(*system, ctx.as_user).await;
            Ok(json!({"packages": packages, "orphans": orphans}))
        }
        Action::ListFlatpakRemotes { system } => {
            Ok(json!({"remotes": remotes(*system, ctx.as_user).await?}))
        }
        Action::ListFlatpakUpdates { system } => {
            Ok(json!({"updates": updates(*system, ctx.as_user).await?}))
        }
        Action::AddFlatpakRemote { name, url, system } => {
            validate_remote_name(name)?;
            validate_remote_url(url)?;
            backup_repo_config(
                *system,
                ctx.as_user,
                crate::backup_configs::copies_from(&ctx.config),
            )
            .await?;
            run(
                *system,
                ctx.as_user,
                &["remote-add", scope(*system), name, url],
            )
            .await?;
            Ok(json!({"remotes": remotes(Some(*system), ctx.as_user).await?}))
        }
        Action::RemoveFlatpakRemote { name, system } => {
            validate_remote_name(name)?;
            backup_repo_config(
                *system,
                ctx.as_user,
                crate::backup_configs::copies_from(&ctx.config),
            )
            .await?;
            run(
                *system,
                ctx.as_user,
                &["remote-delete", scope(*system), name],
            )
            .await?;
            Ok(json!({"remotes": remotes(Some(*system), ctx.as_user).await?}))
        }
        Action::SearchFlatpak { query, system } => Ok(json!({
            "packages": search(query, *system, ctx.as_user).await?,
        })),
        Action::InstallFlatpak {
            app_id,
            system,
            remote,
        } => {
            validate_app_id(app_id)?;
            let remote = validate_remote(remote.as_deref())?;
            let scope = scope(*system);
            let args = [
                "install",
                "-y",
                "--noninteractive",
                scope,
                remote.as_str(),
                app_id,
            ];
            start_job(ctx, "install_flatpak", *system, &args)
        }
        Action::GetFlatpakInfo {
            app_id,
            remote,
            system,
        } => {
            validate_app_id(app_id)?;
            let remote = validate_remote(remote.as_deref())?;
            let info = info(app_id, &remote, *system, ctx.as_user).await?;
            Ok(json!(info))
        }
        Action::RemoveFlatpak { app_id, system } => {
            validate_app_id(app_id)?;
            let scope = scope(*system);
            let args = ["uninstall", "-y", "--noninteractive", scope, app_id];
            start_job(ctx, "remove_flatpak", *system, &args)
        }
        Action::RemoveFlatpakOrphans { system } => {
            let scope = scope(*system);
            let args = ["uninstall", "--unused", "-y", "--noninteractive", scope];
            start_job(ctx, "remove_flatpak_orphans", *system, &args)
        }
        Action::FlatpakRemoveRegex {
            pattern,
            dry_run,
            system,
        } => start_regex_job(ctx, pattern, *dry_run, *system, "uninstall", "remove_regex").await,
        Action::FlatpakUpgradeRegex {
            pattern,
            dry_run,
            system,
        } => start_regex_job(ctx, pattern, *dry_run, *system, "update", "update_regex").await,
        Action::UpdateFlatpak { system } => {
            let scope = scope(*system);
            let args = ["update", "-y", "--noninteractive", scope];
            start_job(ctx, "update_flatpak", *system, &args)
        }
        Action::UpdateFlatpakApp { app_id, system } => {
            validate_app_id(app_id)?;
            let scope = scope(*system);
            let args = ["update", "-y", "--noninteractive", scope, app_id];
            start_job(ctx, "update_flatpak_app", *system, &args)
        }
        _ => Err(ApiError::invalid(
            "action is not valid for module packages_flatpak",
        )),
    }
}

/// Start a background flatpak job; args are validated by the caller. System
/// operations run as root, user operations as the target user.
fn start_job(
    ctx: &ModuleCtx<'_>,
    action: &'static str,
    system: bool,
    args: &[&str],
) -> Result<serde_json::Value, ApiError> {
    let program = flatpak_path()?;
    let user = ctx.as_user.to_string();
    let args: Vec<String> = args.iter().copied().map(str::to_string).collect();
    let started = ctx.jobs.start(
        ModuleId::PackagesFlatpak,
        action,
        ctx.principal,
        // System scope runs as root: never record a target user the action
        // did not honor (mirrors apk).
        (!system).then_some(ctx.as_user),
        move |sink: Arc<JobSink>| async move {
            let refs: Vec<&str> = args.iter().map(String::as_str).collect();
            if system {
                exec::run_root_program_log(program, &refs, TIMEOUT, sink).await
            } else {
                exec::run_as_user_argv_log(&user, program, &refs, TIMEOUT, sink).await
            }
        },
    )?;
    Ok(json!(started))
}

const fn scope(system: bool) -> &'static str {
    if system {
        "--system"
    } else {
        "--user"
    }
}

/// Both regex actions pass the matched refs to one job: a dry run answers
/// with the matched rows, an empty set answers with empty output.
async fn start_regex_job(
    ctx: &ModuleCtx<'_>,
    pattern: &str,
    dry_run: bool,
    system: bool,
    verb: &str,
    action: &'static str,
) -> Result<serde_json::Value, ApiError> {
    let matches = matched_flatpaks(pattern, system, ctx.as_user).await?;
    if dry_run {
        return Ok(pattern_preview(&matches));
    }
    let ids = matched_ids(&matches);
    if let Some(answer) = exec::regex_noop(false, &ids) {
        return Ok(answer);
    }
    let mut args: Vec<&str> = vec![verb, "-y", "--noninteractive", scope(system)];
    args.extend(ids.iter().map(String::as_str));
    start_job(ctx, action, system, &args)
}

/// One argv entry per id: the same id can be installed on several branches
/// and flatpak already acts on every ref of an id it is given.
fn matched_ids(matches: &[FlatpakPatternMatch]) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    matches
        .iter()
        .filter(|row| seen.insert(row.app_id.as_str()))
        .map(|row| row.app_id.clone())
        .collect()
}

/// Dry-run answer for both pattern actions: `would_change` keeps the bare ids
/// that clients already read, `matches` carries the rows the ids cannot
/// describe (kind, branch).
fn pattern_preview(matches: &[FlatpakPatternMatch]) -> serde_json::Value {
    json!({
        "dry_run": true,
        "would_change": matched_ids(matches),
        "matches": matches,
    })
}

/// Installed Flatpak refs matching `pattern` (id or human name).
async fn matched_flatpaks(
    pattern: &str,
    system: bool,
    user: &str,
) -> Result<Vec<FlatpakPatternMatch>, ApiError> {
    let packages = list(system, user).await?;
    matching_flatpaks(pattern, packages)
}

/// Validate + compile once, then keep the rows whose app id OR human
/// name the regex matches. Applications and runtimes both match, because
/// the installed list holds both and a pattern such as `Platform` is meant
/// for the runtime; the row keeps the kind so the preview can say so
/// (pure — the test suite drives it directly).
fn matching_flatpaks(
    pattern: &str,
    packages: Vec<FlatpakPackage>,
) -> Result<Vec<FlatpakPatternMatch>, ApiError> {
    let re = exec::compile_regex(pattern)?;
    Ok(packages
        .into_iter()
        .filter(|package| re.is_match(&package.app_id) || re.is_match(&package.name))
        .map(|package| FlatpakPatternMatch {
            app_id: package.app_id,
            name: package.name,
            branch: package.branch,
            kind: package.kind,
        })
        .collect())
}

async fn list(system: bool, user: &str) -> Result<Vec<FlatpakPackage>, ApiError> {
    // The `ref` column carries no app/runtime prefix, so the split needs
    // two queries; rows are tagged from the flag that produced them.
    // Both run together (same binary/scope) and fail closed together.
    let (apps, runtimes) = tokio::try_join!(
        list_kind(system, user, "--app", FlatpakRefKind::App, false),
        list_kind(system, user, "--runtime", FlatpakRefKind::Runtime, false),
    )?;
    let mut rows = apps;
    rows.extend(runtimes);
    Ok(rows)
}

/// Refs whose remote has a newer commit (`flatpak remote-ls --updates`).
/// Column order matches [`parse_list`], so rows share `FlatpakPackage`.
async fn updates(system: bool, user: &str) -> Result<Vec<FlatpakPackage>, ApiError> {
    let (apps, runtimes) = tokio::try_join!(
        list_kind(system, user, "--app", FlatpakRefKind::App, true),
        list_kind(system, user, "--runtime", FlatpakRefKind::Runtime, true),
    )?;
    let mut rows = apps;
    rows.extend(runtimes);
    Ok(rows)
}

/// One ref list: installed refs, or the refs with a newer remote commit
/// (`--updates`). Both share the column order used by [`parse_list`].
async fn list_kind(
    system: bool,
    user: &str,
    kind_flag: &str,
    kind: FlatpakRefKind,
    updates: bool,
) -> Result<Vec<FlatpakPackage>, ApiError> {
    let scope = scope(system);
    let columns = format!("--columns={LIST_COLUMNS}");
    let args: &[&str] = if updates {
        &["remote-ls", scope, kind_flag, "--updates", columns.as_str()]
    } else {
        &["list", columns.as_str(), kind_flag, scope]
    };
    let output = run(system, user, args).await?;
    Ok(parse_list(&output, system, kind))
}

async fn remotes(system: Option<bool>, user: &str) -> Result<Vec<FlatpakRemote>, ApiError> {
    let mut args = vec!["remotes", "--columns=name,url"];
    match system {
        Some(true) => args.push("--system"),
        Some(false) => args.push("--user"),
        None => {}
    }
    let output = match system {
        Some(false) => {
            exec::run_as_user_argv(user, flatpak_path()?, &args, TIMEOUT)
                .await?
                .stdout
        }
        _ => {
            exec::run_root_program(flatpak_path()?, &args, TIMEOUT)
                .await?
                .stdout
        }
    };
    Ok(output
        .lines()
        .filter_map(|line| {
            let mut fields = line.split('\t');
            Some(FlatpakRemote {
                name: fields.next()?.to_string(),
                url: fields.next()?.to_string(),
            })
        })
        .collect())
}

async fn search(query: &str, system: bool, user: &str) -> Result<Vec<FlatpakPackage>, ApiError> {
    let query = exec::validate_search_query(query, "flatpak")?;
    let scope = scope(system);
    let args = [
        "search",
        "--columns=application,name,version,description",
        scope,
        query,
    ];
    let output = run(system, user, &args).await?;
    Ok(parse_search(&output, system))
}

/// Parse tab rows in [`LIST_COLUMNS`] order (shared by list and updates).
fn parse_list(output: &str, system: bool, kind: FlatpakRefKind) -> Vec<FlatpakPackage> {
    let mut rows = Vec::new();
    for line in output.lines().filter(|line| !line.is_empty()) {
        let mut fields = line.split('\t');
        let (Some(app_id), Some(name), Some(version), Some(branch), Some(origin)) = (
            fields.next(),
            fields.next(),
            fields.next(),
            fields.next(),
            fields.next(),
        ) else {
            continue;
        };
        let description = fields.next().and_then(|value| {
            let value = value.trim();
            (!value.is_empty()).then(|| value.to_string())
        });
        rows.push(FlatpakPackage {
            app_id: app_id.to_string(),
            name: name.to_string(),
            version: version.to_string(),
            branch: branch.to_string(),
            origin: origin.to_string(),
            description,
            system,
            kind,
        });
    }
    rows
}

fn parse_search(output: &str, system: bool) -> Vec<FlatpakPackage> {
    let mut rows = Vec::new();
    for line in output.lines().filter(|line| !line.is_empty()) {
        let mut fields = line.split('\t');
        let (Some(app_id), Some(name), Some(version), Some(description)) =
            (fields.next(), fields.next(), fields.next(), fields.next())
        else {
            continue;
        };
        rows.push(FlatpakPackage {
            app_id: app_id.to_string(),
            name: name.to_string(),
            version: version.to_string(),
            branch: String::new(),
            // Search has no origin column; the description moves to
            // its own field (it used to ride `origin`).
            origin: String::new(),
            description: (!description.is_empty()).then(|| description.to_string()),
            system,
            // Appstream search covers applications.
            kind: FlatpakRefKind::App,
        });
    }
    rows
}

/// Remote names reach argv positionally: flatpak would read a leading
/// `-…` as a flag, so reject it (same charset as services).
fn validate_remote_name(name: &str) -> Result<(), ApiError> {
    if name.is_empty()
        || name.len() > 64
        || name.starts_with('-')
        || !name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
    {
        return Err(ApiError::invalid("invalid Flatpak remote name"));
    }
    Ok(())
}

/// Remote locations: `http(s)://` or `file://` on the SAME charset
/// both exec guards accept (installable-atom: no whitespace, quotes,
/// `%`, `&`, commas that could diverge between root and su paths).
fn validate_remote_url(url: &str) -> Result<(), ApiError> {
    let scheme_ok =
        url.starts_with("http://") || url.starts_with("https://") || url.starts_with("file://");
    if scheme_ok && ursine_api::is_safe_installable_atom(url) && !url.starts_with('-') {
        Ok(())
    } else {
        Err(ApiError::invalid(
            "invalid remote URL (http(s):// or file://, simple charset)",
        ))
    }
}

fn validate_app_id(app_id: &str) -> Result<(), ApiError> {
    if app_id.is_empty()
        || app_id.len() > 255
        || app_id.starts_with('-')
        || app_id == "."
        || app_id == ".."
        || !app_id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
    {
        return Err(ApiError::invalid("invalid Flatpak application id"));
    }
    Ok(())
}

/// Remote names reach the argv unquoted: same strict charset as app ids.
fn validate_remote(remote: Option<&str>) -> Result<String, ApiError> {
    let Some(remote) = remote else {
        return Ok("flathub".to_string());
    };
    if remote.is_empty()
        || remote.len() > 128
        || remote.starts_with('-')
        || !remote
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
    {
        return Err(ApiError::invalid("invalid Flatpak remote name"));
    }
    Ok(remote.to_string())
}

/// Resolve one ref on a remote (`flatpak remote-info`): display name,
/// version and sizes for the install confirmation.
async fn info(
    app_id: &str,
    remote: &str,
    system: bool,
    user: &str,
) -> Result<FlatpakInfo, ApiError> {
    let program = flatpak_path()?;
    let scope = scope(system);
    let args = ["remote-info", scope, remote, app_id];
    let outcome = if system {
        exec::run_root_program(program, &args, TIMEOUT).await?
    } else {
        exec::run_as_user_argv(user, program, &args, TIMEOUT).await?
    };
    parse_info(&outcome.stdout, app_id, remote)
        .ok_or_else(|| ApiError::internal("unexpected flatpak remote-info output"))
}

/// Parse the human `flatpak remote-info` layout:
/// leading-blank name line, then indented `Key: value` rows.
fn parse_info(output: &str, app_id: &str, remote: &str) -> Option<FlatpakInfo> {
    let mut name = String::new();
    let mut version = String::new();
    let mut download_size = String::new();
    let mut installed_size = String::new();
    let mut runtime = String::new();
    let mut seen_id = false;
    for line in output.lines() {
        let trimmed = line.trim();
        if !seen_id && trimmed.is_empty() {
            continue;
        }
        let Some((key, value)) = trimmed.split_once(':') else {
            if !seen_id && name.is_empty() {
                // First non-empty line: "Firefox - Fast, …" summary header.
                name = trimmed.split(" - ").next().unwrap_or(trimmed).to_string();
            }
            continue;
        };
        let value = value.trim();
        match key.trim() {
            "ID" => {
                seen_id = true;
                if value != app_id {
                    return None;
                }
            }
            "Version" => version = value.to_string(),
            "Download" => download_size = value.to_string(),
            "Installed" => installed_size = value.to_string(),
            "Runtime" => runtime = value.to_string(),
            _ => {}
        }
    }
    seen_id.then_some(FlatpakInfo {
        app_id: app_id.to_string(),
        remote: remote.to_string(),
        name: if name.is_empty() {
            app_id.to_string()
        } else {
            name
        },
        version,
        download_size,
        installed_size,
        runtime,
    })
}

fn flatpak_path() -> Result<&'static str, ApiError> {
    FLATPAK_CANDIDATES
        .iter()
        .find(|path| Path::new(path).is_file())
        .copied()
        .ok_or_else(|| ApiError::unavailable("flatpak is not installed", "apk add flatpak"))
}

/// Rotate the installation's `repo/config` (the file `flatpak
/// remote-add/delete` rewrites) before those CLIs run. System installs
/// are root-owned; user installs get chowned back to the user.
async fn backup_repo_config(system: bool, user: &str, copies: usize) -> Result<(), ApiError> {
    let target = if system {
        std::path::PathBuf::from("/var/lib/flatpak/repo/config")
    } else {
        let (home, uid, gid) = crate::modules::theming::target_identity(user)?;
        let path = home.join(".local/share/flatpak/repo/config");
        return crate::backup_configs::rotate_async(path, copies, Some((uid, gid))).await;
    };
    crate::backup_configs::rotate_async(target, copies, None).await
}

/// Count unused runtimes WITHOUT removing anything:
/// `flatpak uninstall --unused` prints the plan, then aborts at the
/// default "N" answer (we always answer `n`; `--yes`/`--noninteractive`
/// are never passed). Returns None when the probe cannot run — the
/// caller shows an unknown count, never a failure.
async fn unused_count(system: bool, user: &str) -> Option<u64> {
    let program = flatpak_path().ok()?;
    let args: &[&str] = if system {
        &["uninstall", "--unused", "--system"]
    } else {
        &["uninstall", "--unused", "--user"]
    };
    let outcome = if system {
        exec::run_root_program_tolerant(program, args, TIMEOUT, Some("n\n"))
            .await
            .ok()?
    } else {
        exec::run_as_user_argv_tolerant(user, program, args, TIMEOUT, Some("n\n"))
            .await
            .ok()?
    };
    Some(parse_unused(&outcome.stdout))
}

/// Refs listed under flatpak's "will be removed" plan header. The pin
/// notice ("won't be removed") never matches that header.
fn parse_unused(stdout: &str) -> u64 {
    let mut count = 0u64;
    let mut counting = false;
    for line in stdout.lines() {
        let trimmed = line.trim();
        if counting {
            if trimmed.starts_with("app/") || trimmed.starts_with("runtime/") {
                count += 1;
            } else if !trimmed.is_empty() {
                break; // the confirmation prompt or any other text
            }
        } else if line.contains("will be removed") {
            counting = true;
        } else if line.contains("Nothing unused") {
            return 0;
        }
    }
    count
}

async fn run(system: bool, user: &str, args: &[&str]) -> Result<String, ApiError> {
    let program = flatpak_path()?;
    // Complete output: installed/update lists must never truncate
    // (user decision: unlimited lists).
    let outcome = if system {
        exec::run_root_program_complete(program, args, TIMEOUT).await?
    } else {
        exec::run_as_user_argv_bounded(user, program, args, TIMEOUT, exec::MAX_COMPLETE_OUTPUT)
            .await?
    };
    Ok(outcome.stdout)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_unused_plan_but_never_the_pin_notice() {
        // Real shapes: the pin notice says "won't be removed" (no
        // "will be removed" header) and its refs must not be counted.
        let pinned = "These runtimes in installation 'user' are pinned \
and won't be removed; see flatpak-pin(1):\n  runtime/org.freedesktop.Platform/x86_64/25.08\nNothing unused to uninstall\n";
        assert_eq!(parse_unused(pinned), 0);

        let plan = "These runtimes will be removed:\n\
  runtime/org.freedesktop.Platform/x86_64/26.08\n\
  runtime/org.freedesktop.Platform.Locale/x86_64/26.08\n\
Is that ok? [y/N] ";
        assert_eq!(parse_unused(plan), 2);

        assert_eq!(parse_unused("Nothing unused to uninstall\n"), 0);
        assert_eq!(parse_unused(""), 0);
    }

    #[test]
    fn regex_matches_app_ids_and_names() {
        let package = |app_id: &str, name: &str| FlatpakPackage {
            app_id: app_id.to_string(),
            name: name.to_string(),
            version: "1".to_string(),
            branch: "stable".to_string(),
            origin: "flathub".to_string(),
            description: None,
            system: false,
            kind: FlatpakRefKind::App,
        };
        let ids = |rows: Vec<FlatpakPatternMatch>| {
            rows.into_iter().map(|row| row.app_id).collect::<Vec<_>>()
        };
        let packages = vec![
            package("org.gnome.Calculator", "Calculator"),
            package("com.bitwarden.desktop", "Bitwarden"),
        ];
        assert_eq!(
            ids(matching_flatpaks("^org[.]gnome", packages.clone()).unwrap()),
            ["org.gnome.Calculator"]
        );
        // Human names match too (the documented flatpak behaviour).
        assert_eq!(
            ids(matching_flatpaks("^(Calc|Bit)", packages.clone()).unwrap()),
            ["org.gnome.Calculator", "com.bitwarden.desktop"]
        );
        assert!(matching_flatpaks("^nothing$", packages.clone())
            .unwrap()
            .is_empty());
        assert!(matching_flatpaks("bad'quote", packages).is_err());
    }

    /// Fixture rows: the application and the runtime ids both carry
    /// `Platform`, which is what made the old preview ambiguous.
    fn pattern_fixture() -> Vec<FlatpakPackage> {
        let row = |app_id: &str, name: &str, branch: &str, kind: FlatpakRefKind| FlatpakPackage {
            app_id: app_id.to_string(),
            name: name.to_string(),
            version: "1".to_string(),
            branch: branch.to_string(),
            origin: "flathub".to_string(),
            description: None,
            system: true,
            kind,
        };
        vec![
            row(
                "org.mozilla.firefox",
                "Firefox",
                "stable",
                FlatpakRefKind::App,
            ),
            row(
                "org.gnome.Platform",
                "GNOME Application Platform",
                "48",
                FlatpakRefKind::Runtime,
            ),
        ]
    }

    #[test]
    fn pattern_preview_keeps_the_kind_of_each_match() {
        // A pattern meant for the application used to answer the runtime's
        // bare id, with nothing to say which of the two it had selected.
        let rows = matching_flatpaks("Platform|Firefox", pattern_fixture()).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].app_id, "org.mozilla.firefox");
        assert_eq!(rows[0].kind, FlatpakRefKind::App);
        assert_eq!(rows[0].branch, "stable");
        assert_eq!(rows[1].app_id, "org.gnome.Platform");
        assert_eq!(rows[1].kind, FlatpakRefKind::Runtime);
        assert_eq!(rows[1].branch, "48");
    }

    #[test]
    fn pattern_preview_lists_the_same_id_once_per_branch() {
        // Two branches of one id are two refs: the preview must not collapse
        // them into one indistinguishable string.
        let runtime = |branch: &str| FlatpakPackage {
            app_id: "org.gnome.Platform".to_string(),
            name: "GNOME Application Platform".to_string(),
            version: "48".to_string(),
            branch: branch.to_string(),
            origin: "flathub".to_string(),
            description: None,
            system: true,
            kind: FlatpakRefKind::Runtime,
        };
        let rows = matching_flatpaks(
            "^org[.]gnome[.]Platform$",
            vec![runtime("48"), runtime("47")],
        )
        .unwrap();
        let branches: Vec<&str> = rows.iter().map(|row| row.branch.as_str()).collect();
        assert_eq!(branches, ["48", "47"]);
        assert!(rows.iter().all(|row| row.kind == FlatpakRefKind::Runtime));
        // The job gets one argv entry, the preview one row per branch.
        assert_eq!(matched_ids(&rows), ["org.gnome.Platform"]);
        let preview = pattern_preview(&rows);
        assert_eq!(preview["matches"].as_array().map(Vec::len), Some(2));
    }

    #[test]
    fn pattern_preview_carries_the_rows_with_kind_and_branch() {
        let rows = matching_flatpaks("Platform|Firefox", pattern_fixture()).unwrap();
        let preview = pattern_preview(&rows);
        assert_eq!(preview["dry_run"], true);
        // `would_change` stays the bare ids clients already read.
        assert_eq!(
            preview["would_change"],
            serde_json::json!(["org.mozilla.firefox", "org.gnome.Platform"])
        );
        let matches = preview["matches"].as_array().unwrap();
        assert_eq!(matches.len(), 2);
        assert_eq!(matches[0]["app_id"], "org.mozilla.firefox");
        assert_eq!(matches[0]["kind"], "app");
        assert_eq!(matches[0]["branch"], "stable");
        assert_eq!(matches[1]["app_id"], "org.gnome.Platform");
        assert_eq!(matches[1]["kind"], "runtime");
        assert_eq!(matches[1]["branch"], "48");
        assert!(pattern_preview(&[])["matches"]
            .as_array()
            .is_some_and(Vec::is_empty));
    }

    #[test]
    fn parse_list_keeps_every_installed_row() {
        // The old300-row cap silently dropped the tail of big systems.
        let output = (0..301)
            .map(|i| format!("org.test.P{i}\tP{i}\t1.0\tstable\tflathub\tdescription {i}"))
            .collect::<Vec<_>>()
            .join("\n");
        let rows = parse_list(&output, false, FlatpakRefKind::App);
        assert_eq!(rows.len(), 301);
        assert_eq!(
            rows.last().map(|row| row.app_id.as_str()),
            Some("org.test.P300")
        );
    }

    #[test]
    fn parse_search_keeps_every_result() {
        let output = (0..301)
            .map(|i| format!("org.test.S{i}\tS{i}\t1.0\tdescription {i}"))
            .collect::<Vec<_>>()
            .join("\n");
        let rows = parse_search(&output, false);
        assert_eq!(rows.len(), 301);
    }

    #[test]
    fn validates_flatpak_remote_inputs() {
        assert!(validate_remote_name("flathub-beta").is_ok());
        assert!(validate_remote_name("my.remote_1").is_ok());
        assert!(validate_remote_name("--system").is_err());
        assert!(validate_remote_name("bad name").is_err());
        assert!(validate_remote_url("https://dl.flathub.org/repo").is_ok());
        assert!(validate_remote_url("file:///media/usb/repo").is_ok());
        assert!(validate_remote_url("ftp://x").is_err());
        assert!(validate_remote_url("/plain/path").is_err());
        assert!(validate_remote_url("https://x has space").is_err());
    }

    #[test]
    fn list_rows_carry_the_description_column() {
        let rows = parse_list(
            "org.mozilla.firefox\tFirefox\t128.0\tstable\tflathub\tA free web browser\n",
            true,
            FlatpakRefKind::App,
        );
        assert_eq!(rows[0].description.as_deref(), Some("A free web browser"));
        // Old five-column output still parses (no description).
        let legacy = parse_list("org.x\tX\t1\tstable\tflathub\n", false, FlatpakRefKind::App);
        assert_eq!(legacy[0].description, None);
    }

    #[test]
    fn parses_flatpak_list_columns() {
        let rows = parse_list(
            "org.mozilla.firefox\tFirefox\t128.0\tstable\tflathub\n",
            true,
            FlatpakRefKind::App,
        );
        assert_eq!(rows[0].app_id, "org.mozilla.firefox");
        assert_eq!(rows[0].origin, "flathub");
        assert!(rows[0].system);
        assert_eq!(rows[0].kind, FlatpakRefKind::App);
        let runtimes = parse_list(
            "org.freedesktop.Platform\tFreedesktop\t26.08\t26.08\tflathub\n",
            false,
            FlatpakRefKind::Runtime,
        );
        assert_eq!(runtimes[0].kind, FlatpakRefKind::Runtime);
        assert!(!runtimes[0].system);
    }

    #[test]
    fn validates_flatpak_ids() {
        assert!(validate_app_id("org.mozilla.firefox").is_ok());
        assert!(validate_app_id("com.example_app-1").is_ok());
        assert!(validate_app_id("-evil").is_err());
        assert!(validate_app_id("org/evil").is_err());
        assert!(validate_app_id(".").is_err());
        assert!(validate_app_id("..").is_err());
    }

    #[test]
    fn validates_remote_names_and_defaults_to_flathub() {
        assert_eq!(validate_remote(None).unwrap(), "flathub");
        assert_eq!(validate_remote(Some("flathub")).unwrap(), "flathub");
        assert_eq!(validate_remote(Some("my-remote_1")).unwrap(), "my-remote_1");
        assert!(validate_remote(Some("")).is_err());
        assert!(validate_remote(Some("-evil")).is_err());
        assert!(validate_remote(Some("remote;rm")).is_err());
    }

    #[test]
    fn parses_remote_info_output() {
        // Captured from `flatpak remote-info --user flathub org.mozilla.firefox`.
        let output = "Firefox - Fast, Private & Safe Web Browser\n\
            \n\
            \x20       ID: org.mozilla.firefox\n\
            \x20      Ref: app/org.mozilla.firefox/x86_64/stable\n\
            \x20   Version: 158.0.1\n\
            \x20    License: MPL-2.0\n\
            Collection: org.flathub.Stable\n\
            \x20  Download: 129.7 MB\n\
            \x20 Installed: 345.7 MB\n\
            \x20   Runtime: org.freedesktop.Platform/x86_64/25.08\n";
        let info = parse_info(output, "org.mozilla.firefox", "flathub").unwrap();
        assert_eq!(info.name, "Firefox");
        assert_eq!(info.version, "158.0.1");
        assert_eq!(info.download_size, "129.7 MB");
        assert_eq!(info.installed_size, "345.7 MB");
        assert_eq!(info.runtime, "org.freedesktop.Platform/x86_64/25.08");
        assert_eq!(info.remote, "flathub");
    }

    #[test]
    fn remote_info_rejects_mismatched_id_and_garbage() {
        let good = "X\n\n ID: org.mozilla.firefox\n Version: 1\n";
        assert!(parse_info(good, "org.other.App", "flathub").is_none());
        assert!(parse_info("no keys here\n", "org.mozilla.firefox", "flathub").is_none());
    }

    #[test]
    fn scope_flags_are_fixed() {
        assert_eq!(scope(true), "--system");
        assert_eq!(scope(false), "--user");
    }
}
