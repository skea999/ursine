//! packages.flatpak — Flatpak applications for system and user installations.
//!
//! System operations run as root; user operations run as the authenticated
//! target user through `su`. No shell is involved.

mod permissions;

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use serde_json::json;
use ursine_api::{
    Action, ApiError, FlatpakEnvVar, FlatpakInfo, FlatpakPackage, FlatpakPermissions,
    FlatpakRefKind, FlatpakRemote, ModuleId,
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
const MAX_ROWS: usize = 300;
/// Shared column order for `flatpak list` / `flatpak remote-ls` rows —
/// [`parse_list`] reads the fields positionally in exactly this order.
const LIST_COLUMNS: &str = "application,name,version,branch,origin";
/// Serializes permission SETs in this process (read + write of one
/// keyfile). A concurrent external writer between our read and write is
/// still a residual TOCTOU: flatpak has no file lock for overrides.
static PERMISSIONS_MUTATION: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

pub async fn handle(action: &Action, ctx: &ModuleCtx<'_>) -> Result<serde_json::Value, ApiError> {
    match action {
        Action::ListFlatpak { system } => {
            Ok(json!({"packages": list(*system, ctx.as_user).await?}))
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
        Action::GetFlatpakPermissions { app_id, system } => {
            validate_app_id(app_id)?;
            Ok(json!(permissions_get(app_id, *system, ctx.as_user).await?))
        }
        Action::SetFlatpakPermissions {
            app_id,
            system,
            sockets,
            devices,
            filesystems,
            env,
        } => {
            validate_app_id(app_id)?;
            permissions_set(
                app_id,
                *system,
                sockets,
                devices,
                filesystems,
                env,
                ctx.as_user,
                crate::backup_configs::copies_from(&ctx.config),
            )
            .await?;
            Ok(json!(permissions_get(app_id, *system, ctx.as_user).await?))
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

async fn list(system: bool, user: &str) -> Result<Vec<FlatpakPackage>, ApiError> {
    // The `ref` column carries no app/runtime prefix, so the split needs
    // two queries; rows are tagged from the flag that produced them.
    // Both run together (same binary/scope) and fail closed together.
    let (apps, runtimes) = tokio::try_join!(
        list_kind(system, user, "--app", FlatpakRefKind::App),
        list_kind(system, user, "--runtime", FlatpakRefKind::Runtime),
    )?;
    let mut rows = apps;
    rows.extend(runtimes);
    // parse caps per query; the merged list keeps the single global cap.
    rows.truncate(MAX_ROWS);
    Ok(rows)
}

/// Refs whose remote has a newer commit (`flatpak remote-ls --updates`).
/// Column order matches [`parse_list`], so rows share `FlatpakPackage`.
async fn updates(system: bool, user: &str) -> Result<Vec<FlatpakPackage>, ApiError> {
    let (apps, runtimes) = tokio::try_join!(
        updates_kind(system, user, "--app", FlatpakRefKind::App),
        updates_kind(system, user, "--runtime", FlatpakRefKind::Runtime),
    )?;
    let mut rows = apps;
    rows.extend(runtimes);
    rows.truncate(MAX_ROWS);
    Ok(rows)
}

async fn updates_kind(
    system: bool,
    user: &str,
    kind_flag: &str,
    kind: FlatpakRefKind,
) -> Result<Vec<FlatpakPackage>, ApiError> {
    let scope = scope(system);
    let columns = format!("--columns={LIST_COLUMNS}");
    let args = ["remote-ls", scope, kind_flag, "--updates", columns.as_str()];
    let output = run(system, user, &args).await?;
    Ok(parse_list(&output, system, kind))
}

/// Absolute path of the override keyfile for `app_id`. The id passed the
/// strict charset check, so it cannot traverse out of the directory.
fn override_path(system: bool, user: &str, app_id: &str) -> Result<String, ApiError> {
    if system {
        return Ok(format!("/var/lib/flatpak/overrides/{app_id}"));
    }
    let home = crate::modules::home_for_user(user)?;
    Ok(format!(
        "{}/.local/share/flatpak/overrides/{app_id}",
        home.display()
    ))
}

/// Decode the user-side reader marker: `1` + content (exists), `0` (missing).
fn decode_marker(stdout: &str) -> Result<(bool, String), ApiError> {
    let mut chars = stdout.chars();
    match chars.next() {
        Some('1') => Ok((true, chars.collect())),
        Some('0') => Ok((false, String::new())),
        _ => Err(ApiError::internal("unexpected file read response")),
    }
}

const fn scope_reader(system: bool, user: &str) -> &str {
    if system {
        "root"
    } else {
        user
    }
}

/// Editable override lists plus the effective sandbox view.
async fn permissions_get(
    app_id: &str,
    system: bool,
    user: &str,
) -> Result<FlatpakPermissions, ApiError> {
    let path = override_path(system, user, app_id)?;
    let outcome = exec::run_as_user_read_file(scope_reader(system, user), &path).await?;
    let (exists, raw) = decode_marker(&outcome.stdout)?;
    let parsed = permissions::parse(&raw);
    let scope = scope(system);
    // An override file can outlive its app: keep the editable state
    // readable (and cleanable) even when `flatpak info` fails.
    let effective = match run(system, user, &["info", scope, "--show-permissions", app_id]).await {
        Ok(effective) => effective,
        Err(_) if exists => String::new(),
        Err(error) => return Err(error),
    };
    Ok(FlatpakPermissions {
        app_id: app_id.to_string(),
        exists,
        sockets: parsed.sockets,
        devices: parsed.devices,
        filesystems: parsed.filesystems,
        env: parsed.env,
        effective,
    })
}

/// Replace the four editable lists, preserving every other key/group of
/// the override file (no `--reset`: it would drop Session Bus policies).
#[allow(clippy::too_many_arguments)] // mirrors the SetFlatpakPermissions action fields
async fn permissions_set(
    app_id: &str,
    system: bool,
    sockets: &[String],
    devices: &[String],
    filesystems: &[String],
    env: &[FlatpakEnvVar],
    user: &str,
    copies: usize,
) -> Result<(), ApiError> {
    permissions::validate_tokens(sockets, 64)?;
    permissions::validate_tokens(devices, 64)?;
    permissions::validate_filesystems(filesystems)?;
    permissions::validate_env(env)?;
    let _guard = PERMISSIONS_MUTATION.lock().await;
    let path = override_path(system, user, app_id)?;
    let reader = scope_reader(system, user);
    let outcome = exec::run_as_user_read_file(reader, &path).await?;
    let (_exists, raw) = decode_marker(&outcome.stdout)?;
    let rendered = permissions::render(&raw, sockets, devices, filesystems, env);
    exec::run_as_user_atomic_write(reader, &path, &rendered, copies).await?;
    Ok(())
}

async fn list_kind(
    system: bool,
    user: &str,
    kind_flag: &str,
    kind: FlatpakRefKind,
) -> Result<Vec<FlatpakPackage>, ApiError> {
    let scope = scope(system);
    let columns = format!("--columns={LIST_COLUMNS}");
    let args = ["list", columns.as_str(), kind_flag, scope];
    let output = run(system, user, &args).await?;
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
    let query = query.trim();
    if query.is_empty()
        || query.len() > 128
        || query.starts_with('-')
        || !query
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '+' | '-'))
    {
        return Err(ApiError::invalid("invalid flatpak search query"));
    }
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
    for line in output
        .lines()
        .filter(|line| !line.is_empty())
        .take(MAX_ROWS)
    {
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
        rows.push(FlatpakPackage {
            app_id: app_id.to_string(),
            name: name.to_string(),
            version: version.to_string(),
            branch: branch.to_string(),
            origin: origin.to_string(),
            system,
            kind,
        });
    }
    rows
}

fn parse_search(output: &str, system: bool) -> Vec<FlatpakPackage> {
    let mut rows = Vec::new();
    for line in output
        .lines()
        .filter(|line| !line.is_empty())
        .take(MAX_ROWS)
    {
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
            origin: description.to_string(),
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

async fn run(system: bool, user: &str, args: &[&str]) -> Result<String, ApiError> {
    let program = flatpak_path()?;
    let outcome = if system {
        exec::run_root_program(program, args, TIMEOUT).await?
    } else {
        exec::run_as_user_argv(user, program, args, TIMEOUT).await?
    };
    Ok(outcome.stdout)
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn decodes_reader_marker() {
        assert_eq!(decode_marker("0").unwrap(), (false, String::new()));
        assert_eq!(
            decode_marker("1[Context]").unwrap(),
            (true, "[Context]".to_string())
        );
        assert!(decode_marker("").is_err());
        assert!(decode_marker("x2").is_err());
    }

    #[test]
    fn override_paths_are_fixed_prefixes() {
        let system_path = override_path(true, "root", "org.x.Y").unwrap();
        assert_eq!(system_path, "/var/lib/flatpak/overrides/org.x.Y");
        // User scope needs a passwd entry; CI containers have root only.
        if let Ok(path) = override_path(false, "root", "org.x.Y") {
            assert!(path.ends_with("/.local/share/flatpak/overrides/org.x.Y"));
            assert!(path.starts_with('/'));
        }
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
