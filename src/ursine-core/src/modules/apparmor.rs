//! apparmor — policy management through the `AppArmor` CLIs (SPEC-apparmor).
//!
//! No D-Bus (invariant 3): `aa-status` for state (its exit codes 1-3 are
//! data, so a tolerant runner carries the streams), `aa-*` mode tools,
//! `apparmor_parser` for load/check/remove. The profile editor
//! syntax-checks the NEW content on stdin before any write; the
//! notifications button flips `show_notifications` and installs the
//! `aa-notify` autostart entry for the session user (next login).

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::json;
use ursine_api::{
    Action, ApiError, ApparmorCheck, ApparmorFile, ApparmorOutput, ApparmorProfile,
    ApparmorProfileFile, ApparmorView,
};

use super::ModuleCtx;
use crate::modules::packages_nix::exec;

const PROFILES_DIR: &str = "/etc/apparmor.d";
const DISABLE_DIR: &str = "/etc/apparmor.d/disable";
/// Root of the profile backup slots: the parser loads every file inside
/// [`PROFILES_DIR`], so the slots must live outside it.
const BACKUP_DIR: &str = "/var/backups/ursine/apparmor";
const NOTIFY_CONF: &str = "/etc/apparmor/notify.conf";
const AUTOSTART: &str = ".config/autostart/aa-notify.desktop";
const TIMEOUT: Duration = Duration::from_secs(60);
/// Profile sources are text files; anything larger is refused outright.
const MAX_PROFILE_BYTES: usize = 512 * 1024;

const AA_STATUS: [&str; 4] = [
    "/usr/sbin/aa-status",
    "/usr/bin/aa-status",
    "/sbin/aa-status",
    "/bin/aa-status",
];
const AA_PARSER: [&str; 4] = [
    "/sbin/apparmor_parser",
    "/usr/sbin/apparmor_parser",
    "/usr/bin/apparmor_parser",
    "/bin/apparmor_parser",
];
const AA_ENFORCE: [&str; 4] = [
    "/usr/sbin/aa-enforce",
    "/usr/bin/aa-enforce",
    "/sbin/aa-enforce",
    "/bin/aa-enforce",
];
const AA_COMPLAIN: [&str; 4] = [
    "/usr/sbin/aa-complain",
    "/usr/bin/aa-complain",
    "/sbin/aa-complain",
    "/bin/aa-complain",
];
const AA_DISABLE: [&str; 4] = [
    "/usr/sbin/aa-disable",
    "/usr/bin/aa-disable",
    "/sbin/aa-disable",
    "/bin/aa-disable",
];
const AA_UNCONFINED: [&str; 4] = [
    "/usr/sbin/aa-unconfined",
    "/usr/bin/aa-unconfined",
    "/sbin/aa-unconfined",
    "/bin/aa-unconfined",
];
const AA_REMOVE_UNKNOWN: [&str; 4] = [
    "/usr/sbin/aa-remove-unknown",
    "/usr/bin/aa-remove-unknown",
    "/sbin/aa-remove-unknown",
    "/bin/aa-remove-unknown",
];
const AA_CLEANPROF: [&str; 4] = [
    "/usr/sbin/aa-cleanprof",
    "/usr/bin/aa-cleanprof",
    "/sbin/aa-cleanprof",
    "/bin/aa-cleanprof",
];
const AA_AUTODEP: [&str; 4] = [
    "/usr/sbin/aa-autodep",
    "/usr/bin/aa-autodep",
    "/sbin/aa-autodep",
    "/bin/aa-autodep",
];
const AA_FEATURES_ABI: [&str; 4] = [
    "/usr/sbin/aa-features-abi",
    "/usr/bin/aa-features-abi",
    "/sbin/aa-features-abi",
    "/bin/aa-features-abi",
];
const AA_DECODE: [&str; 4] = [
    "/usr/sbin/aa-decode",
    "/usr/bin/aa-decode",
    "/sbin/aa-decode",
    "/bin/aa-decode",
];
const AA_NOTIFY: [&str; 4] = [
    "/usr/sbin/aa-notify",
    "/usr/bin/aa-notify",
    "/sbin/aa-notify",
    "/bin/aa-notify",
];
const AA_EASYPROF: [&str; 4] = [
    "/usr/sbin/aa-easyprof",
    "/usr/bin/aa-easyprof",
    "/sbin/aa-easyprof",
    "/bin/aa-easyprof",
];

pub async fn handle(action: &Action, ctx: &ModuleCtx<'_>) -> Result<serde_json::Value, ApiError> {
    let user = ctx.principal.user.as_str();
    match action {
        Action::ApparmorStatus => Ok(json!(view(user).await?)),
        Action::ApparmorSetMode { path, mode } => {
            set_mode(path, mode).await?;
            Ok(json!(view(user).await?))
        }
        Action::ApparmorLoad { path } => {
            let path = validate_profile_path(path)?;
            let parser = tool(&AA_PARSER, "apparmor_parser")?;
            exec::run_root_program(parser, &["-r", "-W", &path], TIMEOUT).await?;
            Ok(json!(view(user).await?))
        }
        Action::ApparmorCheck { path } => {
            let path = validate_profile_path(path)?;
            let parser = tool(&AA_PARSER, "apparmor_parser")?;
            let outcome =
                exec::run_root_program_tolerant(parser, &["-d", &path], TIMEOUT, None).await?;
            Ok(json!(ApparmorCheck {
                ok: outcome.code == 0,
                output: combine(&outcome.stdout, &outcome.stderr),
            }))
        }
        Action::ApparmorRemove { path } => {
            let path = validate_profile_path(path)?;
            let parser = tool(&AA_PARSER, "apparmor_parser")?;
            exec::run_root_program(parser, &["-R", &path], TIMEOUT).await?;
            Ok(json!(view(user).await?))
        }
        Action::ApparmorReloadAll => {
            // The parser reads the whole directory here, so 0.48.0 leftovers
            // would abort the reload with "Conflicting profiles".
            migrate_legacy_backups_logged();
            let parser = tool(&AA_PARSER, "apparmor_parser")?;
            exec::run_root_program(parser, &["-r", "-W", PROFILES_DIR], TIMEOUT).await?;
            Ok(json!(view(user).await?))
        }
        Action::ApparmorUnconfined => {
            let program = tool(&AA_UNCONFINED, "aa-unconfined")?;
            let outcome = exec::run_root_program_tolerant(program, &[], TIMEOUT, None).await?;
            Ok(json!(ApparmorOutput {
                output: combine(&outcome.stdout, &outcome.stderr),
            }))
        }
        Action::ApparmorRemoveUnknown => {
            let program = tool(&AA_REMOVE_UNKNOWN, "aa-remove-unknown")?;
            exec::run_root_program(program, &[], TIMEOUT).await?;
            Ok(json!(view(user).await?))
        }
        Action::ApparmorProfileGet { path } => {
            let path = validate_profile_path(path)?;
            let meta = std::fs::metadata(&path)
                .map_err(|e| ApiError::internal(format!("stat {path}: {e}")))?;
            if meta.len() > MAX_PROFILE_BYTES as u64 {
                return Err(ApiError::invalid("profile too large to edit"));
            }
            let content = std::fs::read_to_string(&path)
                .map_err(|e| ApiError::internal(format!("read {path}: {e}")))?;
            if content.len() > MAX_PROFILE_BYTES {
                return Err(ApiError::invalid("profile too large to edit"));
            }
            Ok(json!(ApparmorProfileFile { path, content }))
        }
        Action::ApparmorProfileSet { path, content } => {
            profile_set(
                path,
                content,
                crate::backup_configs::copies_from(&ctx.config),
            )
            .await?;
            Ok(json!(view(user).await?))
        }
        Action::ApparmorNotificationsEnable => {
            notifications_enable(user, crate::backup_configs::copies_from(&ctx.config)).await?;
            Ok(json!(view(user).await?))
        }
        Action::ApparmorCleanprof { name } => {
            // aa-cleanprof is interactive and its stdin is null here; -s
            // overwrites with the clean profile, --no-reload leaves the
            // reload to the module's own action.
            run_profile_tool(
                ctx,
                user,
                name,
                &AA_CLEANPROF,
                "aa-cleanprof",
                &["-s", "--no-reload"],
            )
            .await
        }
        Action::ApparmorAutodep { path } => {
            run_profile_tool(ctx, user, path, &AA_AUTODEP, "aa-autodep", &[]).await
        }
        Action::ApparmorFeaturesAbi => {
            let program = tool(&AA_FEATURES_ABI, "aa-features-abi")?;
            let outcome =
                exec::run_root_program_tolerant(program, &["--extract"], TIMEOUT, None).await?;
            Ok(json!(ApparmorOutput {
                output: combine(&outcome.stdout, &outcome.stderr),
            }))
        }
        Action::ApparmorDecode { hex } => {
            validate_hex(hex)?;
            let program = tool(&AA_DECODE, "aa-decode")?;
            let outcome = exec::run_root_program_tolerant(program, &[hex], TIMEOUT, None).await?;
            Ok(json!(ApparmorOutput {
                output: combine(&outcome.stdout, &outcome.stderr),
            }))
        }
        Action::ApparmorEasyprof {
            binary,
            template,
            policy_groups,
            template_vars,
        } => {
            let program = tool(&AA_EASYPROF, "aa-easyprof")?;
            let mut args: Vec<String> = Vec::new();
            if let Some(template) = template {
                validate_template(template)?;
                // Two argv elements: Python optparse takes `-t VALUE`
                // portably; `-t=VALUE` would keep the `=` in the value.
                args.push("-t".to_string());
                args.push(template.clone());
            }
            if let Some(groups) = policy_groups {
                validate_policy_groups(groups)?;
                args.push("-p".to_string());
                args.push(groups.clone());
            }
            for template_var in template_vars {
                validate_template_var(template_var)?;
                args.push("--template-var".to_string());
                args.push(template_var.clone());
            }
            args.push(validate_binary(binary)?);
            let arg_refs: Vec<&str> = args.iter().map(String::as_str).collect();
            // Quoted-value guard: profile paths contain `*` and spaces.
            // No --output-directory → the tool prints to stdout only.
            let outcome = exec::run_root_program_value(program, &arg_refs, TIMEOUT).await?;
            Ok(json!(ApparmorOutput {
                output: combine(&outcome.stdout, &outcome.stderr),
            }))
        }
        Action::ApparmorEasyprofList { what } => {
            let flag = match what.as_str() {
                "templates" => "--list-templates",
                "policy_groups" => "--list-policy-groups",
                _ => return Err(ApiError::invalid("what must be templates or policy_groups")),
            };
            let program = tool(&AA_EASYPROF, "aa-easyprof")?;
            let outcome = exec::run_root_program_value(program, &[flag], TIMEOUT).await?;
            Ok(json!(ApparmorOutput {
                output: combine(&outcome.stdout, &outcome.stderr),
            }))
        }
        _ => Err(ApiError::invalid("action is not valid for module apparmor")),
    }
}

// ---- view ---------------------------------------------------------------

/// The two per-profile tools share one path: validate the target, rotate the
/// profile directory once, run the tool, answer with the fresh view.
async fn run_profile_tool(
    ctx: &ModuleCtx<'_>,
    user: &str,
    target: &str,
    candidates: &[&'static str],
    label: &str,
    extra: &[&str],
) -> Result<serde_json::Value, ApiError> {
    let target = validate_target(target)?;
    let program = tool(candidates, label)?;
    rotate_profiles_dir(crate::backup_configs::copies_from(&ctx.config)).await?;
    let mut args: Vec<&str> = extra.to_vec();
    args.push(&target);
    exec::run_root_program(program, &args, TIMEOUT).await?;
    Ok(json!(view(user).await?))
}

async fn view(user: &str) -> Result<ApparmorView, ApiError> {
    let status = aa_status().await?;
    let mut profiles = status.profiles;
    profiles.sort_by(|a, b| a.name.cmp(&b.name));
    let files = profile_files(&profiles);
    Ok(ApparmorView {
        enabled: status.enabled,
        mounted: status.mounted,
        profiles,
        files,
        processes_defined: status.processes_defined,
        processes_enforce: status.processes_enforce,
        processes_complain: status.processes_complain,
        notifications_configured: notify_configured(),
        notifications_autostart: autostart_present(user),
        aa_notify_present: crate::tooling::find(&AA_NOTIFY).is_some(),
    })
}

struct Status {
    enabled: bool,
    mounted: bool,
    profiles: Vec<ApparmorProfile>,
    processes_defined: usize,
    processes_enforce: usize,
    processes_complain: usize,
}

/// Run `aa-status` and parse its (stable) verbose text. Exit codes 1-3
/// are data (not loaded / no policy / securityfs missing); 4 and 42 are
/// real failures.
async fn aa_status() -> Result<Status, ApiError> {
    let program = tool(&AA_STATUS, "aa-status")?;
    let outcome = exec::run_root_program_tolerant(program, &[], TIMEOUT, None).await?;
    if matches!(outcome.code, 4 | 42) {
        let detail = combine(&outcome.stdout, &outcome.stderr);
        return Err(ApiError::internal(format!(
            "aa-status failed (exit {}): {detail}",
            outcome.code
        )));
    }
    Ok(parse_status(&combine(&outcome.stdout, &outcome.stderr)))
}

/// Parse the aa-status verbose format (stable since 2.x).
fn parse_status(text: &str) -> Status {
    #[derive(PartialEq)]
    enum Section {
        None,
        Enforce,
        Complain,
    }
    let mut status = Status {
        enabled: text.contains("apparmor module is loaded."),
        // Mounted only with positive evidence: empty output or a killed
        // run must not claim securityfs is there.
        mounted: text.contains("apparmor module is loaded.")
            && !text.contains("apparmor filesystem is not mounted"),
        profiles: Vec::new(),
        processes_defined: 0,
        processes_enforce: 0,
        processes_complain: 0,
    };
    let mut section = Section::None;
    for line in text.lines() {
        let indented = line.starts_with(' ') || line.starts_with('\t');
        if !indented {
            let trimmed = line.trim();
            if trimmed.contains("profiles are in enforce mode") {
                section = Section::Enforce;
            } else if trimmed.contains("profiles are in complain mode") {
                section = Section::Complain;
            } else {
                section = Section::None;
                if let Some(count) = leading_count(trimmed, "processes have profiles defined") {
                    status.processes_defined = count;
                } else if let Some(count) =
                    leading_count(trimmed, "processes have profiles in enforce mode")
                {
                    status.processes_enforce = count;
                } else if let Some(count) =
                    leading_count(trimmed, "processes have profiles in complain mode")
                {
                    status.processes_complain = count;
                }
            }
            continue;
        }
        let name = line.trim();
        if name.is_empty() {
            continue;
        }
        match section {
            Section::Enforce => status.profiles.push(ApparmorProfile {
                name: name.to_string(),
                mode: "enforce".to_string(),
            }),
            Section::Complain => status.profiles.push(ApparmorProfile {
                name: name.to_string(),
                mode: "complain".to_string(),
            }),
            Section::None => {}
        }
    }
    status
}

/// `110 profiles are loaded.` → 110 when the line ends with `suffix`.
fn leading_count(line: &str, suffix: &str) -> Option<usize> {
    let line = line.trim_end_matches(['.', ' ']);
    let rest = line.strip_suffix(suffix)?.trim();
    rest.split_whitespace().next()?.parse().ok()
}

/// Comparison key for profile identity: aa-status may report a
/// path-style name (`/usr/bin/foo`) while the file is `usr.bin.foo`.
fn profile_key(name: &str) -> String {
    name.trim_start_matches('/').replace('/', ".")
}

fn profile_files(profiles: &[ApparmorProfile]) -> Vec<ApparmorFile> {
    let Ok(entries) = std::fs::read_dir(PROFILES_DIR) else {
        return Vec::new();
    };
    let mut files: Vec<ApparmorFile> = entries
        .flatten()
        .filter(|entry| entry.path().is_file())
        .map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            let path = entry.path().to_string_lossy().into_owned();
            let disabled = Path::new(DISABLE_DIR).join(&name).exists();
            let key = profile_key(&name);
            let matching = profiles
                .iter()
                .find(|profile| profile_key(&profile.name) == key);
            ApparmorFile {
                name,
                path,
                disabled,
                loaded: matching.is_some(),
                mode: matching.map(|profile| profile.mode.clone()),
            }
        })
        .collect();
    files.sort_by(|a, b| a.name.cmp(&b.name));
    files
}

// ---- validation ---------------------------------------------------------

/// Profile files live under `/etc/apparmor.d` (subdirs such as
/// `containers/` included); canonicalize so a symlink cannot escape.
fn validate_profile_path(path: &str) -> Result<String, ApiError> {
    if path.len() > 4096 || !path.starts_with("/etc/apparmor.d/") {
        return Err(ApiError::invalid(
            "profile path must live under /etc/apparmor.d",
        ));
    }
    let canonical = std::fs::canonicalize(path)
        .map_err(|e| ApiError::invalid(format!("cannot open profile {path}: {e}")))?;
    let root = std::fs::canonicalize(PROFILES_DIR)
        .map_err(|e| ApiError::internal(format!("canonicalize {PROFILES_DIR}: {e}")))?;
    if !canonical.starts_with(&root) || !canonical.is_file() {
        return Err(ApiError::invalid(
            "profile path must be a file under /etc/apparmor.d",
        ));
    }
    Ok(canonical.to_string_lossy().into_owned())
}

/// Identifier passed straight to an `aa-*` tool (profile name or binary
/// path): atom charset, no traversal, bounded.
fn validate_target(value: &str) -> Result<String, ApiError> {
    if value.is_empty()
        || value.len() > 512
        || value.contains("..")
        || !value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-' | '+' | '/' | ':'))
    {
        return Err(ApiError::invalid("invalid profile target"));
    }
    Ok(value.to_string())
}

/// The `<path to binary>` argument of aa-easyprof (often `/opt/app/**`).
fn validate_binary(value: &str) -> Result<String, ApiError> {
    if value.is_empty()
        || value.len() > 4096
        || value
            .chars()
            .any(|c| matches!(c, '\n' | '\r' | '\0' | '\''))
    {
        return Err(ApiError::invalid("invalid binary path"));
    }
    Ok(value.to_string())
}

fn validate_template(value: &str) -> Result<(), ApiError> {
    if value.is_empty()
        || value.len() > 256
        || !value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
    {
        return Err(ApiError::invalid("invalid template name"));
    }
    Ok(())
}

/// Comma-separated policy group names.
/// `aa-easyprof` declares policy variables with `--template-var=@{NAME}=VALUE`
/// and rejects a malformed declaration; filter the same forms up front so the
/// caller gets `Invalid`, not `Invalid policy` from the tool.
fn validate_template_var(value: &str) -> Result<(), ApiError> {
    let (key, val) = value
        .split_once('=')
        .ok_or_else(|| ApiError::invalid("template variable must be @{NAME}=VALUE"))?;
    let name = key
        .strip_prefix("@{")
        .and_then(|rest| rest.strip_suffix('}'))
        .filter(|name| {
            !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        });
    if name.is_none() || val.contains('=') {
        return Err(ApiError::invalid(
            "template variable must be @{NAME}=VALUE with NAME alphanumeric or _",
        ));
    }
    if val.contains('"') || val.contains("../") {
        return Err(ApiError::invalid(
            "template variable value must not contain quotes or ../",
        ));
    }
    Ok(())
}

fn validate_policy_groups(value: &str) -> Result<(), ApiError> {
    if value.is_empty() || value.len() > 1024 {
        return Err(ApiError::invalid("invalid policy groups"));
    }
    for group in value.split(',') {
        if group.is_empty()
            || !group
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
        {
            return Err(ApiError::invalid("invalid policy group name"));
        }
    }
    Ok(())
}

fn validate_hex(value: &str) -> Result<(), ApiError> {
    if value.is_empty()
        || value.len() > 8192
        || !value.len().is_multiple_of(2)
        || !value.chars().all(|c| c.is_ascii_hexdigit())
    {
        return Err(ApiError::invalid("expected an even-length hex string"));
    }
    Ok(())
}

fn validate_mode(mode: &str) -> Result<(), ApiError> {
    match mode {
        "enforce" | "complain" | "disable" | "enable" => Ok(()),
        _ => Err(ApiError::invalid(
            "mode must be enforce, complain, disable or enable",
        )),
    }
}

// ---- mutations ----------------------------------------------------------

async fn set_mode(path: &str, mode: &str) -> Result<(), ApiError> {
    validate_mode(mode)?;
    let path = validate_profile_path(path)?;
    // The aa-* tools parse the whole profile directory: leftovers from
    // 0.48.0 would make them fail with "Conflicting profiles".
    migrate_legacy_backups_logged();
    match mode {
        "enforce" => {
            let program = tool(&AA_ENFORCE, "aa-enforce")?;
            exec::run_root_program(program, &[&path], TIMEOUT).await?;
        }
        "complain" => {
            let program = tool(&AA_COMPLAIN, "aa-complain")?;
            exec::run_root_program(program, &[&path], TIMEOUT).await?;
        }
        "disable" => {
            let program = tool(&AA_DISABLE, "aa-disable")?;
            exec::run_root_program(program, &[&path], TIMEOUT).await?;
        }
        // No aa-enable exists: drop the disable/ symlink (aa-disable's
        // marker), then load the file again.
        "enable" => {
            let name = Path::new(&path)
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .ok_or_else(|| ApiError::invalid("profile has no file name"))?;
            // Remove EVERY disable/ marker that points at this file: the
            // link name is aa-disable's choice (basename or profile
            // name), never guess a single one.
            if let Ok(entries) = std::fs::read_dir(DISABLE_DIR) {
                for entry in entries.flatten() {
                    let link = entry.path();
                    if !link.is_symlink() {
                        continue;
                    }
                    let target = std::fs::read_link(&link)
                        .map(|t| t.to_string_lossy().into_owned())
                        .unwrap_or_default();
                    let same_name = entry.file_name().to_string_lossy() == name.as_str();
                    let same_target = target == path || target.ends_with(&format!("/{name}"));
                    if same_name || same_target {
                        std::fs::remove_file(&link).map_err(|e| {
                            ApiError::internal(format!("remove {}: {e}", link.display()))
                        })?;
                    }
                }
            }
            let program = tool(&AA_PARSER, "apparmor_parser")?;
            exec::run_root_program(program, &["-r", "-W", &path], TIMEOUT).await?;
        }
        _ => unreachable!("validate_mode"),
    }
    Ok(())
}

async fn profile_set(path: &str, content: &str, copies: usize) -> Result<(), ApiError> {
    let path = validate_profile_path(path)?;
    if content.len() > MAX_PROFILE_BYTES || content.contains('\0') {
        return Err(ApiError::invalid("profile content rejected (size or NUL)"));
    }
    let _guard = SET_LOCK.lock().await;
    migrate_legacy_backups_logged();
    // Safety chain: the NEW content must parse BEFORE anything is
    // written — the parser reads stdin when no profile argument is given.
    let parser = tool(&AA_PARSER, "apparmor_parser")?;
    let check = exec::run_root_program_tolerant(parser, &["-d"], TIMEOUT, Some(content)).await?;
    if check.code != 0 {
        return Err(ApiError::invalid(format!(
            "profile syntax check failed: {}",
            combine(&check.stdout, &check.stderr)
        )));
    }
    rotate_profile(&path, copies)?;
    crate::atomic::write(Path::new(&path), content)?;
    Ok(())
}

/// Back up one profile OUTSIDE `/etc/apparmor.d`: the `AppArmor` parser reads
/// every file in that directory, so `.ursine-backup` slots next to a profile
/// would be loaded as duplicate profiles. Slots live under
/// `/var/backups/ursine/apparmor` with the profile path encoded.
fn rotate_profile(path: &str, copies: usize) -> Result<(), ApiError> {
    let content = match std::fs::read(path) {
        Ok(content) => content,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => {
            return Err(ApiError::internal(format!(
                "read apparmor profile for backup: {error}"
            )))
        }
    };
    let target = backup_base(
        Path::new(BACKUP_DIR),
        Path::new(PROFILES_DIR),
        Path::new(path),
    );
    if let Some(dir) = target.parent() {
        std::fs::create_dir_all(dir).map_err(|e| {
            ApiError::internal(format!("create apparmor backup dir {}: {e}", dir.display()))
        })?;
    }
    crate::backup_configs::rotate_store(&target, copies, &content, 0o600)
}

async fn rotate_profiles_dir(copies: usize) -> Result<(), ApiError> {
    crate::backup_configs::rotate_async(PathBuf::from(PROFILES_DIR), copies, None).await
}

// ---- 0.48.0 leftover migration -----------------------------------------

/// Backup slot base of one profile: the profile's path relative to
/// `profiles_dir`, copied verbatim when it is already safe and with every
/// other character replaced by `%` — the slot chain then lives under `root`
/// with the same relative layout and never inside the profile directory.
fn backup_base(root: &Path, profiles_dir: &Path, path: &Path) -> PathBuf {
    let relative = path.strip_prefix(profiles_dir).unwrap_or(path);
    let encoded: String = relative
        .to_string_lossy()
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_') {
                c
            } else {
                '%'
            }
        })
        .collect();
    root.join(encoded)
}

/// One `*.ursine-backup*` file moved out of the profile directory.
#[derive(Debug, PartialEq, Eq)]
struct LegacyBackupMove {
    /// Slot that 0.48.0 wrote next to the profile.
    from: PathBuf,
    /// Where the slot lives now.
    to: PathBuf,
}

/// Result of one migration pass: files that moved and files that could not.
#[derive(Debug, Default)]
struct LegacyMigration {
    moved: Vec<LegacyBackupMove>,
    failed: Vec<(PathBuf, String)>,
}

/// Profile name a 0.48.0 slot file name belongs to, or `None` when the name
/// is not a slot. Only `<profile>.ursine-backup` and the numbered
/// `<profile>.ursine-backup.<n>` forms qualify, so a live profile is never
/// mistaken for a backup.
fn legacy_slot_profile(name: &str) -> Option<&str> {
    let (profile, suffix) = name.split_once(".ursine-backup")?;
    if profile.is_empty() {
        return None;
    }
    let slot = suffix.strip_prefix('.').map_or(suffix.is_empty(), |rest| {
        !rest.is_empty() && rest.chars().all(|c| c.is_ascii_digit())
    });
    // `foo.ursine-backup.ursine-backup` is rejected: the first match wins and
    // the tail is not a slot index.
    slot.then_some(profile)
}

/// Collect every slot file below `dir`, deepest subdirectories included.
/// Directories that merely carry a slot name are skipped: the caller must
/// never move or delete anything that is not a slot file.
fn collect_legacy_slots(dir: &Path, found: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(meta) = entry.metadata() else {
            continue;
        };
        if meta.is_dir() {
            collect_legacy_slots(&path, found);
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        if legacy_slot_profile(&name).is_some() {
            found.push(path);
        }
    }
}

/// Append `suffix` to the file name of `path` (`Path::with_extension` would
/// replace the slot extension instead).
fn with_name_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.as_os_str().to_os_string();
    name.push(suffix);
    PathBuf::from(name)
}

/// First free name for a migrated slot: `<slot>`, then `<slot>.migrated`,
/// `<slot>.migrated.1`, … so an existing backup is never overwritten.
fn free_slot_destination(dest: &Path) -> PathBuf {
    if !dest.exists() {
        return dest.to_path_buf();
    }
    let mut candidate = with_name_suffix(dest, ".migrated");
    let mut index = 1;
    while candidate.exists() && index < 1000 {
        candidate = with_name_suffix(dest, &format!(".migrated.{index}"));
        index += 1;
    }
    candidate
}

/// Move one slot, falling back to copy+unlink when `/etc` and `/var` are
/// separate file systems (`rename` cannot cross them).
fn move_slot(from: &Path, to: &Path) -> std::io::Result<()> {
    if let Some(parent) = to.parent() {
        std::fs::create_dir_all(parent)?;
    }
    match std::fs::rename(from, to) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::CrossesDevices => {
            std::fs::copy(from, to)?;
            // Only drop the source once the copy exists: a failed copy keeps
            // the only copy of the profile.
            std::fs::remove_file(from)
        }
        Err(error) => Err(error),
    }
}

/// Move every 0.48.0 leftover slot out of `profiles_dir` into `backup_root`.
///
/// 0.48.0 wrote `<profile>.ursine-backup` / `.ursine-backup.<n>` next to the
/// profiles, where the parser reads them as duplicate profiles; 0.49.0 writes
/// the slots under `backup_root` but never removed the old ones. Only files
/// whose name is such a slot are touched, the migration is idempotent, and a
/// name already taken in `backup_root` gains a `.migrated` suffix instead of
/// losing either copy.
fn migrate_legacy_backups(profiles_dir: &Path, backup_root: &Path) -> LegacyMigration {
    let mut slots = Vec::new();
    collect_legacy_slots(profiles_dir, &mut slots);
    let mut report = LegacyMigration::default();
    for from in slots {
        let Some(name) = from.file_name().map(|n| n.to_string_lossy().into_owned()) else {
            continue;
        };
        let Some(profile) = legacy_slot_profile(&name) else {
            continue;
        };
        // Keep the slot's own suffix: a numbered slot stays numbered next to
        // the same profile base. `strip_prefix` cannot split a character.
        let Some(slot_suffix) = name.strip_prefix(profile) else {
            continue;
        };
        let base = backup_base(backup_root, profiles_dir, &from.with_file_name(profile));
        let mut dest = with_name_suffix(&base, slot_suffix);
        if let Some(parent) = dest.parent() {
            if let Err(error) = std::fs::create_dir_all(parent) {
                report.failed.push((from, format!("{error}")));
                continue;
            }
        }
        dest = free_slot_destination(&dest);
        match move_slot(&from, &dest) {
            Ok(()) => report.moved.push(LegacyBackupMove { from, to: dest }),
            Err(error) => report.failed.push((from, format!("{error}"))),
        }
    }
    report
}

/// Run the migration on the real paths and log the outcome. Best effort on
/// purpose: a slot that cannot be moved (read-only `/etc`, foreign owner) must
/// not block a profile write — the parser reports the leftover itself.
fn migrate_legacy_backups_logged() {
    let report = migrate_legacy_backups(Path::new(PROFILES_DIR), Path::new(BACKUP_DIR));
    for moved in &report.moved {
        tracing::info!(
            from = %moved.from.display(),
            to = %moved.to.display(),
            "moved a leftover apparmor backup out of the profile directory"
        );
    }
    for (path, error) in &report.failed {
        tracing::warn!(
            path = %path.display(),
            error = %error,
            "cannot move a leftover apparmor backup out of the profile directory"
        );
    }
}

/// One button: `show_notifications="yes"` (line-surgical, backup first)
/// + the aa-notify autostart entry for the session user.
async fn notifications_enable(user: &str, copies: usize) -> Result<(), ApiError> {
    if crate::tooling::find(&AA_NOTIFY).is_none() {
        return Err(ApiError::unavailable(
            "aa-notify is not installed",
            "apk add apparmor-utils",
        ));
    }
    enable_notify_conf(copies).await?;
    write_autostart(user)?;
    Ok(())
}

async fn enable_notify_conf(copies: usize) -> Result<(), ApiError> {
    let current = match std::fs::read_to_string(NOTIFY_CONF) {
        Ok(raw) => raw,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(error) => return Err(ApiError::internal(format!("read {NOTIFY_CONF}: {error}"))),
    };
    let updated = set_show_notifications(&current);
    if updated != current {
        crate::backup_configs::rotate_async(PathBuf::from(NOTIFY_CONF), copies, None).await?;
        crate::atomic::write(Path::new(NOTIFY_CONF), &updated)?;
    }
    Ok(())
}

/// Flip only the `show_notifications` assignment; comments and every
/// other key stay byte-identical.
fn set_show_notifications(raw: &str) -> String {
    // Replace EVERY assignment (a later duplicate would override the
    // first in the consumer) and drop the extra lines.
    let mut lines: Vec<String> = Vec::new();
    let mut replaced = false;
    for line in raw.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("show_notifications") && trimmed.contains('=') {
            if !replaced {
                lines.push("show_notifications=\"yes\"".to_string());
                replaced = true;
            }
            continue;
        }
        lines.push(line.to_string());
    }
    if !replaced {
        if let Some(last) = lines.last() {
            if !last.trim().is_empty() {
                lines.push(String::new());
            }
        }
        lines.push("show_notifications=\"yes\"".to_string());
    }
    let mut out = lines.join("\n");
    if !out.ends_with('\n') {
        out.push('\n');
    }
    out
}

fn autostart_path(user: &str) -> Result<PathBuf, ApiError> {
    let (home, _uid, _gid) = crate::modules::theming::target_identity(user)?;
    Ok(home.join(AUTOSTART))
}

fn autostart_present(user: &str) -> bool {
    autostart_path(user).is_ok_and(|path| path.is_file())
}

fn write_autostart(user: &str) -> Result<(), ApiError> {
    use std::os::unix::fs::PermissionsExt;
    let (home, uid, gid) = crate::modules::theming::target_identity(user)?;
    let path = home.join(AUTOSTART);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| ApiError::internal(format!("create {}: {e}", parent.display())))?;
    }
    let body = "[Desktop Entry]\n\
        Type=Application\n\
        Name=AppArmor DENIED notifications\n\
        Comment=Desktop notifications for AppArmor denials\n\
        Exec=aa-notify -p\n\
        Terminal=false\n\
        NoDisplay=true\n\
        X-GNOME-Autostart-enabled=true\n";
    let tmp = path.with_extension(format!("new.{}", std::process::id()));
    std::fs::write(&tmp, body)
        .map_err(|e| ApiError::internal(format!("write {}: {e}", tmp.display())))?;
    std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o644))
        .map_err(|e| ApiError::internal(format!("chmod {}: {e}", tmp.display())))?;
    let cleanup = |error: ApiError| -> ApiError {
        let _ = std::fs::remove_file(&tmp);
        error
    };
    std::os::unix::fs::chown(&tmp, Some(uid), Some(gid))
        .map_err(|e| cleanup(ApiError::internal(format!("chown {}: {e}", tmp.display()))))?;
    std::fs::rename(&tmp, &path).map_err(|e| {
        cleanup(ApiError::internal(format!(
            "replace {}: {e}",
            path.display()
        )))
    })
}

fn notify_configured() -> bool {
    std::fs::read_to_string(NOTIFY_CONF).is_ok_and(|raw| {
        raw.lines().any(|line| {
            let line = line.trim();
            line.starts_with("show_notifications") && line.contains('=') && line.contains("\"yes\"")
        })
    })
}

// ---- helpers ------------------------------------------------------------

/// Serializes writers inside this daemon (one rotation+replace at a
/// time — concurrent sets would race the tmp file and the slots).
static SET_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

fn combine(stdout: &str, stderr: &str) -> String {
    let mut out = String::new();
    for stream in [stdout, stderr] {
        let stream = stream.trim();
        if stream.is_empty() {
            continue;
        }
        if !out.is_empty() {
            out.push('\n');
        }
        out.push_str(stream);
    }
    out
}

fn tool(candidates: &[&'static str], label: &str) -> Result<&'static str, ApiError> {
    crate::tooling::find(candidates).ok_or_else(|| {
        ApiError::unavailable(
            format!("{label} not found"),
            "apk add apparmor apparmor-utils",
        )
    })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::disallowed_methods)] // tests use the host temp dir

    use super::*;

    /// Fresh empty directory under the host temp dir (test only).
    fn temp(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("ursine-apparmor-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    const STATUS_TEXT: &str = "apparmor module is loaded.\n\
        110 profiles are loaded.\n\
        102 profiles are in enforce mode.\n\
        \x20\x20\x20/usr/bin/foo\n\
        \x20\x20\x20/usr/sbin/bar\n\
        8 profiles are in complain mode.\n\
        \x20\x20\x20/usr/bin/baz\n\
        Out of 129 processes running:\n\
        13 processes have profiles defined.\n\
        8 processes have profiles in enforce mode.\n\
        \x20\x20\x20/usr/bin/foo (100)\n\
        5 processes have profiles in complain mode.\n\
        \x20\x20\x20/usr/bin/baz (200)\n\
        0 processes are unconfined but have a profile defined.\n";

    #[test]
    fn parses_loaded_status_with_modes_and_processes() {
        let status = parse_status(STATUS_TEXT);
        assert!(status.enabled);
        assert!(status.mounted);
        assert_eq!(status.profiles.len(), 3);
        assert_eq!(status.profiles.first().unwrap().name, "/usr/bin/foo");
        assert_eq!(status.profiles.first().unwrap().mode, "enforce");
        assert_eq!(status.profiles.get(2).unwrap().mode, "complain");
        assert_eq!(status.processes_defined, 13);
        assert_eq!(status.processes_enforce, 8);
        assert_eq!(status.processes_complain, 5);
    }

    #[test]
    fn parses_not_loaded_and_unmounted_states() {
        let not_loaded = parse_status("apparmor module is not loaded.\n");
        assert!(!not_loaded.enabled);
        // No positive evidence → not claimed as mounted.
        assert!(!not_loaded.mounted);
        let empty = parse_status("");
        assert!(!empty.enabled);
        assert!(!empty.mounted);
        // Only the unmounted notice: enabled cannot be confirmed.
        let unmounted = parse_status("apparmor filesystem is not mounted.\n");
        assert!(!unmounted.enabled);
        assert!(!unmounted.mounted);
        let no_policy = parse_status("apparmor module is loaded.\n0 profiles are loaded.\n");
        assert!(no_policy.enabled);
        assert!(no_policy.profiles.is_empty());
    }

    #[test]
    fn validates_profile_paths() {
        assert!(validate_profile_path("/etc/apparmor.d/usr.bin.foo").is_err()); // missing file in CI
        assert!(validate_profile_path("/etc/passwd").is_err());
        assert!(validate_profile_path("/etc/apparmor.d/../passwd").is_err());
        assert!(validate_profile_path("usr.bin.foo").is_err());
        assert!(validate_profile_path("/etc/apparmor.d/").is_err());
    }

    #[test]
    fn profile_key_normalizes_path_style_names() {
        assert_eq!(profile_key("/usr/bin/foo"), "usr.bin.foo");
        assert_eq!(profile_key("usr.bin.foo"), "usr.bin.foo");
        assert_eq!(profile_key("docker-default"), "docker-default");
        assert_eq!(profile_key("/usr/sbin/dhclient"), "usr.sbin.dhclient");
    }

    #[test]
    fn validates_easyprof_arguments() {
        assert!(validate_binary("/opt/foo/**").is_ok());
        assert!(validate_binary("/opt/my app/bin").is_ok());
        assert!(validate_binary("").is_err());
        assert!(validate_binary("bad\npath").is_err());
        assert!(validate_template("user-application").is_ok());
        assert!(validate_template("bad template").is_err());
        assert!(validate_policy_groups("networking,user-application").is_ok());
        assert!(validate_policy_groups("networking, bad").is_err());
        assert!(validate_policy_groups("").is_err());
        assert!(validate_template_var("@{APPNAME}=scanner-app").is_ok());
        assert!(validate_template_var("@{APP_NAME}=/opt/tools/scanner-app").is_ok());
        assert!(validate_template_var("APPNAME=scanner-app").is_err());
        assert!(validate_template_var("@{APP-NAME}=x").is_err());
        assert!(validate_template_var("@{APPNAME}=a=b").is_err());
        assert!(validate_template_var("@{APPNAME}=../etc").is_err());
        assert!(validate_template_var("@{APPNAME}=say \"hi\"").is_err());
    }

    #[test]
    fn validates_targets_and_hex_and_modes() {
        assert!(validate_target("usr.bin.foo").is_ok());
        assert!(validate_target("/usr/bin/foo").is_ok());
        assert!(validate_target("../etc/passwd").is_err());
        assert!(validate_target("").is_err());
        assert!(validate_hex("2F746D70").is_ok());
        assert!(validate_hex("abc").is_err());
        assert!(validate_hex("zz").is_err());
        assert!(validate_mode("enforce").is_ok());
        assert!(validate_mode("drop").is_err());
    }

    #[test]
    fn notify_conf_flip_preserves_comments_and_other_keys() {
        let raw = "# keep me\nshow_notifications=\"no\"  # old\nuse_group=\"admin\"\n";
        let next = set_show_notifications(raw);
        assert!(next.contains("# keep me"));
        assert!(next.contains("show_notifications=\"yes\""));
        assert!(!next.contains("\"no\""));
        assert!(next.contains("use_group=\"admin\""));
        // Missing file → the one line, newline-terminated.
        assert_eq!(set_show_notifications(""), "show_notifications=\"yes\"\n");
        // Idempotent.
        assert_eq!(set_show_notifications(&next), next);
    }

    #[test]
    fn autostart_body_is_a_valid_desktop_entry() {
        let body = "[Desktop Entry]\nType=Application\nExec=aa-notify -p\n";
        assert!(body.starts_with("[Desktop Entry]"));
        assert!(body.contains("Exec=aa-notify -p"));
    }

    #[test]
    fn backup_base_mirrors_the_profile_path_under_the_backup_root() {
        let root = Path::new("/var/backups/ursine/apparmor");
        let profiles = Path::new(PROFILES_DIR);
        assert_eq!(
            backup_base(root, profiles, Path::new("/etc/apparmor.d/usr.bin.jq")),
            root.join("usr.bin.jq")
        );
        // Subdirectories keep the layout with `%` in place of the separator.
        assert_eq!(
            backup_base(root, profiles, Path::new("/etc/apparmor.d/containers/foo")),
            root.join("containers%foo")
        );
    }

    #[test]
    fn only_slot_file_names_are_recognised() {
        assert_eq!(
            legacy_slot_profile("usr.bin.jq.ursine-backup"),
            Some("usr.bin.jq")
        );
        assert_eq!(
            legacy_slot_profile("usr.bin.jq.ursine-backup.1"),
            Some("usr.bin.jq")
        );
        assert_eq!(
            legacy_slot_profile("usr.bin.jq.ursine-backup.12"),
            Some("usr.bin.jq")
        );
        // A live profile, an unrelated sibling and a malformed slot stay put.
        assert_eq!(legacy_slot_profile("usr.bin.jq"), None);
        assert_eq!(legacy_slot_profile("usr.bin.jq.ursine-backups"), None);
        assert_eq!(legacy_slot_profile("usr.bin.jq.ursine-backup.old"), None);
        assert_eq!(legacy_slot_profile("usr.bin.jq.ursine-backup."), None);
        assert_eq!(legacy_slot_profile(".ursine-backup"), None);
    }

    #[test]
    fn migration_moves_slots_out_of_the_profile_directory() {
        let profiles = temp("migrate-profiles");
        let backups = temp("migrate-backups");
        std::fs::write(profiles.join("usr.bin.jq"), "profile\n").unwrap();
        std::fs::write(profiles.join("usr.bin.jq.ursine-backup"), "newest\n").unwrap();
        std::fs::write(profiles.join("usr.bin.jq.ursine-backup.1"), "older\n").unwrap();
        std::fs::write(profiles.join("usr.bin.keep.ursine-backups"), "not a slot\n").unwrap();

        let report = migrate_legacy_backups(&profiles, &backups);

        assert!(report.failed.is_empty(), "{:?}", report.failed);
        assert_eq!(report.moved.len(), 2);
        // The profile itself stays, the slots are gone from its directory.
        assert!(profiles.join("usr.bin.jq").is_file());
        assert!(!profiles.join("usr.bin.jq.ursine-backup").exists());
        assert!(!profiles.join("usr.bin.jq.ursine-backup.1").exists());
        // Not a slot: untouched.
        assert!(profiles.join("usr.bin.keep.ursine-backups").is_file());
        // Both slots now live under the backup root, suffix preserved.
        assert_eq!(
            std::fs::read_to_string(backups.join("usr.bin.jq.ursine-backup")).unwrap(),
            "newest\n"
        );
        assert_eq!(
            std::fs::read_to_string(backups.join("usr.bin.jq.ursine-backup.1")).unwrap(),
            "older\n"
        );
        let _ = std::fs::remove_dir_all(&profiles);
        let _ = std::fs::remove_dir_all(&backups);
    }

    #[test]
    fn migration_keeps_both_copies_when_the_slot_name_is_taken() {
        let profiles = temp("migrate-occupied-profiles");
        let backups = temp("migrate-occupied-backups");
        std::fs::write(profiles.join("usr.bin.jq.ursine-backup"), "leftover\n").unwrap();
        std::fs::write(backups.join("usr.bin.jq.ursine-backup"), "current\n").unwrap();

        let report = migrate_legacy_backups(&profiles, &backups);

        assert_eq!(
            report.moved.first().map(|moved| moved.to.clone()),
            Some(backups.join("usr.bin.jq.ursine-backup.migrated"))
        );
        assert_eq!(
            std::fs::read_to_string(backups.join("usr.bin.jq.ursine-backup")).unwrap(),
            "current\n"
        );
        assert_eq!(
            std::fs::read_to_string(backups.join("usr.bin.jq.ursine-backup.migrated")).unwrap(),
            "leftover\n"
        );
        let _ = std::fs::remove_dir_all(&profiles);
        let _ = std::fs::remove_dir_all(&backups);
    }

    #[test]
    fn migration_is_idempotent_and_silent_without_slots() {
        let profiles = temp("migrate-idempotent-profiles");
        let backups = temp("migrate-idempotent-backups");
        // Nothing to migrate: empty report, nothing created beyond the roots.
        let quiet = migrate_legacy_backups(&profiles, &backups);
        assert!(quiet.moved.is_empty() && quiet.failed.is_empty());
        assert_eq!(std::fs::read_dir(&backups).unwrap().count(), 0);

        std::fs::write(profiles.join("usr.bin.jq"), "profile\n").unwrap();
        std::fs::write(profiles.join("usr.bin.jq.ursine-backup"), "slot\n").unwrap();
        assert_eq!(migrate_legacy_backups(&profiles, &backups).moved.len(), 1);
        // Second pass finds nothing left to move.
        let again = migrate_legacy_backups(&profiles, &backups);
        assert!(again.moved.is_empty() && again.failed.is_empty());
        assert!(profiles.join("usr.bin.jq").is_file());
        let _ = std::fs::remove_dir_all(&profiles);
        let _ = std::fs::remove_dir_all(&backups);
    }
}
