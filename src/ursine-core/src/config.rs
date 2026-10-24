//! Daemon configuration: load from disk, and render the commented template
//! used by `config.init`.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

#[cfg(unix)]
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};

use toml_edit::{DocumentMut, Item, Table, Value};
use ursine_api::{CapabilityReport, ModuleId, ModulesConfig, TriState, UrsineConfig};

use crate::error::CoreError;

/// Load config from `path`. A missing file yields defaults (with the caller
/// deciding whether to warn) — the daemon must always start.
pub fn load(path: &Path) -> Result<UrsineConfig, CoreError> {
    if !path.exists() {
        return Ok(UrsineConfig::default());
    }
    validate_config_read(path)?;
    let raw = std::fs::read_to_string(path)
        .map_err(|e| CoreError::config(format!("read {}: {e}", path.display())))?;
    let cfg: UrsineConfig = toml::from_str(&raw)
        .map_err(|e| CoreError::config(format!("parse {}: {e}", path.display())))?;
    Ok(cfg)
}

/// Reject a config we must not trust before reading it. The daemon starts as
/// root and this file controls module gating, so a user-writable or symlinked
/// file would be a local privilege boundary violation.
fn validate_config_read(path: &Path) -> Result<(), CoreError> {
    const MAX_CONFIG_BYTES: u64 = 1024 * 1024;
    let meta = std::fs::symlink_metadata(path)
        .map_err(|e| CoreError::config(format!("inspect {}: {e}", path.display())))?;
    if meta.file_type().is_symlink() || !meta.is_file() {
        return Err(CoreError::config(format!(
            "configuration path is not a regular file: {}",
            path.display()
        )));
    }
    #[cfg(unix)]
    if meta.permissions().mode() & 0o022 != 0 {
        return Err(CoreError::config(format!(
            "configuration file is group/world-writable: {}",
            path.display()
        )));
    }
    #[cfg(unix)]
    if meta.uid() != 0 {
        return Err(CoreError::config(format!(
            "configuration file is not root-owned: {}",
            path.display()
        )));
    }
    if meta.len() > MAX_CONFIG_BYTES {
        return Err(CoreError::config(format!(
            "configuration file is larger than {} bytes: {}",
            MAX_CONFIG_BYTES,
            path.display()
        )));
    }
    Ok(())
}

/// Map a configurable daemon module to its `[modules]` key.
pub const fn configurable_module(module: ModuleId) -> Option<&'static str> {
    match module {
        ModuleId::PackagesNix => Some("packages_nix"),
        ModuleId::PackagesApk => Some("packages_apk"),
        ModuleId::PackagesFlatpak => Some("packages_flatpak"),
        ModuleId::Printers => Some("printers"),
        ModuleId::Scanners => Some("scanners"),
        ModuleId::Firewall => Some("firewall"),
        ModuleId::Snapshots => Some("snapshots"),
        ModuleId::Users => Some("users"),
        ModuleId::Niri => Some("niri"),
        ModuleId::System | ModuleId::Config | ModuleId::Capabilities => None,
    }
}

/// Convert a tri-state value to a TOML value without changing formatting.
fn tri_state_value(value: TriState) -> Value {
    match value {
        TriState::Auto => Value::from("auto"),
        TriState::On => Value::from(true),
        TriState::Off => Value::from(false),
    }
}

/// Replace exactly one `[modules]` switch, preserving comments, unknown keys
/// and formatting elsewhere in the TOML document. The replacement is atomic.
pub fn set_module_switch(
    path: &Path,
    module: ModuleId,
    value: TriState,
) -> Result<ModulesConfig, CoreError> {
    let Some(key) = configurable_module(module) else {
        return Err(CoreError::config(format!(
            "module {} has no configurable switch",
            module.as_str()
        )));
    };

    let raw = if path.exists() {
        std::fs::read_to_string(path)
            .map_err(|e| CoreError::config(format!("read {}: {e}", path.display())))?
    } else {
        // Seed only the modules table. Root-level copies would be stale and
        // would collide with the nested [modules] table written below.
        String::from("[modules]\n")
    };
    let mut document = raw
        .parse::<DocumentMut>()
        .map_err(|e| CoreError::config(format!("parse {}: {e}", path.display())))?;

    let modules_item = document
        .entry("modules")
        .or_insert_with(|| Item::Table(Table::new()));
    match modules_item {
        Item::Table(modules) => modules[key] = Item::Value(tri_state_value(value)),
        Item::Value(Value::InlineTable(modules)) => {
            modules.insert(key, tri_state_value(value));
        }
        _ => return Err(CoreError::config("[modules] is not a TOML table")),
    }

    // Validate before touching the real path: malformed configuration must
    // never make a running daemon restart-blind.
    let updated: UrsineConfig = toml::from_str(&document.to_string())
        .map_err(|e| CoreError::config(format!("validate {}: {e}", path.display())))?;

    write_atomic(path, document.to_string().as_bytes())?;
    Ok(updated.modules)
}

/// Monotonic suffix so concurrent process-local writers never share a temp.
static ATOMIC_WRITE_SEQ: AtomicU64 = AtomicU64::new(0);

/// Reject unsafe parents before creating or replacing configuration.
fn validate_config_parent(path: &Path) -> Result<(), CoreError> {
    let Some(dir) = path.parent() else {
        return Err(CoreError::config("configuration path has no parent"));
    };
    if dir == Path::new("/") {
        return Err(CoreError::config(
            "configuration path must not be at filesystem root",
        ));
    }
    std::fs::create_dir_all(dir)
        .map_err(|e| CoreError::config(format!("create {}: {e}", dir.display())))?;
    let meta = std::fs::symlink_metadata(dir)
        .map_err(|e| CoreError::config(format!("inspect {}: {e}", dir.display())))?;
    if meta.file_type().is_symlink() || !meta.is_dir() {
        return Err(CoreError::config(format!(
            "configuration parent is not a directory: {}",
            dir.display()
        )));
    }
    #[cfg(unix)]
    if meta.permissions().mode() & 0o002 != 0 {
        return Err(CoreError::config(format!(
            "configuration parent is world-writable: {}",
            dir.display()
        )));
    }
    #[cfg(unix)]
    if meta.uid() != 0 {
        return Err(CoreError::config(format!(
            "configuration parent is not root-owned: {}",
            dir.display()
        )));
    }
    Ok(())
}

/// Validate the destination without following a final symlink.
fn validate_config_target(path: &Path) -> Result<(), CoreError> {
    let Ok(meta) = std::fs::symlink_metadata(path) else {
        return Ok(());
    };
    if meta.file_type().is_symlink() || !meta.is_file() {
        return Err(CoreError::config(format!(
            "configuration path is not a regular file: {}",
            path.display()
        )));
    }
    #[cfg(unix)]
    if meta.permissions().mode() & 0o022 != 0 {
        return Err(CoreError::config(format!(
            "configuration file is group/world-writable: {}",
            path.display()
        )));
    }
    #[cfg(unix)]
    if meta.uid() != 0 {
        return Err(CoreError::config(format!(
            "configuration file is not root-owned: {}",
            path.display()
        )));
    }
    Ok(())
}

/// Unique `create_new` temp path: never follows a pre-planted symlink.
fn atomic_tmp_path(path: &Path) -> PathBuf {
    let seq = ATOMIC_WRITE_SEQ.fetch_add(1, Ordering::Relaxed);
    let name = path.file_name().map_or_else(
        || "config.toml".to_string(),
        |name| name.to_string_lossy().into_owned(),
    );
    path.with_file_name(format!(".{name}.tmp.{}-{seq}", std::process::id()))
}

/// Write bytes atomically and durably. Existing permissions are preserved;
/// new files are explicit `0640`, never umask-dependent.
fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), CoreError> {
    validate_config_parent(path)?;
    validate_config_target(path)?;
    let tmp = atomic_tmp_path(path);
    let mut created = false;
    let result: Result<(), CoreError> = (|| {
        #[cfg(unix)]
        let mut file = {
            let file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&tmp)?;
            created = true;
            file
        };
        #[cfg(not(unix))]
        let mut file = {
            let file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&tmp)?;
            created = true;
            file
        };
        file.write_all(bytes)?;
        file.sync_all()?;
        if path.exists() {
            let metadata = std::fs::metadata(path)?;
            std::fs::set_permissions(&tmp, metadata.permissions())?;
        } else {
            #[cfg(unix)]
            std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o640))?;
        }
        std::fs::rename(&tmp, path)?;
        #[cfg(unix)]
        if let Some(dir) = path.parent() {
            std::fs::File::open(dir)?.sync_all()?;
        }
        Ok(())
    })();
    if result.is_err() && created {
        let _ = std::fs::remove_file(&tmp);
    }
    result
}

/// Render the commented template for `config.init`, including detected
/// capability hints so the admin sees what `"auto"` resolves to today.
pub fn render_template(caps: &[CapabilityReport]) -> String {
    let mut out = String::new();
    out.push_str("# Ursine daemon configuration\n");
    out.push_str("# Generated by config.init. Tri-state per module: \"auto\" | true | false\n\n");
    out.push_str("token_ttl_min = 10\n");
    out.push_str("login_rate_limit_per_min = 5\n");
    out.push_str("# socket_path = \"/run/ursine.sock\"\n");
    out.push_str("# audit_path = \"/var/log/ursine/audit.jsonl\"\n\n");
    out.push_str("[modules]\n");
    out.push_str("# \"auto\" = detection decides (see hints below)\n");
    out.push_str("packages_nix = \"auto\"\n");
    out.push_str("snapshots = \"auto\"\n");
    out.push_str("users = \"auto\"\n");
    out.push_str("niri = \"auto\"\n\n");

    out.push_str("# --- Detection hints (informational; not read by the daemon) ---\n");
    for report in caps {
        if report.available {
            out.push_str(&format!(
                "# detected: {} = available\n",
                report.capability.as_str()
            ));
        } else {
            let reason = report.reason.as_deref().unwrap_or("unavailable");
            let fix = report.fix.as_deref().unwrap_or("");
            out.push_str(&format!(
                "# detected: {} = unavailable ({reason}; fix: {fix})\n",
                report.capability.as_str()
            ));
        }
    }
    out
}

/// Write the template atomically through the same hardened path as updates.
pub fn write_template(path: &Path, caps: &[CapabilityReport]) -> Result<(), CoreError> {
    write_atomic(path, render_template(caps).as_bytes())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::disallowed_methods)] // tests use the host temp dir
    use super::*;
    use ursine_api::{Capability, TriState};

    fn tmp_dir(name: &str) -> PathBuf {
        // nosemgrep: rust.lang.security.temp-dir.temp-dir — tests only; paths are container-local and non-security-sensitive
        let d = std::env::temp_dir().join(format!("ursine-test-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn no_atomic_leftovers(dir: &Path) -> bool {
        std::fs::read_dir(dir).unwrap().all(|entry| {
            !entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(".config.toml.tmp.")
        })
    }

    #[test]
    fn missing_file_yields_defaults() {
        let cfg = load(&tmp_dir("cfg-missing").join("nope.toml")).unwrap();
        assert_eq!(cfg.token_ttl_min, 10);
        assert!(cfg.modules.snapshots.is_auto());
    }

    #[test]
    fn load_rejects_symlinks_writable_files_and_oversized_input() {
        use std::os::unix::fs::{symlink, PermissionsExt};

        let dir = tmp_dir("cfg-load-guard");
        let target = dir.join("real.toml");
        std::fs::write(&target, "[modules]\npackages_nix = \"auto\"\n").unwrap();
        let link = dir.join("link.toml");
        symlink(&target, &link).unwrap();
        assert!(load(&link).is_err(), "symlinked config must be rejected");

        std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o666)).unwrap();
        assert!(load(&target).is_err(), "writable config must be rejected");

        std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o644)).unwrap();
        std::fs::write(&target, "x".repeat(1024 * 1024 + 1)).unwrap();
        assert!(load(&target).is_err(), "oversized config must be rejected");
    }

    #[test]
    fn refuses_group_or_world_writable_config() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tmp_dir("cfg-writable");
        let path = dir.join("config.toml");
        std::fs::write(&path, "[modules]\npackages_nix = \"auto\"\n").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o666)).unwrap();
        let error = set_module_switch(&path, ModuleId::PackagesNix, TriState::Off).unwrap_err();
        assert!(
            error.to_string().contains("group/world-writable"),
            "{error}"
        );
    }

    #[test]
    fn template_roundtrips_and_hints_are_comments() {
        let caps = vec![
            CapabilityReport::unavailable(
                Capability::Btrbk,
                "btrbk not installed",
                "apk add btrbk",
            ),
            CapabilityReport::available(Capability::Nix),
        ];
        let text = render_template(&caps);
        // The template must parse: hints are comments only.
        let cfg: UrsineConfig = toml::from_str(&text).unwrap();
        assert_eq!(cfg.modules.packages_nix, TriState::Auto);
        assert!(text.contains("# detected: btrbk = unavailable"));
        assert!(text.contains("apk add btrbk"));
    }

    #[test]
    fn set_module_switch_updates_one_key_and_preserves_comments() {
        let dir = tmp_dir("cfg-switch");
        let path = dir.join("config.toml");
        std::fs::write(
            &path,
            "# admin notes\ntoken_ttl_min = 20\n\n[modules]\n# keep auto until tested\npackages_nix = \"auto\"\n",
        )
        .unwrap();

        let modules = set_module_switch(&path, ModuleId::PackagesNix, TriState::Off).unwrap();
        assert_eq!(modules.packages_nix, TriState::Off);
        assert_eq!(modules.snapshots, TriState::Auto);
        let raw = std::fs::read_to_string(&path).unwrap();
        assert!(raw.contains("# admin notes"));
        assert!(raw.contains("token_ttl_min = 20"));
        assert!(raw.contains("# keep auto until tested"));
        assert!(raw.contains("packages_nix = false"));
        assert!(no_atomic_leftovers(&dir));
    }

    #[test]
    fn set_module_switch_creates_modules_table() {
        let dir = tmp_dir("cfg-switch-create");
        let path = dir.join("config.toml");
        std::fs::write(&path, "token_ttl_min = 15\n").unwrap();
        let modules = set_module_switch(&path, ModuleId::Niri, TriState::On).unwrap();
        assert_eq!(modules.niri, TriState::On);
        let cfg = load(&path).unwrap();
        assert_eq!(cfg.token_ttl_min, 15);
        assert_eq!(cfg.modules.niri, TriState::On);
    }

    #[test]
    fn missing_file_seeds_only_modules_table() {
        let dir = tmp_dir("cfg-switch-missing");
        let path = dir.join("config.toml");
        let modules = set_module_switch(&path, ModuleId::Users, TriState::Off).unwrap();
        assert_eq!(modules.users, TriState::Off);
        let raw = std::fs::read_to_string(&path).unwrap();
        let document = raw.parse::<DocumentMut>().unwrap();
        assert!(!document.as_table().contains_key("packages_nix"));
        assert_eq!(document["modules"]["users"].as_bool(), Some(false));
        assert!(no_atomic_leftovers(&dir));
    }

    #[test]
    fn set_module_switch_accepts_inline_modules_table() {
        let dir = tmp_dir("cfg-switch-inline");
        let path = dir.join("config.toml");
        std::fs::write(
            &path,
            "modules = { packages_nix = \"auto\", snapshots = false }\n",
        )
        .unwrap();
        let modules = set_module_switch(&path, ModuleId::PackagesNix, TriState::On).unwrap();
        assert_eq!(modules.packages_nix, TriState::On);
        assert_eq!(modules.snapshots, TriState::Off);
        let raw = std::fs::read_to_string(&path).unwrap();
        assert!(raw.contains("packages_nix = true"));
        assert!(no_atomic_leftovers(&dir));
    }

    #[test]
    fn non_configurable_module_is_rejected() {
        let dir = tmp_dir("cfg-switch-invalid");
        let path = dir.join("config.toml");
        std::fs::write(&path, "[modules]\nusers = \"auto\"\n").unwrap();
        let err = set_module_switch(&path, ModuleId::Capabilities, TriState::On).unwrap_err();
        assert!(err.to_string().contains("no configurable switch"));
    }

    #[test]
    fn write_template_is_atomic_and_readable() {
        let dir = tmp_dir("cfg-write");
        let path = dir.join("config.toml");
        write_template(&path, &[]).unwrap();
        let cfg = load(&path).unwrap();
        assert_eq!(cfg.login_rate_limit_per_min, 5);
        assert!(no_atomic_leftovers(&dir));
    }

    #[cfg(unix)]
    #[test]
    fn new_config_has_explicit_private_mode() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tmp_dir("cfg-mode");
        let path = dir.join("config.toml");
        write_template(&path, &[]).unwrap();
        let mode = std::fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o640);
    }
}
