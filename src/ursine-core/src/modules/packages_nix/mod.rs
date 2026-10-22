//! packages.nix — nix profiles via the NEW CLI (`nix profile ...`).
//!
//! Flake refs only (`nixpkgs#pkg` or `flake#attr`). Classic channels/`nix-env`
//! are not supported by design (skea999: only the latest mode matters).
//! Every command runs AS the target user (login env, own profile), never root.

pub mod exec;
mod generations;
mod parse;

use std::time::Duration;

use serde_json::json;
use ursine_api::{
    normalize_installable, timeouts, validate_installable, Action, ApiError, CommandOutcome,
    ErrorCode, InstalledPackage,
};

use super::ModuleCtx;

const LIST_TIMEOUT: Duration = Duration::from_secs(timeouts::PROFILE_SECS);
const SEARCH_TIMEOUT: Duration = Duration::from_secs(timeouts::SEARCH_SECS);
const HISTORY_TIMEOUT: Duration = Duration::from_secs(timeouts::HISTORY_SECS);
const MUTATE_TIMEOUT: Duration = Duration::from_secs(timeouts::MUTATE_SECS);
const GC_TIMEOUT: Duration = Duration::from_secs(timeouts::GC_SECS);

/// Profile selected by `nix profile` for a regular user. Resolve it in the
/// user's login environment so `XDG/NIX_STATE_HOME` overrides are honoured, and
/// use the same path for every action (including generation deletion).
async fn profile_path(user: &str) -> Result<String, ApiError> {
    let out = exec::run_as_user_script(
        user,
        r#"printf %s "${NIX_STATE_HOME:-${XDG_STATE_HOME:-$HOME/.local/state}/nix}/profiles/profile""#,
        LIST_TIMEOUT,
    )
    .await?;
    let path = out.stdout.trim();
    if !path.starts_with('/') || path.contains('\0') {
        return Err(ApiError::internal("cannot resolve the user profile path"));
    }
    // Old installs (use-xdg-base-directories = false) still use the legacy
    // per-user profile; prefer whichever profile already exists.
    if std::path::Path::new(path).exists() {
        return Ok(path.to_string());
    }
    let legacy = format!("/nix/var/nix/profiles/per-user/{user}/profile");
    if std::path::Path::new(&legacy).exists() {
        return Ok(legacy);
    }
    Ok(path.to_string())
}

/// Names the regex actions would touch. The daemon computes matches so a
/// frontend never re-implements nix regex semantics.
async fn regex_matches(user: &str, pattern: &str) -> Result<Vec<String>, ApiError> {
    exec::validate_regex(pattern)?;
    let out = exec::run_as_user(
        user,
        &[("profile", ""), ("list", ""), ("--json", "")],
        HISTORY_TIMEOUT,
    )
    .await?;
    filter_regex_matches(parse::parse_profile_list(&out.stdout)?, pattern)
}

fn filter_regex_matches(
    packages: Vec<InstalledPackage>,
    pattern: &str,
) -> Result<Vec<String>, ApiError> {
    let regex =
        regex::Regex::new(pattern).map_err(|_| ApiError::invalid("invalid regex pattern"))?;
    Ok(packages
        .into_iter()
        .filter(|package| regex.is_match(&package.name))
        .map(|package| package.name)
        .collect())
}

fn lines(outcome: &CommandOutcome) -> serde_json::Value {
    let lines: Vec<String> = outcome
        .stdout
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(generations::strip_ansi)
        .collect();
    json!({ "lines": lines })
}

/// Validate the profile link and read the active generation.
fn validated_profile_generation(profile_path: &std::path::Path) -> Result<u32, ApiError> {
    let meta = std::fs::symlink_metadata(profile_path)
        .map_err(|e| ApiError::invalid(format!("profile path is not a symlink: {e}")))?;
    if !meta.file_type().is_symlink() {
        return Err(ApiError::invalid("profile path is not a symlink"));
    }
    let target = std::fs::read_link(profile_path)
        .map_err(|e| ApiError::invalid(format!("cannot read profile link: {e}")))?;
    generations::profile_target_generation(&target, profile_path)
        .ok_or_else(|| ApiError::invalid("profile link is not a generation link"))
}

fn prune_apply_payload(
    current: Option<u32>,
    deleted: &[u32],
    stdout: &str,
    stderr: &str,
) -> serde_json::Value {
    json!({
        "dry_run": false,
        "current": current,
        "deleted": deleted,
        "stdout": stdout,
        "stderr": stderr,
    })
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
            let out = exec::run_as_user_search(
                user,
                &[("search", ""), ("nixpkgs", ""), (query, ""), ("--json", "")],
                SEARCH_TIMEOUT,
            )
            .await?;
            let page = parse::parse_search(&out.stdout)?;
            Ok(json!({
                "results": page.results,
                "truncated": page.truncated,
            }))
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
            validate_installable(package)?;
            let out = exec::run_as_user(
                user,
                &[("profile", ""), ("remove", ""), (package, "")],
                MUTATE_TIMEOUT,
            )
            .await?;
            Ok(json!(out))
        }
        Action::RemoveAll => {
            let out = exec::run_as_user(
                user,
                &[("profile", ""), ("remove", ""), ("--all", "")],
                MUTATE_TIMEOUT,
            )
            .await?;
            Ok(json!(out))
        }
        Action::RemoveRegex { pattern, dry_run } => {
            let names = regex_matches(user, pattern).await?;
            if *dry_run {
                return Ok(json!({
                    "dry_run": true,
                    "would_change": names,
                }));
            }
            if names.is_empty() {
                return Ok(json!(CommandOutcome {
                    stdout: String::new(),
                    stderr: String::new(),
                }));
            }
            let mut argv: Vec<&str> = vec!["profile", "remove"];
            argv.extend(names.iter().map(String::as_str));
            let out = exec::run_as_user_argv(user, "nix", &argv, MUTATE_TIMEOUT).await?;
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
                validate_installable(package)?;
                exec::run_as_user(
                    user,
                    &[("profile", ""), ("upgrade", ""), (package, "")],
                    MUTATE_TIMEOUT,
                )
                .await?
            };
            Ok(json!(out))
        }
        Action::UpgradeRegex { pattern, dry_run } => {
            let names = regex_matches(user, pattern).await?;
            if *dry_run {
                return Ok(json!({
                    "dry_run": true,
                    "would_change": names,
                }));
            }
            if names.is_empty() {
                return Ok(json!(CommandOutcome {
                    stdout: String::new(),
                    stderr: String::new(),
                }));
            }
            let mut argv: Vec<&str> = vec!["profile", "upgrade"];
            argv.extend(names.iter().map(String::as_str));
            let out = exec::run_as_user_argv(user, "nix", &argv, MUTATE_TIMEOUT).await?;
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
                validate_installable(age)?; // digits + units (e.g. 30d), safe charset
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
            let program = exec::nix_path()?;
            let out = exec::run_root_program(program, &["store", "gc"], GC_TIMEOUT).await?;
            Ok(json!(out))
        }
        Action::PruneGenerations {
            keep,
            dry_run,
            expected_current,
            expected_delete,
        } => {
            exec::validate_username(user)?;
            let profile = profile_path(user).await?;
            let profile_ref = std::path::Path::new(&profile);

            // nix profile history does not mark the active generation; read it
            // from the profile symlink (profile-N-link).
            let link = std::fs::read_link(profile_ref)
                .map_err(|e| ApiError::invalid(format!("cannot read profile link: {e}")))?;
            let current =
                generations::parse_generation_link(&link.to_string_lossy()).ok_or_else(|| {
                    ApiError::new(
                        ErrorCode::Conflict,
                        "cannot determine the active generation; refresh and retry",
                    )
                })?;

            if !dry_run {
                // A destructive apply without the exact preview is refused at
                // the protocol level: the user must have seen this plan.
                let expected_current = expected_current.ok_or_else(|| {
                    ApiError::invalid("prune apply requires a previewed current generation")
                })?;
                if current != expected_current {
                    return Err(ApiError::new(
                        ErrorCode::Conflict,
                        "profile changed since preview; preview again",
                    ));
                }
            }

            let hist = exec::run_as_user(
                user,
                &[("profile", ""), ("history", ""), ("--profile", &profile)],
                HISTORY_TIMEOUT,
            )
            .await?;
            let gens = generations::parse_history(&hist.stdout);
            let plan = generations::prune_plan(&gens, Some(current), *keep)
                .map_err(|_| ApiError::invalid("keep must be between 1 and 1000"))?;

            if *dry_run {
                return Ok(json!({
                    "dry_run": true,
                    "current": current,
                    "would_delete": plan,
                    "kept": gens.len().saturating_sub(plan.len()),
                }));
            }

            let expected_delete = expected_delete.as_ref().ok_or_else(|| {
                ApiError::invalid("prune apply requires a previewed deletion plan")
            })?;
            if expected_delete != &plan {
                return Err(ApiError::new(
                    ErrorCode::Conflict,
                    "generation plan changed since preview; preview again",
                ));
            }

            let fresh_current = validated_profile_generation(profile_ref)?;
            if current != fresh_current {
                return Err(ApiError::new(
                    ErrorCode::Conflict,
                    "profile changed while pruning; retry",
                ));
            }
            if plan.is_empty() {
                return Ok(prune_apply_payload(Some(fresh_current), &[], "", ""));
            }

            // Run as the profile owner: the profile directory is user-owned,
            // and root-created generation links would break later user actions.
            // The argv is an explicit list, never a user-controlled path.
            let mut argv: Vec<String> =
                vec!["-p".into(), profile.clone(), "--delete-generations".into()];
            argv.extend(plan.iter().map(u32::to_string));
            let argv_refs: Vec<&str> = argv.iter().map(String::as_str).collect();
            let out =
                match exec::run_as_user_argv(user, "nix-env", &argv_refs, MUTATE_TIMEOUT).await {
                    Ok(out) => out,
                    Err(error) => {
                        // Classify a concurrent rollback from the fresh link, not
                        // from nix-env wording.
                        let changed = std::fs::read_link(profile_ref)
                            .ok()
                            .and_then(|link| {
                                generations::parse_generation_link(&link.to_string_lossy())
                            })
                            .is_some_and(|now| now != fresh_current);
                        if changed {
                            return Err(ApiError::new(
                                ErrorCode::Conflict,
                                "profile changed while pruning; retry",
                            ));
                        }
                        return Err(error);
                    }
                };
            Ok(prune_apply_payload(
                Some(fresh_current),
                &plan,
                &out.stdout,
                &out.stderr,
            ))
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

    #[test]
    fn regex_preview_lists_only_matching_names() {
        let packages = vec![
            InstalledPackage {
                name: "python3-requests".into(),
                attr_path: None,
                original_url: None,
                url: None,
                store_paths: Vec::new(),
                active: true,
            },
            InstalledPackage {
                name: "hello".into(),
                attr_path: None,
                original_url: None,
                url: None,
                store_paths: Vec::new(),
                active: true,
            },
        ];
        assert_eq!(
            filter_regex_matches(packages, "^python").unwrap(),
            vec!["python3-requests".to_string()]
        );
        assert!(filter_regex_matches(Vec::new(), "[").is_err());
    }

    #[test]
    fn prune_apply_noop_has_the_dry_run_payload_shape() {
        let payload = prune_apply_payload(Some(4), &[], "", "");
        assert_eq!(payload["dry_run"], false);
        assert_eq!(payload["current"], 4);
        assert_eq!(payload["deleted"], json!([]));
        assert_eq!(payload["stdout"], "");
        assert_eq!(payload["stderr"], "");
    }
}
