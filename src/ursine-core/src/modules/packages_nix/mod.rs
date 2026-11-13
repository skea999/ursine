//! packages.nix — nix profiles via the NEW CLI (`nix profile ...`).
//!
//! Flake refs only (`nixpkgs#pkg` or `flake#attr`). Classic channels/`nix-env`
//! are not supported by design (skea999: only the latest mode matters).
//! Every command runs AS the target user (login env, own profile), never root.

pub mod exec;
mod generations;
mod parse;

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

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

const GLOBAL_REGISTRY: &str = "/etc/nix/registry.json";

/// Drop one entry from the SYSTEM registry. `nix registry remove`
/// only edits the calling user's file, so the daemon edits the system
/// file the Webmin way: read, rotate the backup, atomic replace.
async fn remove_global_repository(name: &str, copies: usize) -> Result<(), ApiError> {
    // Cap BEFORE reading: the limit guards allocation, not just parse.
    let meta = std::fs::metadata(GLOBAL_REGISTRY)
        .map_err(|e| ApiError::internal(format!("stat {GLOBAL_REGISTRY}: {e}")))?;
    if meta.len() > 1024 * 1024 {
        return Err(ApiError::internal(
            "system registry file is larger than 1 MiB",
        ));
    }
    let content = std::fs::read_to_string(GLOBAL_REGISTRY)
        .map_err(|e| ApiError::internal(format!("read {GLOBAL_REGISTRY}: {e}")))?;
    let mut value: serde_json::Value = serde_json::from_str(&content)
        .map_err(|e| ApiError::internal(format!("parse {GLOBAL_REGISTRY}: {e}")))?;
    if !drop_global_flake(&mut value, name)? {
        // Nothing there — removing is idempotent.
        return Ok(());
    }
    let mut updated = serde_json::to_string_pretty(&value)
        .map_err(|e| ApiError::internal(format!("encode {GLOBAL_REGISTRY}: {e}")))?;
    updated.push('\n');
    exec::run_as_user_atomic_write("root", GLOBAL_REGISTRY, &updated, copies).await?;
    Ok(())
}

/// Remove one `type = indirect` flake from a parsed registry document
/// (a same-id entry of another type is never touched). Returns whether
/// anything was dropped. A present-but-wrong-typed `flakes` key is a
/// diagnosed error — never a silent success.
fn drop_global_flake(value: &mut serde_json::Value, name: &str) -> Result<bool, ApiError> {
    let Some(raw) = value.get_mut("flakes") else {
        return Ok(false);
    };
    let Some(flakes) = raw.as_array_mut() else {
        return Err(ApiError::internal("registry file: flakes is not an array"));
    };
    let before = flakes.len();
    flakes.retain(|entry| {
        let from = entry.get("from");
        let is_target = from
            .and_then(|from| from.get("id"))
            .and_then(serde_json::Value::as_str)
            == Some(name)
            && from
                .and_then(|from| from.get("type"))
                .and_then(serde_json::Value::as_str)
                .unwrap_or("indirect")
                == "indirect";
        !is_target
    });
    Ok(flakes.len() != before)
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

/// A lock we can compare: source content hash or commit id.
#[derive(Debug, Clone, PartialEq, Eq)]
enum FlakeLock {
    Nar(String),
    Rev(String),
}

/// The current locks of one flake reference (either may be missing).
#[derive(Debug, Default, PartialEq, Eq)]
struct MetaLock {
    nar: Option<String>,
    rev: Option<String>,
}

/// Percent-decode a query value (`%2B` → `+`): flake URLs encode the
/// narHash parameter this way.
fn percent_decode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0usize;
    while let Some(&byte) = bytes.get(index) {
        if byte == b'%' {
            let pair = bytes.get(index + 1..index + 3).and_then(|pair| {
                let hex = std::str::from_utf8(pair).ok()?;
                u8::from_str_radix(hex, 16).ok()
            });
            if let Some(decoded) = pair {
                out.push(decoded);
                index += 3;
                continue;
            }
        }
        out.push(byte);
        index += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Commit-shaped hex segment (nix revisions are 40 hex chars; short
/// forms appear in hand-written flake refs).
fn is_rev(value: &str) -> bool {
    (7..=40).contains(&value.len()) && value.chars().all(|c| c.is_ascii_hexdigit())
}

/// A profile element's `url` → the lock it is pinned to: the `narHash`
/// query parameter first (what `nix profile list --json` emits), then a
/// `rev=` query parameter, then a hex path segment. `None` means the
/// element carries no comparable lock — never guess.
fn element_lock(url: &str) -> Option<FlakeLock> {
    let (path, query) = url.split_once('?').unwrap_or((url, ""));
    for pair in query.split('&') {
        if let Some(value) = pair.strip_prefix("narHash=") {
            let nar = percent_decode(value);
            if !nar.is_empty() {
                return Some(FlakeLock::Nar(nar));
            }
        }
    }
    for pair in query.split('&') {
        if let Some(value) = pair.strip_prefix("rev=") {
            if is_rev(value) {
                return Some(FlakeLock::Rev(value.to_string()));
            }
        }
    }
    let last = path.rsplit('/').next().unwrap_or("");
    if is_rev(last) {
        Some(FlakeLock::Rev(last.to_string()))
    } else {
        None
    }
}

/// `nix flake metadata <ref> --json` → comparable locks (tolerant:
/// a missing field or an unparsable document yields `Default`, and the
/// caller then reports nothing for that ref instead of failing).
fn meta_locks(json: &str) -> MetaLock {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(json) else {
        return MetaLock::default();
    };
    MetaLock {
        nar: value
            .pointer("/locked/narHash")
            .and_then(serde_json::Value::as_str)
            .map(str::to_string),
        rev: value
            .get("revision")
            .and_then(serde_json::Value::as_str)
            .map(str::to_string),
    }
}

/// True only when the element's lock kind exists in the metadata and
/// the values differ. A missing matching field means "cannot tell" —
/// never report an update we cannot prove.
fn outdatable(lock: &FlakeLock, meta: &MetaLock) -> bool {
    match lock {
        FlakeLock::Nar(nar) => meta.nar.as_ref().is_some_and(|current| current != nar),
        FlakeLock::Rev(rev) => meta.rev.as_ref().is_some_and(|current| current != rev),
    }
}

/// Element names whose pinned lock differs from the current metadata
/// lock of their original flake reference (profile order, unique).
fn outdated_names(elements: &[InstalledPackage], metas: &HashMap<String, MetaLock>) -> Vec<String> {
    let mut packages: Vec<String> = Vec::new();
    for element in elements {
        let (Some(original), Some(url)) = (&element.original_url, &element.url) else {
            continue;
        };
        let Some(lock) = element_lock(url) else {
            continue;
        };
        let Some(meta) = metas.get(original) else {
            continue;
        };
        if outdatable(&lock, meta) && !packages.contains(&element.name) {
            packages.push(element.name.clone());
        }
    }
    packages
}

/// Flake refs embedded in `getFlake "…"`: no quote/backslash/`$`
/// (Nix interpolation) or control chars.
fn nix_string_safe(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 512
        && !value
            .chars()
            .any(|c| matches!(c, '"' | '\\' | '$' | '\'' | '\n' | '\r' | '\0'))
}

/// Profile element names used as eval-result keys (embedded raw).
fn eval_key_safe(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '+'))
}

/// One attr-path segment of `attrPath` (dot-joined in the profile).
fn attr_segment_safe(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
}

/// One `nix eval` expression resolving `meta.description` for every
/// valid target of a single flake reference. Names and path segments
/// are validated before they are embedded raw in the expression.
fn description_expr(flake_ref: &str, targets: &[(String, Vec<String>)]) -> Option<String> {
    if !nix_string_safe(flake_ref) {
        return None;
    }
    let mut items = String::new();
    for (name, path) in targets {
        if !eval_key_safe(name)
            || path.is_empty()
            || !path.iter().all(|segment| attr_segment_safe(segment))
        {
            continue;
        }
        if !items.is_empty() {
            items.push(' ');
        }
        items.push_str("{ name = \"");
        items.push_str(name);
        items.push_str("\"; path = [ ");
        for segment in path {
            items.push('"');
            items.push_str(segment);
            items.push_str("\" ");
        }
        items.push_str("]; }");
    }
    if items.is_empty() {
        return None;
    }
    Some(format!(
        "let f = builtins.getFlake \"{flake_ref}\"; in builtins.listToAttrs (map (t: {{ \
        name = t.name; value = let r = builtins.tryEval (builtins.getAttrFromPath \
        (t.path ++ [ \"meta\" \"description\" ]) f); in if r.success then r.value else null; }}) \
        [ {items} ])"
    ))
}

/// `nix eval --json` object → name → description (nulls dropped).
fn parse_descriptions(json: &str) -> HashMap<String, String> {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(json) else {
        return HashMap::new();
    };
    let Some(object) = value.as_object() else {
        return HashMap::new();
    };
    object
        .iter()
        .filter_map(|(name, description)| {
            let description = description.as_str()?;
            if description.is_empty() {
                None
            } else {
                Some((name.clone(), description.to_string()))
            }
        })
        .collect()
}

/// Best-effort `meta.description` lookup: group the profile by flake
/// reference, one eval per group. Every failure leaves the list as it
/// is — descriptions are decoration, the profile is the data.
async fn attach_descriptions(user: &str, packages: &mut [InstalledPackage], deadline: Instant) {
    let mut groups: HashMap<String, Vec<(usize, String, Vec<String>)>> = HashMap::new();
    for (index, package) in packages.iter().enumerate() {
        let (Some(original), Some(attr)) = (&package.original_url, &package.attr_path) else {
            continue;
        };
        if !nix_string_safe(original) || !eval_key_safe(&package.name) {
            continue;
        }
        let path: Vec<String> = attr.split('.').map(str::to_string).collect();
        if !path.iter().all(|segment| attr_segment_safe(segment)) {
            continue;
        }
        groups
            .entry(original.clone())
            .or_default()
            .push((index, package.name.clone(), path));
    }
    for (original, targets) in groups {
        // Leftover time only: the shared deadline caps the whole
        // description phase, whatever the group count.
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining < Duration::from_secs(1) {
            break;
        }
        let refs: Vec<(String, Vec<String>)> = targets
            .iter()
            .map(|(_, name, path)| (name.clone(), path.clone()))
            .collect();
        let Some(expr) = description_expr(&original, &refs) else {
            continue;
        };
        let Ok(outcome) = exec::run_as_user_json(
            user,
            &[
                ("eval", ""),
                ("--impure", ""),
                ("--json", ""),
                ("--expr", expr.as_str()),
            ],
            remaining,
        )
        .await
        else {
            continue;
        };
        let descriptions = parse_descriptions(&outcome.stdout);
        for (index, name, _) in targets {
            if let Some(description) = descriptions.get(&name) {
                if let Some(package) = packages.get_mut(index) {
                    package.description = Some(description.clone());
                }
            }
        }
    }
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
            // ONE budget for list + descriptions: the client's profile
            // timeout covers the whole handler, so a slow eval can only
            // ever eat leftover time — never push the reply over it.
            let deadline = Instant::now() + LIST_TIMEOUT;
            let out = exec::run_as_user(
                user,
                &[("profile", ""), ("list", ""), ("--json", "")],
                deadline.saturating_duration_since(Instant::now()),
            )
            .await?;
            let mut packages = parse::parse_profile_list(&out.stdout)?;
            // Best-effort descriptions: one `nix eval` per flake ref.
            // A slow or failing eval must never fail the profile list.
            attach_descriptions(user, &mut packages, deadline).await;
            Ok(json!(packages))
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
            let out = exec::run_as_user_json(
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
        Action::RemoveNixRepository { name, scope } => {
            validate_repo_name(name)?;
            let scope = scope.as_deref().unwrap_or("user");
            if !matches!(scope, "user" | "default" | "global") {
                return Err(ApiError::invalid("invalid repository scope"));
            }
            let copies = crate::backup_configs::copies_from(&ctx.config);
            if scope == "global" {
                // `nix registry remove` only edits the calling user's
                // file: the system registry is edited directly
                // (Webmin pattern), with the uniform backup rotation.
                remove_global_repository(name, copies).await?;
            } else {
                backup_registry(ctx.as_user, copies).await?;
                let program = exec::nix_path()?;
                exec::run_as_user_argv(
                    ctx.as_user,
                    program,
                    &["registry", "remove", name],
                    MUTATE_TIMEOUT,
                )
                .await?;
            }
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
            // Hand-tested on the reference VM (nix 2.31.5): `profile
            // upgrade` has NO --dry-run flag, so the check compares each
            // element's pinned lock with the CURRENT lock of its
            // original flake reference (`nix flake metadata --json`).
            // On-demand and network-bound: never automatic without opt-in.
            let out = exec::run_as_user_json(
                user,
                &[("profile", ""), ("list", ""), ("--json", "")],
                UPDATE_CHECK_TIMEOUT,
            )
            .await?;
            let elements = parse::parse_profile_list(&out.stdout)?;
            let mut metas: HashMap<String, MetaLock> = HashMap::new();
            for element in &elements {
                let (Some(original), Some(url)) = (&element.original_url, &element.url.as_deref())
                else {
                    continue;
                };
                // One metadata fetch per unique reference, and only for
                // elements that actually carry a comparable lock.
                if metas.contains_key(original) || element_lock(url).is_none() {
                    continue;
                }
                // The ref rides a single-quoted VALUE after `--` (so
                // `&`/`?` queries survive; quotes/newlines do not) —
                // skip what that guard refuses instead of failing the
                // whole check.
                if !exec::is_safe_quoted_value(original) {
                    continue;
                }
                let meta = exec::run_as_user_json(
                    user,
                    &[
                        ("flake", ""),
                        ("metadata", ""),
                        ("--json", ""),
                        ("--", original.as_str()),
                    ],
                    UPDATE_CHECK_TIMEOUT,
                )
                .await?;
                metas.insert(original.clone(), meta_locks(&meta.stdout));
            }
            let packages = outdated_names(&elements, &metas);
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
    fn element_lock_prefers_percent_encoded_narhash() {
        // Verbatim element url from `nix profile list --json` on the
        // reference VM (nix 2.31.5).
        let url = "github:NixOS/nixpkgs/7dcdd87373d72e7a2173e4db02e20d3d7f495cfd\
            ?narHash=sha256-tItrb9M8eYA6WhFgmT9tpGKueri3%2BwRuVA952o/2x";
        assert_eq!(
            element_lock(url),
            Some(FlakeLock::Nar(
                "sha256-tItrb9M8eYA6WhFgmT9tpGKueri3+wRuVA952o/2x".to_string()
            ))
        );
    }

    #[test]
    fn element_lock_falls_back_to_rev_and_none_without_lock() {
        assert_eq!(
            element_lock("github:a/b/13d0c311e3ae923a00f734b43fd1d35b47d8943a"),
            Some(FlakeLock::Rev(
                "13d0c311e3ae923a00f734b43fd1d35b47d8943a".to_string()
            ))
        );
        assert_eq!(
            element_lock("git+https://example.com/x/y?rev=abcdef1"),
            Some(FlakeLock::Rev("abcdef1".to_string()))
        );
        assert_eq!(element_lock("flake:nixpkgs#hello"), None);
    }

    #[test]
    fn meta_locks_read_locked_narhash_and_revision() {
        // Field shape from `nix flake metadata flake:nixpkgs --json`.
        let meta = meta_locks(
            "{\"locked\":{\"narHash\":\"sha256-XuAV2cUwaod3Bt4XFVqiXz3v1w2ex16W6Sv4S7EWres=\"},\
              \"revision\":\"ac588ef1c914865dad517b337ea0eb9694a4021c\"}",
        );
        assert_eq!(
            meta.nar.as_deref(),
            Some("sha256-XuAV2cUwaod3Bt4XFVqiXz3v1w2ex16W6Sv4S7EWres=")
        );
        assert_eq!(
            meta.rev.as_deref(),
            Some("ac588ef1c914865dad517b337ea0eb9694a4021c")
        );
        // Garbage never invents a lock.
        assert_eq!(meta_locks("not json"), MetaLock::default());
    }

    #[test]
    fn outdatable_requires_a_matching_lock_kind() {
        let meta = MetaLock {
            nar: Some("sha256-b".to_string()),
            rev: Some("aaaaaaa".to_string()),
        };
        assert!(outdatable(&FlakeLock::Nar("sha256-a".to_string()), &meta));
        assert!(!outdatable(&FlakeLock::Nar("sha256-b".to_string()), &meta));
        assert!(outdatable(&FlakeLock::Rev("bbbbbbb".to_string()), &meta));
        assert!(!outdatable(&FlakeLock::Rev("aaaaaaa".to_string()), &meta));
        // No matching field on the metadata side → cannot tell.
        assert!(!outdatable(
            &FlakeLock::Nar("sha256-a".to_string()),
            &MetaLock::default()
        ));
    }

    #[test]
    fn outdated_names_flags_only_proven_changes() {
        let element = |name: &str, url: Option<&str>| InstalledPackage {
            name: name.to_string(),
            attr_path: None,
            original_url: Some("flake:nixpkgs".to_string()),
            url: url.map(str::to_string),
            store_paths: Vec::new(),
            active: true,

            description: None,
        };
        let elements = vec![
            element(
                "bleachbit",
                Some(
                    "github:NixOS/nixpkgs/7dcdd87373d72e7a2173e4db02e20d3d7f495cfd\
                      ?narHash=sha256-old",
                ),
            ),
            element(
                "fresh",
                Some(
                    "github:NixOS/nixpkgs/ac588ef1c914865dad517b337ea0eb9694a4021c\
                      ?narHash=sha256-new",
                ),
            ),
            element("no-lock", Some("flake:nixpkgs#hello")),
            element("no-url", None),
        ];
        let mut metas = HashMap::new();
        metas.insert(
            "flake:nixpkgs".to_string(),
            MetaLock {
                nar: Some("sha256-new".to_string()),
                rev: None,
            },
        );
        // Only the element whose narHash differs is flagged; a missing
        // metadata entry (unfetchable ref) flags nothing.
        assert_eq!(outdated_names(&elements, &metas), ["bleachbit"]);
        assert!(outdated_names(&elements, &HashMap::new()).is_empty());
    }

    #[test]
    fn description_expr_embeds_only_validated_targets() {
        let expr = description_expr(
            "flake:nixpkgs",
            &[
                (
                    "bleachbit".to_string(),
                    vec![
                        "legacyPackages".to_string(),
                        "x86_64-linux".to_string(),
                        "bleachbit".to_string(),
                    ],
                ),
                // Invalid key (space) and segment are skipped, not embedded.
                ("bad name".to_string(), vec!["hello".to_string()]),
            ],
        )
        .expect("valid ref");
        assert!(expr.contains("builtins.getFlake \"flake:nixpkgs\""));
        assert!(expr.contains("\"bleachbit\""));
        assert!(expr.contains("\"legacyPackages\""));
        assert!(!expr.contains("bad name"));
        // Refs unsafe inside a Nix string never build an expression.
        assert!(
            description_expr("evil\"${x}", &[("a".to_string(), vec!["b".to_string()])]).is_none()
        );
        assert!(description_expr("flake:nixpkgs", &[]).is_none());
    }

    #[test]
    fn drop_global_flake_targets_only_matching_indirect_entries() {
        let mut value = json!({
            "version": 2,
            "flakes": [
                {"from": {"id": "nixpkgs", "type": "indirect"},
                  "to": {"owner": "NixOS", "repo": "nixpkgs", "type": "github"}},
                {"from": {"id": "ursine", "type": "indirect"},
                  "to": {"owner": "skea999", "repo": "ursine", "type": "github"}},
                {"from": {"id": "nixpkgs", "type": "path", "path": "/x"},
                  "to": {"type": "path", "path": "/y"}}
            ]
        });
        assert!(drop_global_flake(&mut value, "nixpkgs").unwrap());
        let flakes = value
            .get("flakes")
            .and_then(serde_json::Value::as_array)
            .expect("flakes array");
        let ids: Vec<&str> = flakes
            .iter()
            .filter_map(|entry| {
                entry
                    .pointer("/from/id")
                    .and_then(serde_json::Value::as_str)
            })
            .collect();
        // The indirect target went; the same id under another type stayed.
        assert_eq!(ids, ["ursine", "nixpkgs"]);
        assert!(!drop_global_flake(&mut value, "nixpkgs").unwrap());
        let mut no_key = json!({"version": 2});
        assert!(!drop_global_flake(&mut no_key, "nixpkgs").unwrap());
        // Wrong-typed flakes is diagnosed, not swallowed.
        let mut wrong = json!({"flakes": "not an array"});
        assert!(drop_global_flake(&mut wrong, "nixpkgs").is_err());
    }

    #[test]
    fn parse_descriptions_keeps_non_empty_strings_only() {
        let map = parse_descriptions(r#"{"hello":"Hello!","gone":null,"empty":""}"#);
        assert_eq!(map.get("hello").map(String::as_str), Some("Hello!"));
        assert!(!map.contains_key("gone"));
        assert!(!map.contains_key("empty"));
        assert!(parse_descriptions("not json").is_empty());
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

                description: None,
            },
            InstalledPackage {
                name: "hello".into(),
                attr_path: None,
                original_url: None,
                url: None,
                store_paths: Vec::new(),
                active: true,

                description: None,
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
