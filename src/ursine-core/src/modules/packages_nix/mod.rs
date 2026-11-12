//! packages.nix — nix profiles via the NEW CLI (`nix profile ...`).
//!
//! Flake refs only (`nixpkgs#pkg` or `flake#attr`). Classic channels/`nix-env`
//! are not supported by design (skea999: only the latest mode matters).
//! Every command runs AS the target user (login env, own profile), never root.

pub mod exec;
mod generations;
mod parse;

use std::sync::Arc;
use std::time::Duration;

use serde_json::json;
use ursine_api::{
    normalize_installable, timeouts, validate_installable, Action, ApiError, CommandOutcome,
    ErrorCode, InstalledPackage, ModuleId, NixRepository, SearchResult,
};

use super::ModuleCtx;
use crate::jobs::JobSink;

const LIST_TIMEOUT: Duration = Duration::from_secs(timeouts::PROFILE_SECS);
const SEARCH_TIMEOUT: Duration = Duration::from_secs(timeouts::SEARCH_SECS);
const HISTORY_TIMEOUT: Duration = Duration::from_secs(timeouts::HISTORY_SECS);
const MUTATE_TIMEOUT: Duration = Duration::from_secs(timeouts::MUTATE_SECS);
const UPDATE_CHECK_TIMEOUT: Duration = Duration::from_secs(timeouts::UPDATE_CHECK_SECS);
const GC_TIMEOUT: Duration = Duration::from_secs(timeouts::GC_SECS);

/// Rotate the target user's flake registry JSON before `nix registry`
/// rewrites it (CLI-changed file): root writes the backup but chowns it
/// back to the user. Default XDG path — a custom `XDG_CONFIG_HOME` set
/// in the login profile is not resolved.
async fn backup_registry(user: &str, copies: usize) -> Result<(), ApiError> {
    let (home, uid, gid) = crate::modules::theming::target_identity(user)?;
    let target = home.join(".config/nix/registry.json");
    crate::backup_configs::rotate_async(target, copies, Some((uid, gid))).await
}

fn repo_is_custom(flake: &str) -> bool {
    flake != "nixpkgs"
}

/// Prefix bare search installables with the flake that produced them
/// so `nix profile add` resolves against the same registry entry.
fn prefix_installables(results: &mut [SearchResult], flake: &str) {
    for result in results.iter_mut() {
        if let Some(installable) = &result.installable {
            if !installable.contains('#') {
                result.installable = Some(format!("{flake}#{installable}"));
            }
        }
    }
}

/// `nix registry list` → rows (`user   flake:NAME ref` / `global …`).
async fn list_repositories(user: &str) -> Result<Vec<NixRepository>, ApiError> {
    let program = exec::nix_path()?;
    let out = exec::run_as_user_argv(user, program, &["registry", "list"], MUTATE_TIMEOUT).await?;
    Ok(parse_registry_list(&out.stdout))
}

fn parse_registry_list(output: &str) -> Vec<NixRepository> {
    output
        .lines()
        .filter_map(|line| {
            // Shapes seen in the wild:
            //   user   flake:nixpkgs github:…
            //   global flake:nixpkgs github:…
            //   flake:nixpkgs github:…            (no scope column)
            //   *user  flake:nixpkgs …            (active marker)
            let mut fields = line.split_whitespace().collect::<Vec<_>>().into_iter();
            let first = fields.next()?.trim_start_matches('*');
            let (scope, first) = if first == "user" || first == "global" {
                (first.to_string(), fields.next()?)
            } else {
                // Older/other nix builds omit the scope column: those
                // rows are still visible (and removable) as "default".
                ("default".to_string(), first)
            };
            let name = first.strip_prefix("flake:")?;
            let url = fields.collect::<Vec<_>>().join(" ");
            if url.is_empty() || name.is_empty() {
                return None;
            }
            Some(NixRepository {
                name: name.to_string(),
                url,
                scope,
            })
        })
        .collect()
}

/// Registry names: flake-ref name charset, no leading `-`.
fn validate_repo_name(name: &str) -> Result<(), ApiError> {
    if name.is_empty()
        || name.len() > 64
        || name.starts_with('-')
        || !name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-' | '+'))
    {
        return Err(ApiError::invalid(
            "invalid repository name (letters, digits, . _ - +)",
        ));
    }
    Ok(())
}

/// Flake references (`github:owner/repo`, `git+https://…`, …): no
/// whitespace or control characters, no leading `-`.
fn validate_repo_url(url: &str) -> Result<(), ApiError> {
    let ok = !url.is_empty()
        && url.len() <= 256
        && !url.starts_with('-')
        && url.chars().all(|c| {
            c.is_ascii_alphanumeric()
                || matches!(
                    c,
                    ':' | '/' | '.' | '_' | '-' | '+' | '~' | '%' | '#' | '?' | '=' | '@' | '&'
                )
        });
    if ok {
        Ok(())
    } else {
        Err(ApiError::invalid(
            "invalid flake reference (e.g. github:owner/repo)",
        ))
    }
}

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

fn line_list(outcome: &CommandOutcome) -> Vec<String> {
    outcome
        .stdout
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(generations::strip_ansi)
        .collect()
}

/// Older nix builds predate `upgrade --dry-run`: the flag is rejected
/// instantly (before any network), so map it to an actionable
/// Unavailable instead of a raw "command failed".
fn dry_run_unsupported(message: &str) -> bool {
    message.contains("dry-run")
        && [
            // Both spellings: nix 2.31.5 (verified on the reference VM)
            // prints "unrecognised flag", others use "unrecognized".
            "unrecognized flag",
            "unrecognised flag",
            "unknown flag",
            "unrecognized option",
            "unrecognised option",
            "unknown option",
            "unrecognized argument",
            "unrecognised argument",
        ]
        .iter()
        .any(|needle| message.contains(needle))
}

/// Names from `nix profile upgrade --all --dry-run`: nix logs
/// `upgrading 'hello' from flake 'A' to 'B'` (stderr); warnings about
/// locked/flake-less packages never match.
fn parse_upgradable(outcome: &CommandOutcome) -> Vec<String> {
    let mut packages: Vec<String> = Vec::new();
    for line in outcome.stdout.lines().chain(outcome.stderr.lines()) {
        let Some(rest) = line.split("upgrading '").nth(1) else {
            continue;
        };
        let Some(name) = rest.split('\'').next() else {
            continue;
        };
        let name = name.trim();
        if !name.is_empty() && !packages.iter().any(|existing| existing == name) {
            packages.push(name.to_string());
        }
    }
    packages
}

fn lines(outcome: &CommandOutcome) -> serde_json::Value {
    json!({ "lines": line_list(outcome) })
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

/// Copy borrowed flag/value pairs into owned form for a job closure.
fn pairs(args: &[(&str, &str)]) -> Vec<(String, String)> {
    args.iter()
        .map(|(flag, value)| ((*flag).to_string(), (*value).to_string()))
        .collect()
}

/// Start a profile job that runs `nix <flag/value pairs>` as the target user.
fn start_profile_job(
    ctx: &ModuleCtx<'_>,
    action: &'static str,
    args: Vec<(String, String)>,
    timeout: Duration,
) -> Result<serde_json::Value, ApiError> {
    let user = ctx.as_user.to_string();
    let started = ctx.jobs.start(
        ModuleId::PackagesNix,
        action,
        ctx.principal,
        Some(ctx.as_user),
        move |sink: Arc<JobSink>| async move {
            let refs: Vec<(String, String)> = args;
            let borrowed: Vec<(&str, &str)> = refs
                .iter()
                .map(|(flag, value)| (flag.as_str(), value.as_str()))
                .collect();
            exec::run_as_user_nix_log(&user, &borrowed, timeout, sink).await
        },
    )?;
    Ok(json!(started))
}

/// Start a profile job that runs an explicit argv as the target user.
fn start_argv_job(
    ctx: &ModuleCtx<'_>,
    action: &'static str,
    program: &'static str,
    argv: Vec<String>,
    timeout: Duration,
) -> Result<serde_json::Value, ApiError> {
    let user = ctx.as_user.to_string();
    let started = ctx.jobs.start(
        ModuleId::PackagesNix,
        action,
        ctx.principal,
        Some(ctx.as_user),
        move |sink: Arc<JobSink>| async move {
            let refs: Vec<&str> = argv.iter().map(String::as_str).collect();
            exec::run_as_user_argv_log(&user, program, &refs, timeout, sink).await
        },
    )?;
    Ok(json!(started))
}

/// True when the profile symlink no longer points at `generation` (a
/// concurrent rollback raced the job).
fn profile_changed(profile_link: &std::path::Path, generation: u32) -> bool {
    std::fs::read_link(profile_link)
        .ok()
        .and_then(|link| generations::parse_generation_link(&link.to_string_lossy()))
        .is_some_and(|now| now != generation)
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
        Action::Search { query, repo } => {
            exec::validate_query(query)?;
            let flake = match repo {
                Some(repo) => {
                    validate_repo_name(repo)?;
                    repo.clone()
                }
                None => "nixpkgs".to_string(),
            };
            let out = exec::run_as_user_search(
                user,
                &[
                    ("search", ""),
                    (flake.as_str(), ""),
                    (query, ""),
                    ("--json", ""),
                ],
                SEARCH_TIMEOUT,
            )
            .await?;
            let mut page = parse::parse_search(&out.stdout)?;
            // Bare installables must resolve against the searched flake.
            if repo_is_custom(&flake) {
                prefix_installables(&mut page.results, &flake);
            }
            Ok(json!({
                "results": page.results,
                "truncated": page.truncated,
            }))
        }
        Action::ListNixRepositories => {
            Ok(json!({"repositories": list_repositories(ctx.as_user).await?}))
        }
        Action::AddNixRepository { name, url } => {
            validate_repo_name(name)?;
            validate_repo_url(url)?;
            backup_registry(ctx.as_user, crate::backup_configs::copies_from(&ctx.config)).await?;
            let program = exec::nix_path()?;
            exec::run_as_user_argv(
                ctx.as_user,
                program,
                &["registry", "add", name, url],
                MUTATE_TIMEOUT,
            )
            .await?;
            Ok(json!({"repositories": list_repositories(ctx.as_user).await?}))
        }
        Action::RemoveNixRepository { name } => {
            validate_repo_name(name)?;
            backup_registry(ctx.as_user, crate::backup_configs::copies_from(&ctx.config)).await?;
            let program = exec::nix_path()?;
            exec::run_as_user_argv(
                ctx.as_user,
                program,
                &["registry", "remove", name],
                MUTATE_TIMEOUT,
            )
            .await?;
            Ok(json!({"repositories": list_repositories(ctx.as_user).await?}))
        }
        Action::Install { package } => {
            let installable = normalize_installable(package)?;
            start_profile_job(
                ctx,
                "install",
                pairs(&[("profile", ""), ("add", ""), (installable.as_str(), "")]),
                MUTATE_TIMEOUT,
            )
        }
        Action::Remove { package } => {
            validate_installable(package)?;
            start_profile_job(
                ctx,
                "remove",
                pairs(&[("profile", ""), ("remove", ""), (package, "")]),
                MUTATE_TIMEOUT,
            )
        }
        Action::RemoveAll => start_profile_job(
            ctx,
            "remove_all",
            pairs(&[("profile", ""), ("remove", ""), ("--all", "")]),
            MUTATE_TIMEOUT,
        ),
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
            let mut argv: Vec<String> = vec!["profile".into(), "remove".into()];
            argv.extend(names);
            start_argv_job(ctx, "remove_regex", "nix", argv, MUTATE_TIMEOUT)
        }
        Action::Upgrade { package, all } => {
            if *all {
                start_profile_job(
                    ctx,
                    "upgrade",
                    pairs(&[("profile", ""), ("upgrade", ""), ("--all", "")]),
                    MUTATE_TIMEOUT,
                )
            } else {
                let package = package
                    .as_deref()
                    .ok_or_else(|| ApiError::invalid("package required when all=false"))?;
                validate_installable(package)?;
                start_profile_job(
                    ctx,
                    "upgrade",
                    pairs(&[("profile", ""), ("upgrade", ""), (package, "")]),
                    MUTATE_TIMEOUT,
                )
            }
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
            let mut argv: Vec<String> = vec!["profile".into(), "upgrade".into()];
            argv.extend(names);
            start_argv_job(ctx, "upgrade_regex", "nix", argv, MUTATE_TIMEOUT)
        }
        Action::ListNixUpdates => {
            // On-demand and slow by design (re-fetches every unlocked
            // flake with tarballTtl=0): never automatic without opt-in.
            let out = exec::run_as_user(
                user,
                &[
                    ("profile", ""),
                    ("upgrade", ""),
                    ("--all", ""),
                    ("--dry-run", ""),
                ],
                UPDATE_CHECK_TIMEOUT,
            )
            .await
            .map_err(|error| {
                if dry_run_unsupported(&error.message) {
                    ApiError::unavailable(
                        "this nix is too old: `profile upgrade` has no --dry-run flag",
                        "upgrade nix — the update check needs the dry-run flag",
                    )
                } else {
                    error
                }
            })?;
            let packages = parse_upgradable(&out);
            Ok(serde_json::json!(ursine_api::NixUpdates {
                count: packages.len(),
                packages,
            }))
        }
        Action::Generations => {
            let out = exec::run_as_user(user, &[("profile", ""), ("history", "")], HISTORY_TIMEOUT)
                .await?;
            // Newest generation first for every frontend.
            Ok(json!({
                "lines": generations::reorder_newest_first(line_list(&out))
            }))
        }
        Action::Rollback { generation } => {
            let gen_str = generation.map(|g| g.to_string());
            let mut args: Vec<(&str, &str)> = vec![("profile", ""), ("rollback", "")];
            if let Some(gen) = gen_str.as_deref() {
                args.push(("--to", gen));
            }
            start_profile_job(ctx, "rollback", pairs(&args), MUTATE_TIMEOUT)
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
                let out = exec::run_as_user(user, &args, MUTATE_TIMEOUT).await?;
                return Ok(json!(out));
            }
            start_profile_job(ctx, "wipe_history", pairs(&args), MUTATE_TIMEOUT)
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
            let started = ctx.jobs.start(
                ModuleId::PackagesNix,
                "store_gc",
                ctx.principal,
                None,
                move |sink: Arc<JobSink>| async move {
                    exec::run_root_program_log(program, &["store", "gc"], GC_TIMEOUT, sink).await
                },
            )?;
            Ok(json!(started))
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
            let owner = user.to_string();
            let link = profile_ref.to_path_buf();
            let started = ctx.jobs.start(
                ModuleId::PackagesNix,
                "prune_generations",
                ctx.principal,
                Some(ctx.as_user),
                move |sink: Arc<JobSink>| async move {
                    // Re-validate at execution time: the profile may have
                    // moved between dispatch-time validation and this job.
                    if profile_changed(&link, fresh_current) {
                        return Err(ApiError::new(
                            ErrorCode::Conflict,
                            "profile changed while pruning; retry",
                        ));
                    }
                    let refs: Vec<&str> = argv.iter().map(String::as_str).collect();
                    let result =
                        exec::run_as_user_argv_log(&owner, "nix-env", &refs, MUTATE_TIMEOUT, sink)
                            .await;
                    // Classify a concurrent rollback from the fresh link, not
                    // from nix-env wording.
                    let conflict =
                        ApiError::new(ErrorCode::Conflict, "profile changed while pruning; retry");
                    match result {
                        Ok(0) => Ok(0),
                        Ok(code) => Err(if profile_changed(&link, fresh_current) {
                            conflict
                        } else {
                            ApiError::internal(format!("nix-env exited {code}"))
                        }),
                        Err(error) => Err(if profile_changed(&link, fresh_current) {
                            conflict
                        } else {
                            error
                        }),
                    }
                },
            )?;
            Ok(json!(started))
        }
        _ => Err(ApiError::invalid(
            "action is not valid for module packages_nix",
        )),
    }
}

#[cfg(test)]
mod tests {

    #[test]
    fn classifies_missing_dry_run_flag() {
        // Verbatim from `nix profile upgrade --all --dry-run` on the
        // reference VM (nix 2.31.5): British spelling.
        assert!(dry_run_unsupported(
            "error: unrecognised flag '--dry-run'\nTry 'nix --help' for more information."
        ));
        assert!(dry_run_unsupported("error: unrecognized flag '--dry-run'"));
        assert!(dry_run_unsupported("nix: unknown option: --dry-run"));
        // Real failures keep their own error.
        assert!(!dry_run_unsupported("error: connection refused"));
        assert!(!dry_run_unsupported("unrecognized flag 'other'"));
    }

    #[test]
    fn parse_upgradable_reads_dry_run_log_lines() {
        let outcome = CommandOutcome {
            stdout: String::new(),
            stderr: concat!(
                "warning: package 'x' not from a flake\n",
                "upgrading 'hello' from flake 'nixpkgs' to 'github:a/b'\n",
                "upgrading 'gh' from flake 'nixpkgs' to 'github:c/d'\n",
                "upgrading 'hello' from flake 'nixpkgs' to 'github:a/e'\n",
            )
            .to_string(),
        };
        assert_eq!(parse_upgradable(&outcome), ["hello", "gh"]);
        assert!(parse_upgradable(&CommandOutcome {
            stdout: String::new(),
            stderr: String::new(),
        })
        .is_empty());
    }

    use super::*;

    #[test]
    fn parses_registry_list() {
        let out = "user   flake:nixpkgs github:NixOS/nixpkgs/nixos-26.05\n\
global flake:agenix github:ryantm/agenix\nweird line\n";
        let rows = parse_registry_list(out);
        assert_eq!(rows.len(), 2, "{rows:?}");
        assert_eq!(rows[0].name, "nixpkgs");
        assert_eq!(rows[0].url, "github:NixOS/nixpkgs/nixos-26.05");
        assert_eq!(rows[0].scope, "user");
        assert_eq!(rows[1].scope, "global");
        assert_eq!(rows[1].name, "agenix");
    }

    #[test]
    fn parses_registry_real_reference_vm_output() {
        // Verbatim shape from `nix registry list` on nix 2.31.5
        // (reference VM): scope column present, duplicate names across
        // scopes, slash names, URL with query string.
        let out = "user   flake:nixpkgs github:NixOS/nixpkgs/nixos-26.05\n\
user   flake:ursine github:skea999/ursine\n\
global flake:nixpkgs https://channels.nixos.org/nixpkgs-unstable/nixexprs.tar.zst\n\
global flake:nixpkgs/nixos-26.05 https://channels.nixos.org/nixos-26.05/nixexprs.tar.zst\n\
global flake:blender-bin github:edolstra/nix-warez?dir=blender\n";
        let rows = parse_registry_list(out);
        assert_eq!(rows.len(), 5, "{rows:?}");
        assert_eq!(rows[0].scope, "user");
        assert_eq!(rows[0].name, "nixpkgs");
        assert_eq!(rows[1].name, "ursine");
        assert_eq!(rows[2].scope, "global");
        assert!(rows[2].url.starts_with("https://channels"));
        assert_eq!(rows[3].name, "nixpkgs/nixos-26.05");
        assert!(rows[4].url.contains("?dir=blender"));
        // The default the user manages is a user row → removable scope.
        assert_eq!(rows[0].scope, "user");
    }

    #[test]
    fn parses_registry_without_scope_column_and_markers() {
        // Older/other nix builds print no scope; an active marker may
        // prefix the scope. Both must still show up (as "default").
        let out = "flake:nixpkgs github:NixOS/nixpkgs\n\
*user flake:agenix github:ryantm/agenix\n";
        let rows = parse_registry_list(out);
        assert_eq!(rows.len(), 2, "{rows:?}");
        assert_eq!(rows[0].scope, "default");
        assert_eq!(rows[0].name, "nixpkgs");
        assert_eq!(rows[1].scope, "user");
        assert_eq!(rows[1].name, "agenix");
    }

    #[test]
    fn prefixes_search_installables_with_the_custom_flake() {
        let mut results = vec![
            SearchResult {
                attr: "legacyPackages.x86_64-linux.hello".into(),
                installable: Some("hello".into()),
                pname: None,
                version: None,
                description: None,
            },
            SearchResult {
                attr: "packages.x86_64-linux.foo".into(),
                installable: Some("myflake#foo".into()),
                pname: None,
                version: None,
                description: None,
            },
        ];
        prefix_installables(&mut results, "myflake");
        assert_eq!(results[0].installable.as_deref(), Some("myflake#hello"));
        assert_eq!(results[1].installable.as_deref(), Some("myflake#foo"));
        // Default flake keeps bare installables (nixpkgs# normalization
        // happens at install time).
        assert!(!repo_is_custom("nixpkgs"));
        assert!(repo_is_custom("myflake"));
    }

    #[test]
    fn validates_registry_inputs() {
        assert!(validate_repo_name("my-flake").is_ok());
        assert!(validate_repo_name("edge2").is_ok());
        assert!(validate_repo_name("-edge").is_err());
        assert!(validate_repo_name("bad name").is_err());
        assert!(validate_repo_url("github:NixOS/nixpkgs").is_ok());
        assert!(validate_repo_url("git+https://example.com/repo?ref=x").is_ok());
        assert!(validate_repo_url("").is_err());
        assert!(validate_repo_url("has space").is_err());
        assert!(validate_repo_url("-rf").is_err());
    }

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
