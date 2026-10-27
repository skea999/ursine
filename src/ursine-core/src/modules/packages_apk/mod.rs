//! packages.apk — Alpine package management through the system `apk` binary.
//!
//! Every command runs as root with an absolute trusted path and argv arguments
//! only. The GUI never parses shell output; it receives typed rows.

use std::path::Path;
use std::time::Duration;

use serde_json::json;
use ursine_api::{Action, ApiError, ApkPackage, ApkUpdate};

use super::ModuleCtx;
use crate::modules::packages_nix::exec;

const APK_CANDIDATES: [&str; 3] = ["/sbin/apk", "/usr/sbin/apk", "/usr/bin/apk"];
const TIMEOUT: Duration = Duration::from_secs(300);
const MAX_ROWS: usize = 300;

pub async fn handle(action: &Action, _ctx: &ModuleCtx<'_>) -> Result<serde_json::Value, ApiError> {
    match action {
        Action::ListApk => Ok(json!({"packages": installed().await?})),
        Action::SearchApk { query } => Ok(json!({"packages": search(query).await?})),
        Action::RefreshApk => Ok(json!({"output": run(&["update"]).await?})),
        Action::ListApkRepositories => Ok(json!({"repositories": repositories()?})),
        Action::InstallApk { package } | Action::UpgradeApkPackage { package } => {
            validate_package(package)?;
            Ok(json!({"output": run(&["add", "--no-progress", package]).await?}))
        }
        Action::RemoveApk { package } => {
            validate_package(package)?;
            Ok(json!({"output": run(&["del", package]).await?}))
        }
        Action::UpgradeApk => Ok(json!({"output": run(&["upgrade", "--no-progress"]).await?})),
        Action::ListApkUpdates => Ok(json!({"updates": updates().await?})),
        _ => Err(ApiError::invalid(
            "action is not valid for module packages_apk",
        )),
    }
}

fn repositories() -> Result<Vec<String>, ApiError> {
    let content = std::fs::read_to_string("/etc/apk/repositories")
        .map_err(|e| ApiError::internal(format!("read /etc/apk/repositories: {e}")))?;
    Ok(content
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .take(64)
        .map(str::to_string)
        .collect())
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

async fn run(args: &[&str]) -> Result<String, ApiError> {
    let program = APK_CANDIDATES
        .iter()
        .find(|path| Path::new(path).is_file())
        .copied()
        .ok_or_else(|| {
            ApiError::unavailable("apk is not available", "boot a FeralOS/Alpine system")
        })?;
    exec::run_root_program(program, args, TIMEOUT)
        .await
        .map(|outcome| outcome.stdout)
}

#[cfg(test)]
mod tests {
    use super::*;

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
