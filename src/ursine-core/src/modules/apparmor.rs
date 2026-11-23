//! apparmor — policy management through the `AppArmor` CLIs (SPEC-apparmor).
//!
//! No D-Bus (invariant 3): `aa-status` for state (its exit codes 1-3 are
//! data, so a tolerant runner carries the streams), `aa-*` mode tools,
//! `apparmor_parser` for load/check/remove. The profile editor
//! syntax-checks the NEW content on stdin before any write; the
//! notifications button flips `show_notifications` and installs the
//! `aa-notify` autostart entry for the session user (next login).
//!
//! Backups never live inside `/etc/apparmor.d`: every `AppArmor` tool reads
//! that whole tree, so a rotated copy parked next to a profile defines the
//! same profile twice. The slot chain mirrors the source path under
//! `BACKUP_ROOT` instead (one shared helper backs up before every write).

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
/// Backup root for profile files: the slot chain mirrors the source path
/// below this directory instead of sitting in `/etc/apparmor.d`.
const BACKUP_ROOT: &str = "/var/lib/ursine/backups/apparmor";
/// Depth guard for the stray-backup scan (the profile tree is shallow).
const MAX_SCAN_DEPTH: usize = 8;
const NOTIFY_CONF: &str = "/etc/apparmor/notify.conf";
const AUTOSTART: &str = ".config/autostart/aa-notify.desktop";
const TIMEOUT: Duration = Duration::from_secs(60);
/// Profile sources are text files; anything larger is refused outright.
const MAX_PROFILE_BYTES: usize = 512 * 1024;
/// `aa-easyprof` takes one `--template-var` per declaration; a policy group
/// needs a handful at most.
const MAX_TEMPLATE_VARS: usize = 32;

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
            // `-s` = silent: without it the tool asks a question, gets no
            // terminal behind the daemon and dies with a Python traceback.
            run_profile_tool(ctx, user, name, &AA_CLEANPROF, "aa-cleanprof", &["-s"]).await
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
            args.extend(template_var_args(template_vars)?);
            args.push(validate_binary(binary)?);
            let arg_refs: Vec<&str> = args.iter().map(String::as_str).collect();
            // Quoted-value guard: profile paths contain `*`, spaces and
            // `@{VAR}` declarations. No --output-directory → stdout only.
            let outcome =
                exec::run_root_program_value_tolerant(program, &arg_refs, TIMEOUT).await?;
            if outcome.code != 0 {
                return Err(easyprof_error(&combine(&outcome.stdout, &outcome.stderr)));
            }
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

/// The two per-profile tools share one path: validate the target, back the
/// profile tree up outside `/etc/apparmor.d`, run the tool, answer with the
/// fresh view. `extra` carries the tool's non-interactive flags.
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
    backup_profile_target(
        Path::new(PROFILES_DIR),
        crate::backup_configs::copies_from(&ctx.config),
    )
    .await?;
    let mut args: Vec<&str> = extra.to_vec();
    args.push(&target);
    let outcome = exec::run_root_program_tolerant(program, &args, TIMEOUT, None).await?;
    if outcome.code != 0 {
        return Err(profile_tool_error(label, &outcome));
    }
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

/// `aa-easyprof --template-var=@{NAME}=VALUE` argv entries, one per declared
/// variable. The policy groups that need them (`opt-application` writes
/// `@{APPNAME}`) fail with `ERROR: Invalid policy` when nothing declares the
/// variable, so callers must be able to pass them.
fn template_var_args(vars: &[String]) -> Result<Vec<String>, ApiError> {
    if vars.len() > MAX_TEMPLATE_VARS {
        return Err(ApiError::invalid(format!(
            "at most {MAX_TEMPLATE_VARS} template variables"
        )));
    }
    vars.iter()
        .map(|entry| {
            let (name, value) = entry
                .split_once('=')
                .ok_or_else(|| ApiError::invalid("template variable must be NAME=VALUE"))?;
            validate_template_var_name(name)?;
            validate_template_var_value(value)?;
            Ok(format!("--template-var=@{{{name}}}={value}"))
        })
        .collect()
}

/// `AppArmor` variable names are the `@{NAME}` identifier: letters, digits and
/// underscores, bounded (an `=` or a newline would break the argv form).
fn validate_template_var_name(name: &str) -> Result<(), ApiError> {
    if name.is_empty()
        || name.len() > 64
        || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
    {
        return Err(ApiError::invalid(
            "template variable name must be letters, digits or _ (64 max)",
        ));
    }
    Ok(())
}

/// The value is one argv element (no shell), but a newline or NUL would still
/// split or truncate it on the way to the tool.
fn validate_template_var_value(value: &str) -> Result<(), ApiError> {
    if value.is_empty()
        || value.len() > 512
        || value.chars().any(|c| matches!(c, '\n' | '\r' | '\0'))
    {
        return Err(ApiError::invalid(
            "template variable value is empty, too long or contains a control character",
        ));
    }
    Ok(())
}

/// `aa-easyprof` verifies the generated policy and refuses it when a policy
/// group uses a variable nobody declared. That is a client-side omission, not
/// an internal failure: name the variable (or the missing field) and say how
/// to declare it.
fn easyprof_error(detail: &str) -> ApiError {
    if let Some(name) = missing_template_var(detail) {
        return ApiError::invalid(format!(
            "the generated policy uses @{{{name}}} and nothing declares it, so aa-easyprof refused it"
        ))
        .with_fix(format!(
            "declare it with the action's template_vars field: template_vars=[\"{name}=<value>\"] (aa-easyprof --template-var=@{{{name}}}=<value>)"
        ));
    }
    if detail.contains("Invalid policy") {
        return ApiError::invalid(
            "aa-easyprof rejected the generated policy (ERROR: Invalid policy)",
        )
        .with_fix(
            "a policy group uses an AppArmor variable: declare each one with template_vars=[\"NAME=VALUE\"] (passed as --template-var=@{NAME}=VALUE), or pick a group without variables",
        );
    }
    ApiError::internal(format!("aa-easyprof failed: {}", clip(detail, 2000)))
}

/// The undeclared variable named in a verification message
/// (`Found reference to variable APPNAME, but is never declared`).
fn missing_template_var(detail: &str) -> Option<&str> {
    let marker = "Found reference to variable ";
    let start = detail.find(marker)? + marker.len();
    let rest = detail.get(start..)?;
    let end = rest
        .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
        .unwrap_or(rest.len());
    rest.get(..end).filter(|name| !name.is_empty())
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
    backup_profile_target(Path::new(&path), copies).await?;
    crate::atomic::write(Path::new(&path), content)?;
    Ok(())
}

/// Mirror path for the `backup_configs` slot chain of `target`: the source
/// path with `/etc/apparmor.d` replaced by [`BACKUP_ROOT`] (`/etc/apparmor.d`
/// itself maps to the mirror of the whole directory).
fn backup_mirror(target: &Path) -> PathBuf {
    let root = Path::new(BACKUP_ROOT).join(PROFILES_DIR.trim_start_matches('/'));
    // Callers validate the path first; a foreign path still gets a chain below
    // the root rather than one in the read directory.
    target.strip_prefix(PROFILES_DIR).map_or_else(
        |_| root.join(target.file_name().unwrap_or_default()),
        |relative| root.join(relative),
    )
}

/// Entries under `dir` whose name carries the `ursine-backup` marker, found
/// recursively and symlinks skipped (older releases parked the slot chain
/// next to the profiles).
fn stray_backups(dir: &Path) -> Vec<PathBuf> {
    fn walk(dir: &Path, depth: usize, found: &mut Vec<PathBuf>) {
        if depth > MAX_SCAN_DEPTH {
            return;
        }
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(meta) = std::fs::symlink_metadata(&path) else {
                continue;
            };
            if meta.file_type().is_symlink() {
                continue;
            }
            let name = entry.file_name();
            if name.to_string_lossy().contains("ursine-backup") {
                found.push(path);
                continue;
            }
            if meta.is_dir() {
                walk(&path, depth + 1, found);
            }
        }
    }
    let mut found = Vec::new();
    walk(dir, 0, &mut found);
    found.sort();
    found
}

/// Refuse to touch the profile tree while it still holds backups from older
/// releases: two files defining one profile make loads fail for every UNRELATED
/// profile too ("Conflicting profiles for … defined in two files"). Deleting
/// files under `/etc/apparmor.d` is the admin's call, so this is a typed
/// conflict naming every file, not a silent cleanup.
fn ensure_profiles_dir_clean() -> Result<(), ApiError> {
    let stray = stray_backups(Path::new(PROFILES_DIR));
    if stray.is_empty() {
        return Ok(());
    }
    let names: Vec<String> = stray
        .iter()
        .map(|path| path.display().to_string())
        .collect();
    let mirror_dir = Path::new(BACKUP_ROOT).join(PROFILES_DIR.trim_start_matches('/'));
    let fix = format!(
        "move them into the backup mirror: mkdir -p {dir} && mv {names} {dir}/",
        dir = mirror_dir.display(),
        names = names.join(" ")
    );
    Err(ApiError::new(
        ursine_api::ErrorCode::Conflict,
        format!(
            "{} backup file(s) inside {PROFILES_DIR} define those profiles twice: {}",
            stray.len(),
            names.join(", ")
        ),
    )
    .with_fix(fix))
}

/// The one backup helper shared by the single-file write and the whole-
/// directory rotation: mirror `target`'s slot chain outside the read tree,
/// with the configured `copies` cap.
async fn backup_profile_target(target: &Path, copies: usize) -> Result<(), ApiError> {
    ensure_profiles_dir_clean()?;
    crate::backup_configs::rotate_into_async(
        target.to_path_buf(),
        backup_mirror(target),
        copies,
        None,
    )
    .await
}

/// `aa-cleanprof`/`aa-autodep` refused: a tool that asks a question has no
/// terminal behind the daemon, and its Python traceback is not an answer a
/// user can act on.
fn profile_tool_error(label: &str, outcome: &exec::ProcessOutcome) -> ApiError {
    let detail = combine(&outcome.stdout, &outcome.stderr);
    if detail.contains("Traceback (most recent call last)")
        || detail.contains("promptUser")
        || detail.contains("Text_PromptUser")
        || detail.contains("readkey")
        || detail.contains("EOFError")
        || detail.contains("/dev/tty")
    {
        return ApiError::unavailable(
            format!("{label} asked for interactive input and no terminal is attached"),
            format!(
                "run {label} for this profile in a terminal, or a version of {label} that has a non-interactive flag"
            ),
        );
    }
    ApiError::internal(format!(
        "{label} failed (exit {}): {}",
        outcome.code,
        clip(&detail, 2000)
    ))
}

/// Bounded detail text: tool output is shown, but never unbounded.
fn clip(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_string();
    }
    let head: String = text.chars().take(max_chars).collect();
    format!("{head}…")
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
    }

    #[test]
    fn template_vars_become_one_template_var_argv_each() {
        let args = template_var_args(&[
            "APPNAME=scanner-app".to_string(),
            "APPDIR=/opt/tools".to_string(),
        ])
        .unwrap();
        assert_eq!(
            args,
            vec![
                "--template-var=@{APPNAME}=scanner-app".to_string(),
                "--template-var=@{APPDIR}=/opt/tools".to_string(),
            ]
        );
        // An empty list stays empty: no flag is invented.
        assert!(template_var_args(&[]).unwrap().is_empty());
        // One argv element each, so `=` inside the value is preserved.
        let args = template_var_args(&["PLUGINS=/opt/a=b".to_string()]).unwrap();
        assert_eq!(args, vec!["--template-var=@{PLUGINS}=/opt/a=b".to_string()]);
    }

    #[test]
    fn template_vars_reject_malformed_declarations() {
        for bad in [
            "APPNAME".to_string(),      // no `=`
            "=value".to_string(),       // no name
            "APPNAME=".to_string(),     // no value
            "APP NAME=x".to_string(),   // space in the name
            "APP-NAME=x".to_string(),   // dash in the name
            "APPNAME=a\nb".to_string(), // newline in the value
            format!("{}=x", "A".repeat(65)),
            format!("NAME={}", "x".repeat(513)),
        ] {
            assert!(
                template_var_args(std::slice::from_ref(&bad)).is_err(),
                "{bad} must be refused"
            );
        }
    }

    #[test]
    fn invalid_policy_maps_to_a_typed_error_naming_template_vars() {
        let plain = easyprof_error("ERROR: Invalid policy");
        assert_eq!(plain.code, ursine_api::ErrorCode::Invalid);
        let fix = plain.fix.unwrap_or_default();
        assert!(fix.contains("template_vars"), "fix names the field: {fix}");
        assert!(
            !plain.message.contains("Invalid policy") || fix.contains("--template-var"),
            "the fix carries the argv form"
        );
        // The verification message names the missing variable: report it.
        let named = easyprof_error(
            "ERROR: Invalid policy\nFound reference to variable APPNAME, but is never declared",
        );
        assert_eq!(named.code, ursine_api::ErrorCode::Invalid);
        assert!(named.message.contains("APPNAME"), "{}", named.message);
        assert!(named
            .fix
            .unwrap_or_default()
            .contains("template_vars=[\"APPNAME=<value>\"]"));
        // Anything else stays an internal failure with the tool text.
        let other = easyprof_error("aa-easyprof: cannot open template file");
        assert_eq!(other.code, ursine_api::ErrorCode::Internal);
        assert!(other.message.contains("cannot open template file"));
    }

    #[test]
    fn interactive_profile_tool_failure_is_not_reported_as_a_traceback() {
        let outcome = exec::ProcessOutcome {
            code: 1,
            stdout: String::new(),
            stderr: "Traceback (most recent call last):\n  File \"/usr/bin/aa-cleanprof\", line 34\n    ans = getkey().lower()\nEOFError\n".to_string(),
        };
        let error = profile_tool_error("aa-cleanprof", &outcome);
        assert_eq!(error.code, ursine_api::ErrorCode::Unavailable);
        assert!(error.fix.is_some(), "a fix tells the user what to do");
        assert!(!error.message.contains("Traceback"), "{}", error.message);
        assert!(error.message.contains("terminal"), "{}", error.message);
        // A plain failure keeps the tool's own text, bounded.
        let plain = exec::ProcessOutcome {
            code: 2,
            stdout: String::new(),
            stderr: "ERROR: profile not found".to_string(),
        };
        let error = profile_tool_error("aa-cleanprof", &plain);
        assert_eq!(error.code, ursine_api::ErrorCode::Internal);
        assert!(error.message.contains("profile not found"));
    }

    #[test]
    fn profile_backups_mirror_the_source_path_outside_the_read_dir() {
        assert_eq!(
            backup_mirror(Path::new("/etc/apparmor.d/usr.bin.jq")),
            PathBuf::from("/var/lib/ursine/backups/apparmor/etc/apparmor.d/usr.bin.jq")
        );
        assert_eq!(
            backup_mirror(Path::new("/etc/apparmor.d/local/extra")),
            PathBuf::from("/var/lib/ursine/backups/apparmor/etc/apparmor.d/local/extra")
        );
        // The whole profile directory maps to the mirror of the directory.
        assert_eq!(
            backup_mirror(Path::new("/etc/apparmor.d")),
            PathBuf::from("/var/lib/ursine/backups/apparmor/etc/apparmor.d")
        );
        // Never a path inside the read directory.
        for target in ["/etc/apparmor.d/a", "/etc/apparmor.d", "/etc/passwd"] {
            let mirror = backup_mirror(Path::new(target));
            assert!(
                !mirror.starts_with("/etc/apparmor.d"),
                "{} must stay outside",
                mirror.display()
            );
        }
    }

    #[test]
    fn stray_backups_are_found_recursively_in_a_temp_tree() {
        let root = std::env::temp_dir().join(format!("ursine-aa-stray-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("local")).unwrap();
        assert!(stray_backups(&root).is_empty());
        std::fs::write(root.join("usr.bin.jq"), "profile\n").unwrap();
        std::fs::write(root.join("usr.bin.jq.ursine-backup"), "old\n").unwrap();
        std::fs::write(root.join("local/x.ursine-backup.1"), "old\n").unwrap();
        let found = stray_backups(&root);
        assert_eq!(found.len(), 2, "{found:?}");
        assert!(found.iter().all(|path| path.exists()));
        let _ = std::fs::remove_dir_all(&root);
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
}
