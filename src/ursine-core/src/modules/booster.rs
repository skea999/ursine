//! booster — initramfs config editing + regeneration (SPEC-booster).
//!
//! Booster-only (user decision 2026-11-10): the module edits
//! `/etc/booster.yaml` surgically — we own the known keys, comments,
//! unknown keys and the installer marker stay byte-identical — and
//! regenerates through the distribution's own kernel-update process
//! first (`apk fix kernel-hooks` runs every executable in
//! `/etc/kernel-hooks.d`, e.g. `FeralOS` ships `50-booster.hook` = booster
//! build + signed UKI + fallbacks), falling back to `booster build`.

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::json;
use ursine_api::{
    Action, ApiError, BoosterConfig, BoosterConfigPatch, BoosterRegenerateReport, CommandOutcome,
};

use super::ModuleCtx;
use crate::modules::packages_nix::exec;

const BOOSTER_YAML: &str = "/etc/booster.yaml";
const HOOKS_DIR: &str = "/etc/kernel-hooks.d";
const REGENERATE_IMAGES: &str = "/usr/lib/booster/regenerate_images";
const MODULES_DIR: &str = "/lib/modules";
const BOOSTER: [&str; 4] = [
    "/usr/bin/booster",
    "/usr/sbin/booster",
    "/bin/booster",
    "/sbin/booster",
];
const TIMEOUT: Duration = Duration::from_secs(120);

/// Top-level scalar keys we own (values are raw YAML scalar text).
const SCALAR_KEYS: [&str; 13] = [
    "compression",
    "universal",
    "strip",
    "vconsole",
    "enable_lvm",
    "enable_mdraid",
    "append_all_modaliases",
    "mount_timeout",
    "token_timeout",
    "pin_delay",
    "extra_files",
    "modules",
    "modules_force_load",
];
/// Keys under the `network:` section we own.
const NETWORK_KEYS: [&str; 5] = ["dhcp", "ip", "gateway", "dns_servers", "interfaces"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Scope {
    Top,
    Network,
}

pub async fn handle(action: &Action, ctx: &ModuleCtx<'_>) -> Result<serde_json::Value, ApiError> {
    match action {
        Action::BoosterConfigGet => {
            let mut view = config_get()?;
            view.regenerate_step = select_step().await.label();
            Ok(json!(view))
        }
        Action::BoosterConfigSet { keys } => {
            let view = config_set(keys, crate::backup_configs::copies_from(&ctx.config)).await?;
            Ok(json!(view))
        }
        Action::BoosterRegenerate => Ok(json!(regenerate().await?)),
        _ => Err(ApiError::invalid("action is not valid for module booster")),
    }
}

// ---- config get/set -----------------------------------------------------

fn config_get() -> Result<BoosterConfig, ApiError> {
    let (exists, raw) = match std::fs::read_to_string(BOOSTER_YAML) {
        Ok(raw) => (true, raw),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => (false, String::new()),
        Err(error) => return Err(ApiError::internal(format!("read {BOOSTER_YAML}: {error}"))),
    };
    let mut view = BoosterConfig {
        exists,
        raw: raw.clone(),
        ..BoosterConfig::default()
    };
    parse_into(&raw, &mut view);
    Ok(view)
}

async fn config_set(patch: &BoosterConfigPatch, copies: usize) -> Result<BoosterConfig, ApiError> {
    let current = match std::fs::read_to_string(BOOSTER_YAML) {
        Ok(raw) => raw,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(error) => return Err(ApiError::internal(format!("read {BOOSTER_YAML}: {error}"))),
    };
    let next = apply(&current, patch)?;
    if next != current {
        // Binding write order: rotate first (fail-closed), validate the
        // complete result, then replace atomically. One writer at a time.
        let _guard = SET_LOCK.lock().await;
        crate::backup_configs::rotate_async(PathBuf::from(BOOSTER_YAML), copies, None).await?;
        serde_yaml::from_str::<serde_yaml::Mapping>(&next)
            .map_err(|e| ApiError::invalid(format!("resulting YAML invalid: {e}")))?;
        write_atomic(Path::new(BOOSTER_YAML), &next)?;
    }
    config_get()
}

/// Parse the known keys out of the raw file (line-based; comments and
/// unknown content are ignored for values but kept in `raw`).
fn parse_into(raw: &str, view: &mut BoosterConfig) {
    let mut in_network = false;
    for line in raw.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let indented = line.starts_with(' ') || line.starts_with('\t');
        let Some((key, rest)) = trimmed.split_once(':') else {
            // A top-level non-mapping line ends any open section.
            if !indented {
                in_network = false;
            }
            continue;
        };
        let key = key.trim();
        if !indented {
            if rest.trim().is_empty() || rest.trim().starts_with('#') {
                in_network = key == "network";
                continue;
            }
            in_network = false;
            let value = scalar_value(rest);
            match key {
                "compression" => view.compression = Some(value),
                "universal" => view.universal = Some(value),
                "strip" => view.strip = Some(value),
                "vconsole" => view.vconsole = Some(value),
                "enable_lvm" => view.enable_lvm = Some(value),
                "enable_mdraid" => view.enable_mdraid = Some(value),
                "append_all_modaliases" => view.append_all_modaliases = Some(value),
                "mount_timeout" => view.mount_timeout = Some(value),
                "token_timeout" => view.token_timeout = Some(value),
                "pin_delay" => view.pin_delay = Some(value),
                "extra_files" => view.extra_files = Some(value),
                "modules" => view.modules = Some(value),
                "modules_force_load" => view.modules_force_load = Some(value),
                _ => {}
            }
        } else if in_network {
            let value = scalar_value(rest);
            match key {
                "dhcp" => view.network_dhcp = Some(value),
                "ip" => view.network_ip = Some(value),
                "gateway" => view.network_gateway = Some(value),
                "dns_servers" => view.network_dns_servers = Some(value),
                "interfaces" => view.network_interfaces = Some(value),
                _ => {}
            }
        }
    }
}

/// Trim the raw scalar after `:` — inline ` # comments` are not part of
/// the value (they stay on the line when we write the key back).
/// The trailing ` # …` segment of a mapping line's value (kept verbatim
/// when we rewrite the key so inline comments survive).
fn trailing_comment(rest: &str) -> String {
    let rest = rest.trim_start();
    if rest.starts_with('"') || rest.starts_with('\'') {
        return String::new();
    }
    let Some(at) = rest.find(" #") else {
        return String::new();
    };
    // Include every space before the `#` so the comment segment keeps
    // its original spacing byte-for-byte.
    let mut start = at;
    while start > 0 && rest.get(..start).is_some_and(|head| head.ends_with(' ')) {
        start -= 1;
    }
    rest.get(start..).unwrap_or_default().to_string()
}

fn scalar_value(rest: &str) -> String {
    let rest = rest.trim();
    if rest.starts_with('"') || rest.starts_with('\'') {
        // Quoted scalar: keep the bytes (a `#` inside quotes is data).
        return rest.to_string();
    }
    rest.find(" #").map_or_else(
        || rest.to_string(),
        |at| rest.get(..at).unwrap_or(rest).trim().to_string(),
    )
}

fn validate_value(value: &str) -> Result<(), ApiError> {
    // Empty would write `key:` (YAML null); a leading `#` would turn the
    // value into a comment. Newlines/NUL break the line structure.
    if value.is_empty()
        || value.starts_with('#')
        || value.len() > 4096
        || value.chars().any(|c| matches!(c, '\n' | '\r' | '\0'))
    {
        return Err(ApiError::invalid("invalid config value"));
    }
    Ok(())
}

/// Resolve a `clear` entry (known key name) to its scope.
fn known_key(name: &str) -> Result<(Scope, &'static str), ApiError> {
    if let Some(network_key) = name.strip_prefix("network.") {
        if let Some(&key) = NETWORK_KEYS.iter().find(|&&k| k == network_key) {
            return Ok((Scope::Network, key));
        }
    } else if let Some(&key) = SCALAR_KEYS.iter().find(|&&k| k == name) {
        return Ok((Scope::Top, key));
    }
    Err(ApiError::invalid(format!(
        "unknown booster config key {name:?}"
    )))
}

/// Apply the patch line-wise: only our key lines change; everything
/// else (comments, marker, unknown keys) stays byte-identical.
fn apply(raw: &str, patch: &BoosterConfigPatch) -> Result<String, ApiError> {
    fn push(
        updates: &mut Vec<(Scope, &'static str, String)>,
        scope: Scope,
        key: &'static str,
        value: Option<&str>,
    ) -> Result<(), ApiError> {
        if let Some(value) = value {
            validate_value(value)?;
            updates.push((scope, key, value.to_string()));
        }
        Ok(())
    }
    let mut updates: Vec<(Scope, &'static str, String)> = Vec::new();
    push(
        &mut updates,
        Scope::Top,
        "compression",
        patch.compression.as_deref(),
    )?;
    push(
        &mut updates,
        Scope::Top,
        "universal",
        patch.universal.as_deref(),
    )?;
    push(&mut updates, Scope::Top, "strip", patch.strip.as_deref())?;
    push(
        &mut updates,
        Scope::Top,
        "vconsole",
        patch.vconsole.as_deref(),
    )?;
    push(
        &mut updates,
        Scope::Top,
        "enable_lvm",
        patch.enable_lvm.as_deref(),
    )?;
    push(
        &mut updates,
        Scope::Top,
        "enable_mdraid",
        patch.enable_mdraid.as_deref(),
    )?;
    push(
        &mut updates,
        Scope::Top,
        "append_all_modaliases",
        patch.append_all_modaliases.as_deref(),
    )?;
    push(
        &mut updates,
        Scope::Top,
        "mount_timeout",
        patch.mount_timeout.as_deref(),
    )?;
    push(
        &mut updates,
        Scope::Top,
        "token_timeout",
        patch.token_timeout.as_deref(),
    )?;
    push(
        &mut updates,
        Scope::Top,
        "pin_delay",
        patch.pin_delay.as_deref(),
    )?;
    push(
        &mut updates,
        Scope::Top,
        "extra_files",
        patch.extra_files.as_deref(),
    )?;
    push(
        &mut updates,
        Scope::Top,
        "modules",
        patch.modules.as_deref(),
    )?;
    push(
        &mut updates,
        Scope::Top,
        "modules_force_load",
        patch.modules_force_load.as_deref(),
    )?;
    push(
        &mut updates,
        Scope::Network,
        "dhcp",
        patch.network_dhcp.as_deref(),
    )?;
    push(
        &mut updates,
        Scope::Network,
        "ip",
        patch.network_ip.as_deref(),
    )?;
    push(
        &mut updates,
        Scope::Network,
        "gateway",
        patch.network_gateway.as_deref(),
    )?;
    push(
        &mut updates,
        Scope::Network,
        "dns_servers",
        patch.network_dns_servers.as_deref(),
    )?;
    push(
        &mut updates,
        Scope::Network,
        "interfaces",
        patch.network_interfaces.as_deref(),
    )?;

    let mut clears = Vec::new();
    for name in &patch.clear {
        clears.push(known_key(name)?);
    }
    if updates.is_empty() && clears.is_empty() {
        return Ok(raw.to_string());
    }

    let mut lines: Vec<String> = raw.lines().map(str::to_string).collect();
    for (scope, key, value) in updates {
        match scope {
            Scope::Top => upsert_top(&mut lines, key, &value),
            Scope::Network => upsert_network(&mut lines, key, &value),
        }
    }
    for (scope, key) in clears {
        match scope {
            Scope::Top => remove_top(&mut lines, key),
            Scope::Network => remove_network(&mut lines, key),
        }
    }
    let mut out = lines.join("\n");
    if !out.ends_with('\n') {
        out.push('\n');
    }
    Ok(out)
}

fn is_comment(line: &str) -> bool {
    line.trim_start().starts_with('#')
}

fn is_top_level(line: &str) -> bool {
    !line.starts_with(' ') && !line.starts_with('\t')
}

/// `(key, rest-after-colon)` for a non-comment line, if it is a mapping
/// entry (a top-level `network:` opener has an empty `rest`).
fn line_entry(line: &str) -> Option<(&str, &str)> {
    if is_comment(line) {
        return None;
    }
    let (key, rest) = line.trim().split_once(':')?;
    let key = key.trim();
    if key.is_empty() || key.chars().any(char::is_whitespace) {
        return None;
    }
    Some((key, rest))
}

fn upsert_top(lines: &mut Vec<String>, key: &str, value: &str) {
    for line in lines.iter_mut() {
        if is_top_level(line) && line_entry(line).is_some_and(|(k, _)| k == key) {
            let comment = line_entry(line)
                .map(|(_, rest)| trailing_comment(rest))
                .unwrap_or_default();
            *line = format!("{key}: {value}{comment}");
            return;
        }
    }
    lines.push(format!("{key}: {value}"));
}

fn upsert_network(lines: &mut Vec<String>, key: &str, value: &str) {
    let Some(start) = lines.iter().position(|line| {
        is_top_level(line)
            && line_entry(line).is_some_and(|(k, rest)| {
                k == "network" && (rest.trim().is_empty() || rest.trim().starts_with('#'))
            })
    }) else {
        if !lines.is_empty() && lines.last().is_some_and(|l| !l.trim().is_empty()) {
            lines.push(String::new());
        }
        lines.push("network:".to_string());
        lines.push(format!("  {key}: {value}"));
        return;
    };
    let end = lines
        .iter()
        .enumerate()
        .skip(start + 1)
        .find(|(_, line)| !line.trim().is_empty() && is_top_level(line) && !is_comment(line))
        .map_or(lines.len(), |(index, _)| index);
    if let Some(section) = lines.get_mut(start + 1..end) {
        for line in section {
            if line_entry(line).is_some_and(|(k, _)| k == key) {
                let comment = line_entry(line)
                    .map(|(_, rest)| trailing_comment(rest))
                    .unwrap_or_default();
                *line = format!("  {key}: {value}{comment}");
                return;
            }
        }
    }
    lines.insert(end, format!("  {key}: {value}"));
}

fn remove_top(lines: &mut Vec<String>, key: &str) {
    lines.retain(|line| !(is_top_level(line) && line_entry(line).is_some_and(|(k, _)| k == key)));
}

fn remove_network(lines: &mut Vec<String>, key: &str) {
    let Some(start) = lines.iter().position(|line| {
        is_top_level(line)
            && line_entry(line).is_some_and(|(k, rest)| {
                k == "network" && (rest.trim().is_empty() || rest.trim().starts_with('#'))
            })
    }) else {
        return;
    };
    let end = lines
        .iter()
        .enumerate()
        .skip(start + 1)
        .find(|(_, line)| !line.trim().is_empty() && is_top_level(line) && !is_comment(line))
        .map_or(lines.len(), |(index, _)| index);
    let mut index = start + 1;
    while index < end.min(lines.len()) {
        let matched = lines
            .get(index)
            .is_some_and(|line| line_entry(line).is_some_and(|(k, _)| k == key));
        if matched {
            lines.remove(index);
            return;
        }
        index += 1;
    }
}

fn write_atomic(path: &Path, text: &str) -> Result<(), ApiError> {
    use std::os::unix::fs::PermissionsExt;
    // Pid-unique temp name: a stale file from another process never collides.
    let tmp = path.with_extension(format!("ursine-new.{}", std::process::id()));
    let cleanup = |error: ApiError| -> ApiError {
        let _ = std::fs::remove_file(&tmp);
        error
    };
    std::fs::write(&tmp, text)
        .map_err(|e| cleanup(ApiError::internal(format!("write {}: {e}", tmp.display()))))?;
    std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o644))
        .map_err(|e| cleanup(ApiError::internal(format!("chmod {}: {e}", tmp.display()))))?;
    std::fs::rename(&tmp, path).map_err(|e| {
        cleanup(ApiError::internal(format!(
            "replace {}: {e}",
            path.display()
        )))
    })
}

/// Serializes config writes inside this daemon (one rotation+replace at
/// a time — concurrent sets would race the tmp file and the slots).
static SET_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

// ---- regenerate ---------------------------------------------------------

/// The chain step that will run (FeralOS/Alpine hooks first).
enum Step {
    KernelHooks(&'static str),
    RegenerateImages,
    DirectBuild,
}

impl Step {
    fn label(&self) -> String {
        match self {
            Self::KernelHooks(_) => "apk fix kernel-hooks".to_string(),
            Self::RegenerateImages => "regenerate_images".to_string(),
            Self::DirectBuild => "booster build".to_string(),
        }
    }
}

/// Same selection the regenerate action executes: the preview shown in
/// the confirm dialog cannot drift from the run.
async fn select_step() -> Step {
    if let Some(apk) = find(&crate::modules::packages_apk::APK_CANDIDATES) {
        if hooks_present() && kernel_hooks_installed(apk).await {
            return Step::KernelHooks(apk);
        }
    }
    if Path::new(REGENERATE_IMAGES).is_file() {
        return Step::RegenerateImages;
    }
    Step::DirectBuild
}

async fn regenerate() -> Result<BoosterRegenerateReport, ApiError> {
    match select_step().await {
        // 1 — the distro's own kernel hooks (FeralOS 50-booster.hook runs
        // booster build + efi-mkuki UKI + fallbacks: the kernel-update path).
        Step::KernelHooks(apk) => {
            let out = exec::run_root_program(apk, &["fix", "kernel-hooks"], TIMEOUT).await?;
            return Ok(report("apk fix kernel-hooks", &out));
        }
        // 2a — distro helper shipped by some booster packages (Arch).
        Step::RegenerateImages => {
            let out = exec::run_root_program(REGENERATE_IMAGES, &[], TIMEOUT).await?;
            return Ok(report("regenerate_images", &out));
        }
        // 2b — direct booster build, one image per installed kernel.
        Step::DirectBuild => {}
    }
    let booster = find(&BOOSTER)
        .ok_or_else(|| ApiError::unavailable("booster is not installed", "apk add booster"))?;
    let kernels = installed_kernels()?;
    let mut combined = String::new();
    for kver in &kernels {
        let flavor = image_flavor(kver)?;
        let output = format!("/boot/booster-{flavor}");
        let out = exec::run_root_program(
            booster,
            &["build", "--force", "--kernel-version", kver, &output],
            TIMEOUT,
        )
        .await?;
        append_output(&mut combined, &out);
    }
    Ok(BoosterRegenerateReport {
        step: format!("booster build ({} kernel(s))", kernels.len()),
        output: combined.trim().to_string(),
    })
}

fn hooks_present() -> bool {
    std::fs::read_dir(HOOKS_DIR).is_ok_and(|entries| {
        entries
            .flatten()
            .any(|entry| entry.file_name().to_string_lossy().ends_with(".hook"))
    })
}

async fn kernel_hooks_installed(apk: &str) -> bool {
    exec::run_root_program(
        apk,
        &["info", "-e", "kernel-hooks"],
        Duration::from_secs(30),
    )
    .await
    .is_ok()
}

fn installed_kernels() -> Result<Vec<String>, ApiError> {
    let entries = std::fs::read_dir(MODULES_DIR).map_err(|e| {
        ApiError::unavailable(
            format!("read {MODULES_DIR}: {e}"),
            "install a kernel package",
        )
    })?;
    let mut kernels: Vec<String> = entries
        .flatten()
        .filter(|entry| entry.path().is_dir())
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();
    kernels.sort();
    if kernels.is_empty() {
        return Err(ApiError::unavailable(
            "no installed kernels found",
            "install a kernel package",
        ));
    }
    Ok(kernels)
}

/// Image name part after the last `-` (`6.6.7-0-lts` → `lts`). Refuses
/// pure-digit or missing suffixes: never guess a boot path.
fn image_flavor(kver: &str) -> Result<String, ApiError> {
    let flavor = kver.rsplit_once('-').map(|(_, tail)| tail);
    let plausible = |candidate: &str| {
        !candidate.is_empty()
            && !candidate.chars().all(|c| c.is_ascii_digit())
            && candidate
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
    };
    match flavor {
        Some(flavor) if plausible(flavor) => Ok(flavor.to_string()),
        _ => Err(ApiError::unavailable(
            format!("cannot derive the image name for kernel {kver}"),
            "install kernel-hooks (apk add kernel-hooks) so regeneration runs the distro hooks",
        )),
    }
}

fn report(step: &str, out: &CommandOutcome) -> BoosterRegenerateReport {
    let mut output = String::new();
    append_output(&mut output, out);
    BoosterRegenerateReport {
        step: step.to_string(),
        output: output.trim().to_string(),
    }
}

fn append_output(combined: &mut String, out: &CommandOutcome) {
    for stream in [out.stdout.as_str(), out.stderr.as_str()] {
        let stream = stream.trim();
        if stream.is_empty() {
            continue;
        }
        if !combined.is_empty() {
            combined.push('\n');
        }
        combined.push_str(stream);
    }
}

fn find(candidates: &[&'static str]) -> Option<&'static str> {
    candidates
        .iter()
        .find(|path| Path::new(path).is_file())
        .copied()
}

#[cfg(test)]
mod tests {
    use super::*;

    const FERALOS_YAML: &str = "compression: zstd\n\
        enable_lvm: false\n\
        enable_mdraid: false\n\
        network:\n\
        \x20 dhcp: off\n\
        \x20 # static below\n\
        modules_force_load: nvidia\n\
        mount_timeout: 3m\n\
        extra_files: busybox,mount,btrfs\n\
        append_all_modaliases: true\n\
        # Generated by FeralOS Installer\n";

    #[test]
    fn parses_known_keys_and_keeps_raw() {
        let mut view = BoosterConfig::default();
        parse_into(FERALOS_YAML, &mut view);
        assert_eq!(view.compression.as_deref(), Some("zstd"));
        assert_eq!(view.enable_lvm.as_deref(), Some("false"));
        assert_eq!(view.network_dhcp.as_deref(), Some("off"));
        assert_eq!(view.modules_force_load.as_deref(), Some("nvidia"));
        assert_eq!(view.extra_files.as_deref(), Some("busybox,mount,btrfs"));
        assert_eq!(view.universal, None);
        assert_eq!(view.raw, "");
    }

    #[test]
    fn upsert_preserves_comments_marker_and_unknown_keys() {
        let patch = BoosterConfigPatch {
            compression: Some("xz".into()),
            network_dhcp: Some("on".into()),
            ..BoosterConfigPatch::default()
        };
        let next = apply(FERALOS_YAML, &patch).unwrap();
        assert!(next.contains("compression: xz"));
        assert!(next.contains("network:\n  dhcp: on"));
        // Everything we do not own stays byte-identical.
        assert!(next.contains("# Generated by FeralOS Installer"));
        assert!(next.contains("  # static below"));
        assert!(next.contains("mount_timeout: 3m"));
        assert!(next.contains("extra_files: busybox,mount,btrfs"));
        // Round-trip parse still sees every key.
        let mut view = BoosterConfig::default();
        parse_into(&next, &mut view);
        assert_eq!(view.compression.as_deref(), Some("xz"));
        assert_eq!(view.network_dhcp.as_deref(), Some("on"));
    }

    #[test]
    fn upsert_appends_missing_keys_and_creates_network_section() {
        let patch = BoosterConfigPatch {
            vconsole: Some("true".into()),
            network_ip: Some("10.0.2.15/24".into()),
            ..BoosterConfigPatch::default()
        };
        let next = apply("", &patch).unwrap();
        assert!(next.contains("vconsole: true"));
        assert!(next.contains("network:\n  ip: 10.0.2.15/24"));
        let value: serde_yaml::Value = serde_yaml::from_str(&next).unwrap();
        assert!(value.is_mapping());
    }

    #[test]
    fn clear_removes_only_the_named_key_line() {
        let patch = BoosterConfigPatch {
            clear: vec!["modules_force_load".into()],
            ..BoosterConfigPatch::default()
        };
        let next = apply(FERALOS_YAML, &patch).unwrap();
        assert!(!next.contains("modules_force_load"));
        assert!(next.contains("mount_timeout: 3m"));
        assert!(next.contains("# Generated by FeralOS Installer"));
    }

    #[test]
    fn rejects_unknown_clear_keys_and_multiline_values() {
        let unknown = BoosterConfigPatch {
            clear: vec!["not_a_key".into()],
            ..BoosterConfigPatch::default()
        };
        assert!(apply(FERALOS_YAML, &unknown).is_err());
        let multiline = BoosterConfigPatch {
            compression: Some("zstd\nhack: true".into()),
            ..BoosterConfigPatch::default()
        };
        assert!(apply(FERALOS_YAML, &multiline).is_err());
    }

    #[test]
    fn empty_patch_is_a_noop() {
        let patch = BoosterConfigPatch::default();
        assert_eq!(apply(FERALOS_YAML, &patch).unwrap(), FERALOS_YAML);
    }

    #[test]
    fn network_section_survives_blank_lines_and_inline_comments() {
        let raw = "network:\n  dhcp: off  # keep me\n\n  ip: 10.0.0.1\n\ncompression: zstd\n";
        let patch = BoosterConfigPatch {
            network_dhcp: Some("on".into()),
            ..BoosterConfigPatch::default()
        };
        let next = apply(raw, &patch).unwrap();
        assert!(next.contains("dhcp: on  # keep me"), "{next}");
        // The blank line did not end the section: ip stays in network.
        let mut view = BoosterConfig::default();
        parse_into(&next, &mut view);
        assert_eq!(view.network_ip.as_deref(), Some("10.0.0.1"));
        assert_eq!(view.network_dhcp.as_deref(), Some("on"));
        // Re-applying an empty patch is a byte-exact no-op.
        let again = apply(&next, &BoosterConfigPatch::default()).unwrap();
        assert_eq!(again, next);
    }

    #[test]
    fn rejects_empty_and_comment_shaped_values() {
        let empty = BoosterConfigPatch {
            compression: Some(String::new()),
            ..BoosterConfigPatch::default()
        };
        assert!(apply(FERALOS_YAML, &empty).is_err());
        let hash = BoosterConfigPatch {
            compression: Some("# zstd".into()),
            ..BoosterConfigPatch::default()
        };
        assert!(apply(FERALOS_YAML, &hash).is_err());
    }

    #[test]
    fn image_flavor_never_guesses() {
        assert_eq!(image_flavor("6.6.7-0-lts").unwrap(), "lts");
        assert_eq!(image_flavor("6.6.7-0-virt").unwrap(), "virt");
        assert!(image_flavor("6.6.7").is_err());
        // a trailing build number is not a flavor (Arch ships
        // regenerate_images instead — never guess a boot path)
        assert!(image_flavor("6.12.9-arch1-1").is_err());
        assert!(image_flavor("").is_err());
    }
}
