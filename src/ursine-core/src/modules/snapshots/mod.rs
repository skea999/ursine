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
        _ => Err(ApiError::invalid(
            "action is not valid for module snapshots",
        )),
    }
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
