//! `/etc/btrbk/btrbk.conf` read/create with a conservative managed template.

use std::io::Write;
use std::path::{Path, PathBuf};

use ursine_api::{ApiError, BtrbkConfigView, BtrbkSettings};

use super::btrbk;

const MAX_CONFIG_BYTES: u64 = 1024 * 1024;

pub(super) fn read(path: &Path) -> Result<BtrbkConfigView, ApiError> {
    let display = path.display().to_string();
    if !path.exists() {
        return Ok(BtrbkConfigView {
            path: display,
            exists: false,
            content: String::new(),
            settings: BtrbkSettings::default(),
        });
    }
    let meta = std::fs::symlink_metadata(path)
        .map_err(|e| ApiError::internal(format!("inspect btrbk config: {e}")))?;
    if meta.file_type().is_symlink() || !meta.is_file() {
        return Err(ApiError::invalid("btrbk config is not a regular file"));
    }
    if meta.len() > MAX_CONFIG_BYTES {
        return Err(ApiError::invalid("btrbk config is too large"));
    }
    let content = std::fs::read_to_string(path)
        .map_err(|e| ApiError::internal(format!("read btrbk config: {e}")))?;
    let settings = parse_settings(&content);
    Ok(BtrbkConfigView {
        path: display,
        exists: true,
        content,
        settings,
    })
}

pub(super) async fn init(path: &Path) -> Result<BtrbkConfigView, ApiError> {
    if path.exists() {
        return Err(ApiError::new(
            ursine_api::ErrorCode::Conflict,
            "btrbk configuration already exists",
        ));
    }
    let parent = path
        .parent()
        .ok_or_else(|| ApiError::invalid("btrbk config has no parent directory"))?;
    std::fs::create_dir_all(parent)
        .map_err(|e| ApiError::internal(format!("create btrbk config dir: {e}")))?;

    let template = default_config();
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
        .map_err(|e| ApiError::internal(format!("create btrbk config: {e}")))?;
    file.write_all(template.as_bytes())
        .map_err(|e| ApiError::internal(format!("write btrbk config: {e}")))?;
    file.sync_all()
        .map_err(|e| ApiError::internal(format!("sync btrbk config: {e}")))?;
    #[cfg(unix)]
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o640))
        .map_err(|e| ApiError::internal(format!("set btrbk config mode: {e}")))?;

    // btrbk validates the syntax without touching btrfs.
    let config = path
        .to_str()
        .ok_or_else(|| ApiError::internal("btrbk config path is not UTF-8"))?;
    if let Ok(program) = btrbk::btrbk_path() {
        if let Err(error) = crate::modules::packages_nix::exec::run_root_program(
            program,
            &["-c", config, "--format", "raw", "list", "config"],
            btrbk::LIST_TIMEOUT,
        )
        .await
        {
            let _ = std::fs::remove_file(path);
            return Err(ApiError::invalid(format!(
                "generated btrbk config is invalid: {}",
                error.message
            )));
        }
    }

    read(path)
}

pub(super) async fn set(
    path: &Path,
    settings: &BtrbkSettings,
) -> Result<BtrbkConfigView, ApiError> {
    let current = if path.exists() {
        std::fs::read_to_string(path)
            .map_err(|e| ApiError::internal(format!("read btrbk config: {e}")))?
    } else {
        default_config()
    };
    let managed = render_managed(settings)?;
    let mut body = String::new();
    // Drop managed global keys before the first section; keep everything else.
    let mut in_section = false;
    for line in current.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("volume ")
            || trimmed.starts_with("subvolume ")
            || trimmed.starts_with("target ")
        {
            in_section = true;
        }
        if !in_section && managed_key(trimmed.split_whitespace().next().unwrap_or("")).is_some() {
            continue;
        }
        body.push_str(line);
        body.push('\n');
    }
    let updated = format!("{managed}\n{body}");

    // Validate with btrbk before replacing the live file.
    let temp = path.with_file_name(format!(
        ".btrbk.conf.tmp.{}.{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos())
    ));
    {
        use std::io::Write as _;
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&temp)
            .map_err(|e| ApiError::internal(format!("create btrbk temp config: {e}")))?;
        file.write_all(updated.as_bytes())
            .map_err(|e| ApiError::internal(format!("write btrbk temp config: {e}")))?;
        file.sync_all()
            .map_err(|e| ApiError::internal(format!("sync btrbk temp config: {e}")))?;
    }
    let config = temp
        .to_str()
        .ok_or_else(|| ApiError::internal("btrbk config path is not UTF-8"))?;
    let program = btrbk::btrbk_path()?;
    let valid = crate::modules::packages_nix::exec::run_root_program(
        program,
        &["-c", config, "--format", "raw", "list", "config"],
        btrbk::LIST_TIMEOUT,
    )
    .await;
    if let Err(error) = valid {
        let _ = std::fs::remove_file(&temp);
        return Err(ApiError::invalid(format!(
            "btrbk rejected the configuration: {}",
            error.message
        )));
    }
    if path.exists() {
        std::fs::copy(path, path.with_extension("conf.bak"))
            .map_err(|e| ApiError::internal(format!("backup btrbk config: {e}")))?;
    }
    #[cfg(unix)]
    std::fs::set_permissions(&temp, std::fs::Permissions::from_mode(0o640))
        .map_err(|e| ApiError::internal(format!("set btrbk config mode: {e}")))?;
    std::fs::rename(&temp, path)
        .map_err(|e| ApiError::internal(format!("replace btrbk config: {e}")))?;
    read(path)
}

type SettingGetter = fn(&BtrbkSettings) -> Option<&String>;
const MANAGED_KEYS: [(&str, SettingGetter); 13] = [
    ("timestamp_format", |s| s.timestamp_format.as_ref()),
    ("snapshot_create", |s| s.snapshot_create.as_ref()),
    ("incremental", |s| s.incremental.as_ref()),
    ("snapshot_preserve_min", |s| {
        s.snapshot_preserve_min.as_ref()
    }),
    ("snapshot_preserve", |s| s.snapshot_preserve.as_ref()),
    ("target_preserve_min", |s| s.target_preserve_min.as_ref()),
    ("target_preserve", |s| s.target_preserve.as_ref()),
    ("archive_preserve_min", |s| s.archive_preserve_min.as_ref()),
    ("archive_preserve", |s| s.archive_preserve.as_ref()),
    ("preserve_hour_of_day", |s| s.preserve_hour_of_day.as_ref()),
    ("preserve_day_of_week", |s| s.preserve_day_of_week.as_ref()),
    ("stream_compress", |s| s.stream_compress.as_ref()),
    ("btrfs_commit_delete", |s| s.btrfs_commit_delete.as_ref()),
];

fn managed_key(key: &str) -> Option<usize> {
    MANAGED_KEYS.iter().position(|(name, _)| *name == key)
}

fn render_managed(settings: &BtrbkSettings) -> Result<String, ApiError> {
    let mut out = String::from("# --- Ursine managed options ---\n");
    for (name, get) in MANAGED_KEYS {
        if let Some(value) = get(settings) {
            validate_value(name, value)?;
            out.push_str(&format!("{name} {value}\n"));
        }
    }
    out.push_str("# --- end Ursine managed options ---\n");
    Ok(out)
}

fn validate_value(key: &str, value: &str) -> Result<(), ApiError> {
    if value.is_empty()
        || value.len() > 64
        || value.starts_with('-')
        || !value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, ' ' | '*' | '+' | '_' | '-'))
    {
        return Err(ApiError::invalid(format!("invalid value for {key}")));
    }
    Ok(())
}

fn parse_settings(content: &str) -> BtrbkSettings {
    let mut settings = BtrbkSettings::default();
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("volume ")
            || trimmed.starts_with("subvolume ")
            || trimmed.starts_with("target ")
        {
            break;
        }
        if trimmed.starts_with('#') || trimmed.is_empty() {
            continue;
        }
        let mut parts = trimmed.split_whitespace();
        let Some(key) = parts.next() else { continue };
        let value = parts.collect::<Vec<_>>().join(" ");
        let Some(index) = managed_key(key) else {
            continue;
        };
        let value = (!value.is_empty()).then_some(value);
        match index {
            0 => settings.timestamp_format = value,
            1 => settings.snapshot_create = value,
            2 => settings.incremental = value,
            3 => settings.snapshot_preserve_min = value,
            4 => settings.snapshot_preserve = value,
            5 => settings.target_preserve_min = value,
            6 => settings.target_preserve = value,
            7 => settings.archive_preserve_min = value,
            8 => settings.archive_preserve = value,
            9 => settings.preserve_hour_of_day = value,
            10 => settings.preserve_day_of_week = value,
            11 => settings.stream_compress = value,
            12 => settings.btrfs_commit_delete = value,
            _ => {}
        }
    }
    settings
}

fn default_config() -> String {
    let home = PathBuf::from("/home");
    let subvolume = if home.exists() { "home" } else { "@" };
    format!(
        r"# Generated by Ursine. Managed defaults; keep custom sections below.
timestamp_format long
snapshot_preserve_min 24h
snapshot_preserve 24h 14d

volume /
  subvolume {subvolume}
  snapshot_dir /.snapshots
"
    )
}

#[cfg(unix)]
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};

#[cfg(test)]
mod tests {
    #![allow(clippy::disallowed_methods)] // tests use the host temp dir

    use super::*;

    #[tokio::test]
    async fn init_creates_a_readable_config_once() {
        let dir = std::env::temp_dir().join(format!("ursine-btrbk-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("btrbk.conf");

        let view = init(&path).await.unwrap();
        assert!(view.exists);
        assert!(view.content.contains("snapshot_preserve 24h 14d"));
        assert!(init(&path).await.is_err(), "second init must refuse");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn parses_global_settings_and_stops_at_sections() {
        let settings = parse_settings(
            "timestamp_format long\nsnapshot_preserve 24h 14d\nvolume /\n  snapshot_preserve 1d\n",
        );
        assert_eq!(settings.timestamp_format.as_deref(), Some("long"));
        assert_eq!(settings.snapshot_preserve.as_deref(), Some("24h 14d"));
    }

    #[test]
    fn render_managed_rejects_unsafe_values() {
        let unsafe_settings = BtrbkSettings {
            snapshot_preserve: Some("24h; rm -rf /".into()),
            ..BtrbkSettings::default()
        };
        assert!(render_managed(&unsafe_settings).is_err());
        let valid_settings = BtrbkSettings {
            snapshot_preserve: Some("24h 14d".into()),
            ..BtrbkSettings::default()
        };
        assert!(render_managed(&valid_settings)
            .unwrap()
            .contains("snapshot_preserve 24h 14d"));
    }

    #[test]
    fn read_missing_is_not_an_error() {
        let view = read(Path::new("/definitely/missing/btrbk.conf")).unwrap();
        assert!(!view.exists);
    }
}
