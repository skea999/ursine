//! snapshots — btrbk integration.
//!
//! btrbk is the source of truth for listing, stats and snapshot creation.
//! Restore uses `btrfs subvolume snapshot` because btrbk 0.32 has no restore
//! command; it is previewed and confirmed exactly like any destructive action.

mod btrbk;
mod conf;
mod restore;

use serde_json::json;
use ursine_api::{Action, ApiError};

use super::ModuleCtx;

pub async fn handle(action: &Action, _ctx: &ModuleCtx<'_>) -> Result<serde_json::Value, ApiError> {
    match action {
        Action::ListSnapshots => {
            let raw = btrbk::run(
                &["--format", "raw", "list", "snapshots"],
                btrbk::LIST_TIMEOUT,
            )
            .await?;
            Ok(json!({ "snapshots": btrbk::parse_snapshots(&raw) }))
        }
        Action::Stats => {
            let raw = btrbk::run(&["--format", "raw", "stats"], btrbk::LIST_TIMEOUT).await?;
            Ok(json!({ "stats": btrbk::parse_stats(&raw) }))
        }
        Action::CreateSnapshot { section } => {
            let mut args = vec!["snapshot"];
            if let Some(section) = section {
                validate_section(section)?;
                args.push(section);
            }
            let out = btrbk::run(&args, btrbk::MUTATE_TIMEOUT).await?;
            Ok(json!({ "stdout": out }))
        }
        Action::RestoreSnapshot {
            snapshot,
            target,
            dry_run,
        } => {
            if *dry_run {
                restore::preview(snapshot, target)
            } else {
                restore::apply(snapshot, target).await
            }
        }
        Action::GetBtrbkConfig => {
            let view = conf::read(&btrbk::config_path())?;
            Ok(json!(view))
        }
        Action::InitBtrbkConfig => {
            let view = conf::init(&btrbk::config_path()).await?;
            Ok(json!(view))
        }
        Action::SetBtrbkConfig { settings } => {
            let view = conf::set(&btrbk::config_path(), settings.as_ref()).await?;
            Ok(json!(view))
        }
        Action::DeleteSnapshot { snapshot } => {
            let raw = btrbk::run(
                &["--format", "raw", "list", "snapshots"],
                btrbk::LIST_TIMEOUT,
            )
            .await?;
            let snaps = btrbk::parse_snapshots(&raw);
            if !snaps.iter().any(|s| s.snapshot_subvolume == *snapshot) {
                return Err(ApiError::invalid("snapshot is not managed by btrbk"));
            }
            delete_snapshots(std::slice::from_ref(snapshot)).await
        }
        Action::DeleteSnapshots { sources } => {
            let raw = btrbk::run(
                &["--format", "raw", "list", "snapshots"],
                btrbk::LIST_TIMEOUT,
            )
            .await?;
            let paths: Vec<String> = btrbk::parse_snapshots(&raw)
                .into_iter()
                .filter(|s| sources.is_empty() || sources.contains(&s.source_subvolume))
                .map(|s| s.snapshot_subvolume)
                .collect();
            delete_snapshots(&paths).await
        }
        _ => Err(ApiError::invalid(
            "action is not valid for module snapshots",
        )),
    }
}

async fn delete_snapshots(paths: &[String]) -> Result<serde_json::Value, ApiError> {
    if paths.is_empty() {
        return Ok(json!({"deleted": [], "failed": []}));
    }
    let program = btrbk::btrfs_path()?;
    let mut deleted = Vec::new();
    let mut failed = Vec::new();
    for path in paths {
        match crate::modules::packages_nix::exec::run_root_program(
            program,
            &["subvolume", "delete", path],
            btrbk::MUTATE_TIMEOUT,
        )
        .await
        {
            Ok(_) => deleted.push(path.clone()),
            Err(error) => failed.push(json!({"path": path, "error": error.message})),
        }
    }
    Ok(json!({"deleted": deleted, "failed": failed}))
}

fn validate_section(section: &str) -> Result<(), ApiError> {
    if section.starts_with('-')
        || section.is_empty()
        || section.len() > 256
        || !section
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-' | '/' | '@'))
    {
        return Err(ApiError::invalid("invalid btrbk section"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn section_validation_rejects_options() {
        assert!(validate_section("home").is_ok());
        assert!(validate_section("@home").is_ok());
        assert!(validate_section("--all").is_err());
        assert!(validate_section("a b").is_err());
        assert!(validate_section("").is_err());
    }
}
