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

/// One `nix registry list` row with its scope kept for filtering.
#[derive(Debug)]
struct RegistryRow {
    scope: String,
    name: String,
    url: String,
}

/// `nix registry list` → rows.
///
/// Shapes seen in the wild:
///   user   flake:nixpkgs github:…
///   system flake:nixpkgs github:…          (/etc/nix/registry.json)
///   global flake:nixpkgs …                 (builtin: dropped, not listed)
///   *user  flake:nixpkgs …                 (active marker)
/// A line without a scope column (older nix) parses as `user`.
fn parse_registry_list(output: &str) -> Vec<RegistryRow> {
    output
        .lines()
        .filter_map(|line| {
            let mut fields = line.split_whitespace().collect::<Vec<_>>().into_iter();
            let first = fields.next()?.trim_start_matches('*');
            let (scope, first) = if matches!(first, "user" | "system" | "global") {
                (first.to_string(), fields.next()?)
            } else {
                ("user".to_string(), first)
            };
            let name = first.strip_prefix("flake:")?;
            let url = fields.collect::<Vec<_>>().join(" ");
            if url.is_empty() || name.is_empty() {
                return None;
            }
            Some(RegistryRow {
                scope,
                name: name.to_string(),
                url,
            })
        })
        .collect()
}

/// `nix-channel --list` → `name url` rows (one pair per line).
fn parse_channel_list(output: &str) -> Vec<(String, String)> {
    output
        .lines()
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            let name = fields.next()?;
            let url = fields.next()?;
            Some((name.to_string(), url.to_string()))
        })
        .collect()
}

/// Configured repositories for one scope. Builtin `global` defaults are
/// never returned: there is no file to edit and no way for a user to
/// restore one by accident. A channel fills a name the registry does not
/// define, so a `FeralOS` install shows its pinned `nixpkgs` once.
async fn list_repositories(user: &str, system: bool) -> Result<Vec<NixRepository>, ApiError> {
    let wanted = if system { "system" } else { "user" };
    let mut rows: Vec<NixRepository> = registry_rows(user)
        .await?
        .into_iter()
        .filter(|row| row.scope == wanted)
        .map(|row| NixRepository {
            name: row.name,
            url: row.url,
            source: ursine_api::NixRepoSource::Registry,
        })
        .collect();
    for (name, url) in channel_rows(user, system).await? {
        if !rows.iter().any(|row| row.name == name) {
            rows.push(NixRepository {
                name,
                url,
                source: ursine_api::NixRepoSource::Channel,
            });
        }
    }
    Ok(rows)
}

async fn registry_rows(user: &str) -> Result<Vec<RegistryRow>, ApiError> {
    let program = exec::nix_path()?;
    let out = exec::run_as_user_argv(user, program, &["registry", "list"], MUTATE_TIMEOUT).await?;
    Ok(parse_registry_list(&out.stdout))
}

/// Channels belong to one user: root's channel is the system one (the
/// `FeralOS` installer adds it there).
async fn channel_rows(user: &str, system: bool) -> Result<Vec<(String, String)>, ApiError> {
    let program = exec::nix_channel_path()?;
    let out = if system {
        exec::run_root_program(program, &["--list"], MUTATE_TIMEOUT).await?
    } else {
        exec::run_as_user_argv(user, program, &["--list"], MUTATE_TIMEOUT).await?
    };
    Ok(parse_channel_list(&out.stdout))
}

/// Read the system registry, or the empty document when it does not exist
/// yet (fresh install; the `FeralOS` installer creates it with the pin).
fn read_system_registry() -> Result<serde_json::Value, ApiError> {
    let path = std::path::Path::new(GLOBAL_REGISTRY);
    let Ok(meta) = std::fs::symlink_metadata(path) else {
        return Ok(json!({"version": 2, "flakes": []}));
    };
    if meta.file_type().is_symlink() || !meta.is_file() {
        return Err(ApiError::internal("system registry is not a regular file"));
    }
    if meta.len() > 1024 * 1024 {
        return Err(ApiError::internal(
            "system registry file is larger than 1 MiB",
        ));
    }
    let content = std::fs::read_to_string(path)
        .map_err(|e| ApiError::internal(format!("read {GLOBAL_REGISTRY}: {e}")))?;
    serde_json::from_str(&content)
        .map_err(|e| ApiError::internal(format!("parse {GLOBAL_REGISTRY}: {e}")))
}

async fn write_system_registry(value: &serde_json::Value, copies: usize) -> Result<(), ApiError> {
    let mut updated = serde_json::to_string_pretty(value)
        .map_err(|e| ApiError::internal(format!("encode {GLOBAL_REGISTRY}: {e}")))?;
    updated.push('\n');
    exec::run_as_user_atomic_write("root", GLOBAL_REGISTRY, &updated, copies)
        .await
        .map(|_| ())
}

/// `from.id` of an `indirect` registry entry.
fn indirect_id(entry: &serde_json::Value) -> Option<&str> {
    let from = entry.get("from")?;
    if from
        .get("type")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("indirect")
        != "indirect"
    {
        return None;
    }
    from.get("id").and_then(serde_json::Value::as_str)
}

/// Add or replace one `type = indirect` entry in the system registry.
async fn upsert_system_repository(name: &str, url: &str, copies: usize) -> Result<(), ApiError> {
    let target = github_flake_target(url)?;
    let mut value = read_system_registry()?;
    let Some(flakes) = value
        .get_mut("flakes")
        .and_then(serde_json::Value::as_array_mut)
    else {
        return Err(ApiError::internal(
            "system registry: flakes is not an array",
        ));
    };
    let mut replaced = false;
    for entry in flakes.iter_mut() {
        if indirect_id(entry) == Some(name) {
            entry["to"] = target.clone();
            replaced = true;
            break;
        }
    }
    if !replaced {
        flakes.push(json!({
            "from": {"type": "indirect", "id": name},
            "to": target,
        }));
    }
    write_system_registry(&value, copies).await
}

/// System registry entries are written by us, so the accepted URL shape is
/// minimal and exact: `github:owner/repo[/ref]`.
fn github_flake_target(url: &str) -> Result<serde_json::Value, ApiError> {
    let Some(rest) = url.strip_prefix("github:") else {
        return Err(ApiError::invalid(
            "system repositories require a github:owner/repo[/ref] reference",
        ));
    };
    let mut parts = rest.splitn(3, '/');
    let owner = parts.next().unwrap_or_default();
    let repo = parts.next().unwrap_or_default();
    let git_ref = parts.next().filter(|value| !value.is_empty());
    if owner.is_empty() || repo.is_empty() {
        return Err(ApiError::invalid(
            "system repository reference must be github:owner/repo[/ref]",
        ));
    }
    let mut target = json!({"type": "github", "owner": owner, "repo": repo});
    if let Some(git_ref) = git_ref {
        if let Some(map) = target.as_object_mut() {
            map.insert("ref".to_string(), json!(git_ref));
        }
    }
    Ok(target)
}

/// Drop one entry from the SYSTEM registry: `nix registry remove` only
/// edits the calling user's file, so the daemon edits the system file the
/// Webmin way (backup rotation + atomic replace).
async fn remove_system_repository(name: &str, copies: usize) -> Result<(), ApiError> {
    let mut value = read_system_registry()?;
    if !drop_registry_flake(&mut value, name)? {
        // Nothing there — removing is idempotent.
        return Ok(());
    }
    write_system_registry(&value, copies).await
}

/// Remove one `type = indirect` flake from a parsed registry document
/// (a same-id entry of another type is never touched). Returns whether
/// anything was dropped. A present-but-wrong-typed `flakes` key is a
/// diagnosed error — never a silent success.
fn drop_registry_flake(value: &mut serde_json::Value, name: &str) -> Result<bool, ApiError> {
    let Some(raw) = value.get_mut("flakes") else {
        return Ok(false);
    };
    let Some(flakes) = raw.as_array_mut() else {
        return Err(ApiError::internal("registry file: flakes is not an array"));
    };
    let before = flakes.len();
    flakes.retain(|entry| indirect_id(entry) != Some(name));
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
    let out = exec::run_as_user_json(
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

/// Locks pinned by one profile element's URL.
#[derive(Debug, Default, PartialEq, Eq)]
struct ElementPin {
    nar: Option<String>,
    rev: Option<String>,
}

/// The current locks of one flake reference (either may be missing).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
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

/// Both locks a profile element's `url` pins: the `narHash` query value and
/// the revision (a `rev=` query value or the hex path segment nix emits).
/// Empty fields mean "not pinned that way" — never guess.
fn element_pin(url: &str) -> ElementPin {
    let (path, query) = url.split_once('?').unwrap_or((url, ""));
    let mut pin = ElementPin::default();
    for pair in query.split('&') {
        if let Some(value) = pair.strip_prefix("narHash=") {
            let nar = percent_decode(value);
            if !nar.is_empty() {
                pin.nar = Some(nar);
            }
        } else if let Some(value) = pair.strip_prefix("rev=") {
            if is_rev(value) {
                pin.rev = Some(value.to_string());
            }
        }
    }
    if pin.rev.is_none() {
        let last = path.rsplit('/').next().unwrap_or("");
        if is_rev(last) {
            pin.rev = Some(last.to_string());
        }
    }
    pin
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

/// Short pins are prefixes of the full commit id.
fn rev_matches(pinned: &str, current: &str) -> bool {
    current == pinned || current.starts_with(pinned)
}

/// True only when a lock kind exists on BOTH sides and the values differ.
/// A revision comparison wins when the current side reports one (cheap
/// github path); otherwise the narHash is compared. Missing data means
/// "cannot tell" — never report an update we cannot prove.
fn outdatable(pin: &ElementPin, meta: &MetaLock) -> bool {
    if let (Some(pinned), Some(current)) = (&pin.rev, &meta.rev) {
        return !rev_matches(pinned, current);
    }
    match (&pin.nar, &meta.nar) {
        (Some(pinned), Some(current)) => pinned != current,
        _ => false,
    }
}

/// A cheap `github:` reference resolvable with `git ls-remote`.
#[derive(Debug, Clone, PartialEq, Eq)]
struct GithubRef {
    owner: String,
    repo: String,
    git_ref: Option<String>,
}

/// `github:owner/repo[/ref]` (a `?dir=` query falls back to metadata: the
/// subdir does not change the commit, but keep the rule strict anyway).
fn parse_github_ref(value: &str) -> Option<GithubRef> {
    let rest = value.strip_prefix("github:")?;
    if rest.contains('?') || rest.contains('#') {
        return None;
    }
    let mut parts = rest.splitn(3, '/');
    let owner = parts.next()?;
    let repo = parts.next()?;
    let git_ref = parts.next().filter(|value| !value.is_empty());
    let atom = |value: &str| {
        !value.is_empty()
            && value.len() <= 128
            && !value.starts_with('-')
            && value
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
    };
    if !atom(owner) || !atom(repo) {
        return None;
    }
    if let Some(git_ref) = git_ref {
        let good = git_ref.len() <= 256
            && !git_ref.starts_with('-')
            && git_ref
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-' | '/'));
        if !good {
            return None;
        }
    }
    Some(GithubRef {
        owner: owner.to_string(),
        repo: repo.to_string(),
        git_ref: git_ref.map(str::to_string),
    })
}

/// `git ls-remote <url> <ref>` output → the first commit id.
fn parse_ls_remote(output: &str) -> Option<String> {
    output.lines().find_map(|line| {
        let mut fields = line.split_whitespace();
        let sha = fields.next()?;
        is_rev(sha).then(|| sha.to_string())
    })
}

/// Resolve `flake:alias` through the registry rows (user > system > global,
/// the order `nix registry list` prints).
fn resolve_ref(original: &str, aliases: &HashMap<String, String>) -> String {
    let mut current = original.to_string();
    for _ in 0..3 {
        let Some(name) = current.strip_prefix("flake:") else {
            break;
        };
        let Some(target) = aliases.get(name) else {
            break;
        };
        if target == &current {
            break;
        }
        current.clone_from(target);
    }
    current
}

/// Element names whose pinned lock differs from the current lock of their
/// original flake reference (profile order, unique).
fn outdated_names(elements: &[InstalledPackage], metas: &HashMap<String, MetaLock>) -> Vec<String> {
    let mut packages: Vec<String> = Vec::new();
    for element in elements {
        let Some(original) = &element.original_url else {
            continue;
        };
        let Some(url) = &element.url else {
            continue;
        };
        let pin = element_pin(url);
        if pin.rev.is_none() && pin.nar.is_none() {
            continue;
        }
        let Some(meta) = metas.get(original) else {
            continue;
        };
        if outdatable(&pin, meta) && !packages.contains(&element.name) {
            packages.push(element.name.clone());
        }
    }
    packages
}

/// `git` is optional: when present it resolves a github commit id with a
/// tiny `ls-remote` call instead of a full flake fetch + eval.
const GIT_CANDIDATES: [&str; 4] = [
    "/usr/bin/git",
    "/bin/git",
    "/usr/local/bin/git",
    "/run/current-system/sw/bin/git",
];

fn git_path() -> Option<&'static str> {
    crate::tooling::find(&GIT_CANDIDATES)
}

/// Current commit id of a github ref (`HEAD` when the flake names none).
async fn github_rev(user: &str, github: &GithubRef) -> Option<String> {
    let git = git_path()?;
    let url = format!("https://github.com/{}/{}.git", github.owner, github.repo);
    let spec = github.git_ref.as_deref().unwrap_or("HEAD");
    let out = exec::run_as_user_argv_tolerant(
        user,
        git,
        &["ls-remote", &url, spec],
        Duration::from_secs(20),
        None,
    )
    .await
    .ok()?;
    (out.code == 0)
        .then(|| parse_ls_remote(&out.stdout))
        .flatten()
}

/// Registry aliases (`flake:name` → target), first row wins (user > system
/// > global, the order `nix registry list` prints).
async fn alias_map(user: &str) -> HashMap<String, String> {
    registry_rows(user)
        .await
        .map(|rows| {
            let mut map = HashMap::new();
            for row in rows {
                map.entry(row.name).or_insert(row.url);
            }
            map
        })
        .unwrap_or_default()
}

/// One target's current lock: a github commit id via `ls-remote` when the
/// target is a github flake and every element pinning it carries a rev,
/// otherwise the full flake metadata.
async fn target_lock(user: String, target: String, use_github: bool) -> (String, MetaLock) {
    if use_github {
        if let Some(github) = parse_github_ref(&target) {
            if let Some(rev) = github_rev(&user, &github).await {
                return (
                    target,
                    MetaLock {
                        nar: None,
                        rev: Some(rev),
                    },
                );
            }
        }
    }
    let lock = exec::run_as_user_json(
        &user,
        &[
            ("flake", ""),
            ("metadata", ""),
            ("--json", ""),
            ("--", target.as_str()),
        ],
        UPDATE_CHECK_TIMEOUT,
    )
    .await
    .map_or_else(|_| MetaLock::default(), |out| meta_locks(&out.stdout));
    (target, lock)
}

/// Update check: profile list + the current lock of every unique flake
/// reference, fetched in parallel (github refs use `git ls-remote`, no
/// flake fetch or eval). Same proof as the sequential version: an update is
/// reported only when a comparable lock differs.
async fn nix_updates(user: &str) -> Result<ursine_api::NixUpdates, ApiError> {
    let out = exec::run_as_user_json(
        user,
        &[("profile", ""), ("list", ""), ("--json", "")],
        UPDATE_CHECK_TIMEOUT,
    )
    .await?;
    let elements = parse::parse_profile_list(&out.stdout)?;
    let aliases = alias_map(user).await;

    // target → (every element carries a rev, originals that resolve to it)
    let mut groups: HashMap<String, (bool, Vec<String>)> = HashMap::new();
    for element in &elements {
        let (Some(original), Some(url)) = (&element.original_url, element.url.as_deref()) else {
            continue;
        };
        let pin = element_pin(url);
        if pin.rev.is_none() && pin.nar.is_none() {
            continue;
        }
        if !exec::is_safe_quoted_value(original) {
            continue;
        }
        let target = resolve_ref(original, &aliases);
        if !exec::is_safe_quoted_value(&target) {
            continue;
        }
        let entry = groups.entry(target).or_insert_with(|| (true, Vec::new()));
        entry.0 = entry.0 && pin.rev.is_some();
        if !entry.1.contains(original) {
            entry.1.push(original.clone());
        }
    }

    // Bounded parallelism: a couple of refs are network-bound, the rest
    // usually hit nix's own cache.
    let mut pending = groups
        .iter()
        .map(|(target, (all_rev, _))| (target.clone(), *all_rev))
        .collect::<Vec<_>>()
        .into_iter();
    let mut set: tokio::task::JoinSet<(String, MetaLock)> = tokio::task::JoinSet::new();
    let mut locks: HashMap<String, MetaLock> = HashMap::new();
    for _ in 0..4 {
        if let Some((target, use_github)) = pending.next() {
            set.spawn(target_lock(user.to_string(), target, use_github));
        }
    }
    while let Some(joined) = set.join_next().await {
        if let Ok((target, lock)) = joined {
            locks.insert(target, lock);
        }
        if let Some((target, use_github)) = pending.next() {
            set.spawn(target_lock(user.to_string(), target, use_github));
        }
    }

    let mut metas: HashMap<String, MetaLock> = HashMap::new();
    for (target, (_, originals)) in groups {
        let lock = locks.remove(&target).unwrap_or_default();
        for original in originals {
            metas.insert(original, lock.clone());
        }
    }
    let packages = outdated_names(&elements, &metas);
    Ok(ursine_api::NixUpdates {
        count: packages.len(),
        packages,
    })
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

/// Active generation from the profile symlink, `Ok(None)` when the user has
/// no profile at all (a fresh account with nothing installed through nix).
///
/// A link that exists but cannot be read keeps its error: that is a broken
/// profile, not an empty one.
fn active_generation(profile: &std::path::Path) -> Result<Option<u32>, ApiError> {
    let link = match std::fs::read_link(profile) {
        Ok(link) => link,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(ApiError::invalid(format!(
                "cannot read profile link: {error}"
            )))
        }
    };
    generations::parse_generation_link(&link.to_string_lossy())
        .map(Some)
        .ok_or_else(|| {
            ApiError::new(
                ErrorCode::Conflict,
                "cannot determine the active generation; refresh and retry",
            )
        })
}

/// Reply for a profile that does not exist yet: no generation would be
/// deleted (dry run) and a real run succeeds deleting nothing. Keys match
/// the normal payloads so a client parses one shape.
fn no_profile_payload(dry_run: bool) -> serde_json::Value {
    if dry_run {
        json!({
            "dry_run": true,
            "current": null,
            "would_delete": [],
            "kept": 0,
        })
    } else {
        prune_apply_payload(None, &[], "", "")
    }
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
            // Complete output: the installed list must never truncate.
            let out = exec::run_as_user_json(
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
        Action::ListNixRepositories { system } => {
            Ok(json!({"repositories": list_repositories(ctx.as_user, *system).await?}))
        }
        Action::AddNixRepository { name, url, system } => {
            validate_repo_name(name)?;
            validate_repo_url(url)?;
            let copies = crate::backup_configs::copies_from(&ctx.config);
            if *system {
                upsert_system_repository(name, url, copies).await?;
            } else {
                backup_registry(ctx.as_user, copies).await?;
                let program = exec::nix_path()?;
                exec::run_as_user_argv(
                    ctx.as_user,
                    program,
                    &["registry", "add", name, url],
                    MUTATE_TIMEOUT,
                )
                .await?;
            }
            Ok(json!({"repositories": list_repositories(ctx.as_user, *system).await?}))
        }
        Action::RemoveNixRepository {
            name,
            system,
            source,
        } => {
            validate_repo_name(name)?;
            let copies = crate::backup_configs::copies_from(&ctx.config);
            match source {
                ursine_api::NixRepoSource::Channel => {
                    let program = exec::nix_channel_path()?;
                    if *system {
                        exec::run_root_program(program, &["--remove", name], MUTATE_TIMEOUT)
                            .await?;
                    } else {
                        exec::run_as_user_argv(
                            ctx.as_user,
                            program,
                            &["--remove", name],
                            MUTATE_TIMEOUT,
                        )
                        .await?;
                    }
                }
                ursine_api::NixRepoSource::Registry => {
                    if *system {
                        remove_system_repository(name, copies).await?;
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
                }
            }
            Ok(json!({"repositories": list_repositories(ctx.as_user, *system).await?}))
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
        Action::ListNixUpdates => Ok(json!(nix_updates(user).await?)),

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
            let Some(current) = active_generation(profile_ref)? else {
                // No profile link at all: nothing was ever installed with nix
                // for this user, so there is no generation to keep and none to
                // delete. Answering empty (dry run) and deleting nothing (real
                // run) is the honest reply; a link error here read as a failure
                // of a request that has nothing to do.
                return Ok(no_profile_payload(*dry_run));
            };

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
    #![allow(clippy::disallowed_methods)] // tests use the host temp dir

    #[test]
    fn element_lock_prefers_percent_encoded_narhash() {
        // Verbatim element url from `nix profile list --json` on the
        // reference VM (nix 2.31.5).
        let url = "github:NixOS/nixpkgs/7dcdd87373d72e7a2173e4db02e20d3d7f495cfd\
            ?narHash=sha256-tItrb9M8eYA6WhFgmT9tpGKueri3%2BwRuVA952o/2x";
        let pin = element_pin(url);
        assert_eq!(
            pin.nar.as_deref(),
            Some("sha256-tItrb9M8eYA6WhFgmT9tpGKueri3+wRuVA952o/2x")
        );
        assert_eq!(
            pin.rev.as_deref(),
            Some("7dcdd87373d72e7a2173e4db02e20d3d7f495cfd")
        );
    }

    #[test]
    fn element_lock_falls_back_to_rev_and_none_without_lock() {
        assert_eq!(
            element_pin("github:a/b/13d0c311e3ae923a00f734b43fd1d35b47d8943a").rev,
            Some("13d0c311e3ae923a00f734b43fd1d35b47d8943a".to_string())
        );
        assert_eq!(
            element_pin("git+https://example.com/x/y?rev=abcdef1").rev,
            Some("abcdef1".to_string())
        );
        assert_eq!(element_pin("flake:nixpkgs#hello"), ElementPin::default());
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
    fn element_pin_reads_narhash_and_path_rev() {
        let pin = element_pin(
            "github:skea999/ursine/0123456789abcdef0123456789abcdef01234567?narHash=sha256-a%2Bb%3D",
        );
        assert_eq!(
            pin.rev.as_deref(),
            Some("0123456789abcdef0123456789abcdef01234567")
        );
        assert_eq!(pin.nar.as_deref(), Some("sha256-a+b="));
        let short = element_pin("github:o/r/abcdef1");
        assert_eq!(short.rev.as_deref(), Some("abcdef1"));
        assert!(short.nar.is_none());
        assert_eq!(element_pin("path:/nix/store/x"), ElementPin::default());
    }

    #[test]
    fn rev_comparison_prefers_revisions_and_falls_back_to_nar() {
        let pin =
            element_pin("github:o/r/0123456789abcdef0123456789abcdef01234567?narHash=sha256-a");
        let newer = MetaLock {
            nar: None,
            rev: Some("fedcba9876543210fedcba9876543210fedcba98".into()),
        };
        assert!(outdatable(&pin, &newer));
        let same = MetaLock {
            nar: None,
            rev: Some("0123456789abcdef0123456789abcdef01234567".into()),
        };
        assert!(!outdatable(&pin, &same));
        // No revision on the table → look at the narHash.
        let nar_same = MetaLock {
            nar: Some("sha256-a".into()),
            rev: None,
        };
        assert!(!outdatable(&pin, &nar_same));
        let nar_new = MetaLock {
            nar: Some("sha256-b".into()),
            rev: None,
        };
        assert!(outdatable(&pin, &nar_new));
        // Nothing comparable → never guess.
        assert!(!outdatable(&pin, &MetaLock::default()));
    }

    #[test]
    fn github_refs_parse_and_ls_remote_output_is_checked() {
        let github = parse_github_ref("github:nix-community/nixGL").unwrap();
        assert_eq!(github.owner, "nix-community");
        assert_eq!(github.repo, "nixGL");
        assert_eq!(github.git_ref, None);
        let pinned = parse_github_ref("github:NixOS/nixpkgs/nixos-26.05").unwrap();
        assert_eq!(pinned.git_ref.as_deref(), Some("nixos-26.05"));
        assert!(parse_github_ref("github:owner/repo?dir=sub").is_none());
        assert!(parse_github_ref("github:-bad/repo").is_none());
        assert!(parse_github_ref("flake:nixpkgs").is_none());
        let sha = "0123456789abcdef0123456789abcdef01234567";
        assert_eq!(
            parse_ls_remote(&format!("{sha}\trefs/heads/main\n")),
            Some(sha.to_string())
        );
        assert!(parse_ls_remote("not a sha\trefs/heads/main").is_none());
    }

    #[test]
    fn aliases_resolve_like_nix() {
        let mut aliases = HashMap::new();
        aliases.insert(
            "nixpkgs".to_string(),
            "github:NixOS/nixpkgs/nixpkgs-unstable".to_string(),
        );
        aliases.insert("other".to_string(), "flake:nixpkgs".to_string());
        assert_eq!(
            resolve_ref("flake:nixpkgs", &aliases),
            "github:NixOS/nixpkgs/nixpkgs-unstable"
        );
        // One level of alias chaining is enough; a cycle stops.
        assert_eq!(
            resolve_ref("flake:other", &aliases),
            "github:NixOS/nixpkgs/nixpkgs-unstable"
        );
        assert_eq!(resolve_ref("github:o/r", &aliases), "github:o/r");
        assert_eq!(resolve_ref("flake:unknown", &aliases), "flake:unknown");
    }

    #[test]
    fn outdatable_requires_a_matching_lock_kind() {
        let meta = MetaLock {
            nar: Some("sha256-b".to_string()),
            rev: Some("aaaaaaa".to_string()),
        };
        let nar = ElementPin {
            nar: Some("sha256-a".to_string()),
            rev: None,
        };
        assert!(outdatable(&nar, &meta));
        let nar_same = ElementPin {
            nar: Some("sha256-b".to_string()),
            rev: None,
        };
        assert!(!outdatable(&nar_same, &meta));
        let rev = ElementPin {
            nar: None,
            rev: Some("bbbbbbb".to_string()),
        };
        assert!(outdatable(&rev, &meta));
        let rev_same = ElementPin {
            nar: None,
            rev: Some("aaaaaaa".to_string()),
        };
        assert!(!outdatable(&rev_same, &meta));
        // No matching field on the metadata side → cannot tell.
        assert!(!outdatable(&nar, &MetaLock::default()));
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

            version: None,
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
        assert!(drop_registry_flake(&mut value, "nixpkgs").unwrap());
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
        assert!(!drop_registry_flake(&mut value, "nixpkgs").unwrap());
        let mut no_key = json!({"version": 2});
        assert!(!drop_registry_flake(&mut no_key, "nixpkgs").unwrap());
        // Wrong-typed flakes is diagnosed, not swallowed.
        let mut wrong = json!({"flakes": "not an array"});
        assert!(drop_registry_flake(&mut wrong, "nixpkgs").is_err());
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
        assert_eq!(rows[0].scope, "user");
        assert_eq!(rows[0].name, "nixpkgs");
        assert_eq!(rows[1].scope, "user");
        assert_eq!(rows[1].name, "agenix");
    }

    #[test]
    fn parses_channel_list() {
        // `nix-channel --list`: one `name url` pair per line.
        let out = "nixpkgs https://nixos.org/channels/nixos-26.05\n";
        let rows = parse_channel_list(out);
        assert_eq!(rows.len(), 1, "{rows:?}");
        assert_eq!(rows[0].0, "nixpkgs");
        assert_eq!(rows[0].1, "https://nixos.org/channels/nixos-26.05");
        assert!(parse_channel_list("").is_empty());
        assert!(parse_channel_list("only-a-name\n").is_empty());
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

                version: None,
            },
            InstalledPackage {
                name: "hello".into(),
                attr_path: None,
                original_url: None,
                url: None,
                store_paths: Vec::new(),
                active: true,

                description: None,

                version: None,
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

    #[test]
    fn missing_profile_link_is_no_profile_not_an_error() {
        let dir = std::env::temp_dir().join(format!("ursine-prune-profile-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let profile = dir.join("profile");

        // Issue #18: a fresh account has no profile link at all — that is an
        // empty profile, never "cannot read profile link".
        assert_eq!(active_generation(&profile).unwrap(), None);

        // A real `profile-N-link` answers its generation.
        std::os::unix::fs::symlink(dir.join("profile-7-link"), &profile).unwrap();
        assert_eq!(active_generation(&profile).unwrap(), Some(7));

        // A link that exists but is not a generation link is a conflict, not
        // an empty profile: the file was planted or nix changed its layout.
        std::fs::remove_file(&profile).unwrap();
        std::fs::create_dir_all(dir.join("nixpkgs")).unwrap();
        std::os::unix::fs::symlink("nixpkgs", &profile).unwrap();
        assert!(active_generation(&profile).is_err());

        // A path that cannot be read as a link either (a directory) keeps its
        // error too.
        std::fs::remove_file(&profile).unwrap();
        std::fs::create_dir(&profile).unwrap();
        let error = active_generation(&profile).unwrap_err();
        assert!(error.message.contains("cannot read profile link"));

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn no_profile_prune_answers_empty_plans() {
        // Dry run: an empty list of generations to delete, no current one.
        let preview = no_profile_payload(true);
        assert_eq!(preview["dry_run"], true);
        assert_eq!(preview["current"], serde_json::Value::Null);
        assert_eq!(preview["would_delete"], json!([]));
        assert_eq!(preview["kept"], 0);

        // Real run: success with nothing deleted.
        let applied = no_profile_payload(false);
        assert_eq!(applied["dry_run"], false);
        assert_eq!(applied["current"], serde_json::Value::Null);
        assert_eq!(applied["deleted"], json!([]));
    }
}
