//! packages.flatpak — Flatpak applications for system and user installations.
//!
//! System operations run as root; user operations run as the authenticated
//! target user through `su`. No shell is involved.

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use serde_json::json;
use ursine_api::{Action, ApiError, FlatpakPackage, FlatpakRemote, ModuleId};

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

pub async fn handle(action: &Action, ctx: &ModuleCtx<'_>) -> Result<serde_json::Value, ApiError> {
    match action {
        Action::ListFlatpak { system } => {
            Ok(json!({"packages": list(*system, ctx.as_user).await?}))
        }
        Action::ListFlatpakRemotes => Ok(json!({"remotes": remotes().await?})),
        Action::SearchFlatpak { query, system } => Ok(json!({
            "packages": search(query, *system, ctx.as_user).await?,
        })),
        Action::InstallFlatpak { app_id, system } => {
            validate_app_id(app_id)?;
            let scope = scope(*system);
            let args = [
                "install",
                "-y",
                "--noninteractive",
                scope,
                "flathub",
                app_id,
            ];
            start_job(ctx, "install_flatpak", *system, &args)
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
    let scope = scope(system);
    let args = [
        "list",
        "--columns=application,name,version,branch,origin",
        scope,
    ];
    let output = run(system, user, &args).await?;
    Ok(parse_list(&output, system))
}

async fn remotes() -> Result<Vec<FlatpakRemote>, ApiError> {
    let output = run(true, "root", &["remotes", "--columns=name,url"]).await?;
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

fn parse_list(output: &str, system: bool) -> Vec<FlatpakPackage> {
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
        });
    }
    rows
}

fn validate_app_id(app_id: &str) -> Result<(), ApiError> {
    if app_id.is_empty()
        || app_id.len() > 255
        || app_id.starts_with('-')
        || !app_id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
    {
        return Err(ApiError::invalid("invalid Flatpak application id"));
    }
    Ok(())
}

fn flatpak_path() -> Result<&'static str, ApiError> {
    FLATPAK_CANDIDATES
        .iter()
        .find(|path| Path::new(path).is_file())
        .copied()
        .ok_or_else(|| ApiError::unavailable("flatpak is not installed", "apk add flatpak"))
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
    fn parses_flatpak_list_columns() {
        let rows = parse_list(
            "org.mozilla.firefox\tFirefox\t128.0\tstable\tflathub\n",
            true,
        );
        assert_eq!(rows[0].app_id, "org.mozilla.firefox");
        assert_eq!(rows[0].origin, "flathub");
        assert!(rows[0].system);
    }

    #[test]
    fn validates_flatpak_ids() {
        assert!(validate_app_id("org.mozilla.firefox").is_ok());
        assert!(validate_app_id("com.example_app-1").is_ok());
        assert!(validate_app_id("-evil").is_err());
        assert!(validate_app_id("org/evil").is_err());
    }

    #[test]
    fn scope_flags_are_fixed() {
        assert_eq!(scope(true), "--system");
        assert_eq!(scope(false), "--user");
    }
}
