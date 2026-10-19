//! packages.nix — nix profiles via the NEW CLI (`nix profile ...`), flake refs
//! only (`nixpkgs#pkg` or `flake#attr`). Classic channels/`nix-env` are not
//! supported by design (skea999: only the latest mode matters).
//!
//! Every command runs AS the target user (login env, own profile), never root.

mod exec;
mod generations;
mod parse;

use std::time::Duration;

use serde_json::json;
use ursine_api::{Action, ApiError, CommandOutcome};

use super::ModuleCtx;

const LIST_TIMEOUT: Duration = Duration::from_secs(60);
const SEARCH_TIMEOUT: Duration = Duration::from_secs(300);
const HISTORY_TIMEOUT: Duration = Duration::from_secs(60);
const MUTATE_TIMEOUT: Duration = Duration::from_secs(900);
const GC_TIMEOUT: Duration = Duration::from_secs(3600);

/// Normalize an installable: bare attr → `nixpkgs#attr`; flake refs pass
/// through (same accepted charset). Path installs are rejected in v1.
fn normalize_installable(arg: &str) -> Result<String, ApiError> {
    exec::validate_installable(arg)?;
    if arg.starts_with('.') || arg.starts_with('/') {
        return Err(ApiError::invalid(
            "path installs are not supported; use nixpkgs#name",
        ));
    }
    Ok(if arg.contains('#') {
        arg.to_string()
    } else {
        format!("nixpkgs#{arg}")
    })
}

fn lines(outcome: &CommandOutcome) -> serde_json::Value {
    let lines: Vec<&str> = outcome
        .stdout
        .lines()
        .filter(|l| !l.trim().is_empty())
        .collect();
    json!({ "lines": lines })
}

/// Handle a packages.nix action.
pub async fn handle(action: &Action, ctx: &ModuleCtx<'_>) -> Result<serde_json::Value, ApiError> {
    let user = ctx.as_user;

    match action {
        Action::ListInstalled => {
            let out = exec::run_as_user(
                user,
                &[("profile", ""), ("list", ""), ("--json", "")],
                LIST_TIMEOUT,
            )
            .await?;
            Ok(json!(parse::parse_profile_list(&out.stdout)?))
        }
        Action::Search { query } => {
            exec::validate_query(query)?;
            let out = exec::run_as_user(
                user,
                &[("search", ""), ("nixpkgs", ""), (query, ""), ("--json", "")],
                SEARCH_TIMEOUT,
            )
            .await?;
            Ok(json!(parse::parse_search(&out.stdout)?))
        }
        Action::Install { package } => {
            let installable = normalize_installable(package)?;
            let out = exec::run_as_user(
                user,
                &[("profile", ""), ("add", ""), (&installable, "")],
                MUTATE_TIMEOUT,
            )
            .await?;
            Ok(json!(out))
        }
        Action::Remove { package } => {
            exec::validate_installable(package)?;
            let out = exec::run_as_user(
                user,
                &[("profile", ""), ("remove", ""), (package, "")],
                MUTATE_TIMEOUT,
            )
            .await?;
            Ok(json!(out))
        }
        Action::Upgrade { package, all } => {
            let out = if *all {
                exec::run_as_user(
                    user,
                    &[("profile", ""), ("upgrade", ""), ("--all", "")],
                    MUTATE_TIMEOUT,
                )
                .await?
            } else {
                let package = package
                    .as_deref()
                    .ok_or_else(|| ApiError::invalid("package required when all=false"))?;
                exec::validate_installable(package)?;
                exec::run_as_user(
                    user,
                    &[("profile", ""), ("upgrade", ""), (package, "")],
                    MUTATE_TIMEOUT,
                )
                .await?
            };
            Ok(json!(out))
        }
        Action::Generations => {
            let out = exec::run_as_user(user, &[("profile", ""), ("history", "")], HISTORY_TIMEOUT)
                .await?;
            Ok(lines(&out))
        }
        Action::Rollback { generation } => {
            let gen_str = generation.map(|g| g.to_string());
            let mut args: Vec<(&str, &str)> = vec![("profile", ""), ("rollback", "")];
            if let Some(g) = gen_str.as_deref() {
                args.push(("--to", g));
            }
            let out = exec::run_as_user(user, &args, MUTATE_TIMEOUT).await?;
            Ok(json!(out))
        }
        Action::WipeHistory {
            older_than,
            dry_run,
        } => {
            let mut args: Vec<(&str, &str)> = vec![("profile", ""), ("wipe-history", "")];
            if let Some(age) = older_than.as_deref() {
                exec::validate_installable(age)?; // digits + units (e.g. 30d), safe charset
                args.push(("--older-than", age));
            }
            if *dry_run {
                args.push(("--dry-run", ""));
            }
            let out = exec::run_as_user(user, &args, MUTATE_TIMEOUT).await?;
            Ok(json!(out))
        }
        Action::DiffClosures { generation } => {
            let gen_str = generation.map(|g| g.to_string());
            let mut args: Vec<(&str, &str)> = vec![("profile", ""), ("diff-closures", "")];
            if let Some(g) = gen_str.as_deref() {
                args.push(("--to", g));
            }
            let out = exec::run_as_user(user, &args, HISTORY_TIMEOUT).await?;
            Ok(lines(&out))
        }
        Action::StoreGc => {
            // Store-wide GC: unreachable paths only; runs as root via the daemon.
            let out = exec::run_as_root(&[("store", ""), ("gc", "")], GC_TIMEOUT).await?;
            Ok(json!(out))
        }
        Action::PruneGenerations { keep, dry_run } => {
            let profile_path = format!("/nix/var/nix/profiles/per-user/{user}/profile");
            let hist =
                exec::run_as_user(user, &[("profile", ""), ("history", "")], HISTORY_TIMEOUT)
                    .await?;
            let gens = generations::parse_history(&hist.stdout);
            let plan = generations::prune_plan(&gens, *keep)?;
            let plan_numbers: Vec<u32> = plan.iter().map(|g| g.number).collect();

            if *dry_run {
                return Ok(json!({
                    "dry_run": true,
                    "would_delete": plan_numbers,
                    "kept": gens.len().saturating_sub(plan.len()),
                }));
            }

            let keep_arg = format!("+{keep}");
            let out = exec::run_as_user_program(
                user,
                "nix-env",
                &[("-p", &profile_path), ("--delete-generations", &keep_arg)],
                MUTATE_TIMEOUT,
            )
            .await?;
            Ok(json!({
                "dry_run": false,
                "deleted": plan_numbers,
                "stdout": out.stdout,
                "stderr": out.stderr,
            }))
        }
        _ => Err(ApiError::invalid(
            "action is not valid for module packages_nix",
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bare_names_get_the_nixpkgs_prefix() {
        assert_eq!(normalize_installable("hello").unwrap(), "nixpkgs#hello");
        assert_eq!(
            normalize_installable("python3Packages.requests").unwrap(),
            "nixpkgs#python3Packages.requests"
        );
    }

    #[test]
    fn explicit_flake_refs_pass_through() {
        assert_eq!(
            normalize_installable("nixpkgs#hello").unwrap(),
            "nixpkgs#hello"
        );
        assert_eq!(
            normalize_installable("github:NixOS/nixpkgs#hello").unwrap(),
            "github:NixOS/nixpkgs#hello"
        );
    }

    #[test]
    fn path_installs_are_rejected() {
        assert!(normalize_installable("./flake#pkg").is_err());
        assert!(normalize_installable("/abs/path").is_err());
    }
}
