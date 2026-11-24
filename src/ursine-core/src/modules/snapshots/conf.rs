//! `/etc/btrbk/btrbk.conf` read/create with a conservative managed template.

use std::io::Write;
use std::path::{Path, PathBuf};

use ursine_api::{ApiError, BtrbkConfigView, BtrbkSettings, ErrorCode};

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
            ErrorCode::Conflict,
            "btrbk configuration already exists",
        ));
    }
    let parent = path
        .parent()
        .ok_or_else(|| ApiError::invalid("btrbk config has no parent directory"))?;
    std::fs::create_dir_all(parent)
        .map_err(|e| ApiError::internal(format!("create btrbk config dir: {e}")))?;

    let layout = detect_layout().await;
    prepare_default_layout(&layout).await?;
    let template = default_config(&layout);
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
    copies: usize,
) -> Result<BtrbkConfigView, ApiError> {
    let current = if path.exists() {
        std::fs::read_to_string(path)
            .map_err(|e| ApiError::internal(format!("read btrbk config: {e}")))?
    } else {
        // The same default `init` would write: make it runnable first.
        let layout = detect_layout().await;
        prepare_default_layout(&layout).await?;
        default_config(&layout)
    };
    // Union of the values already in the file and the values this call names:
    // an option written by an earlier call must survive a call that does not
    // repeat it.
    let merged = overlay(parse_settings(&current), settings);
    if let Some((line, key)) = shadowed_managed_key(&current, &merged) {
        return Err(ApiError::new(
            ErrorCode::Conflict,
            format!(
                "btrbk.conf:{line} sets {key} below the first volume/subvolume/target line, which wins over the managed global value"
            ),
        )
        .with_fix(format!(
            "delete the {key} line at btrbk.conf:{line}; btrbk would keep using it and ignore the managed option"
        )));
    }
    let managed = render_managed(&merged)?;
    let updated = merge_managed(&current, &managed);

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
    // Uniform slot rotation (replaces the single fixed `.conf.bak`).
    crate::backup_configs::rotate(path, copies, None)?;
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

/// First line of the managed block; the removal pass drops it too, so
/// repeated `set` calls replace the block instead of stacking copies.
const MANAGED_MARKER: &str = "# --- Ursine managed options ---";
/// Last line of the managed block. It has to go with the block: dropping the
/// start marker alone leaves one more end marker (and its blank line) on
/// every call.
const MANAGED_END_MARKER: &str = "# --- end Ursine managed options ---";

/// A `volume`, `subvolume` or `target` line starts a btrbk section; the
/// options below it belong to that section.
fn is_section_header(trimmed: &str) -> bool {
    trimmed.starts_with("volume ")
        || trimmed.starts_with("subvolume ")
        || trimmed.starts_with("target ")
}

fn render_managed(settings: &BtrbkSettings) -> Result<String, ApiError> {
    let mut out = String::from(MANAGED_MARKER);
    out.push('\n');
    for (name, get) in MANAGED_KEYS {
        if let Some(value) = get(settings) {
            validate_value(name, value)?;
            out.push_str(&format!("{name} {value}\n"));
        }
    }
    out.push_str(MANAGED_END_MARKER);
    out.push('\n');
    Ok(out)
}

/// Values already in the file plus the values this call names: an option set
/// by an earlier call survives a call that does not repeat it, so a client can
/// change one option without reading and resending the whole set. A value the
/// call does not name and the file does not set stays absent. Inherited values
/// that would not pass [`validate_value`] (a hand-edited file) are left where
/// they are instead of failing an unrelated update.
fn overlay(existing: BtrbkSettings, overrides: &BtrbkSettings) -> BtrbkSettings {
    let mut merged = existing;
    for (index, (name, get)) in MANAGED_KEYS.iter().enumerate() {
        let Some(value) = get(overrides) else {
            continue;
        };
        if validate_value(name, value).is_ok() {
            set_key(&mut merged, index, Some(value.clone()));
        }
    }
    merged
}

/// Put the managed block on top and drop the previous block plus the managed
/// global keys, so repeated calls replace the block instead of stacking it.
/// A removed marker takes the blank line that followed it along, otherwise
/// every call would leave one more blank line behind.
fn merge_managed(current: &str, managed: &str) -> String {
    let mut body = String::new();
    let mut in_section = false;
    let mut after_marker = false;
    for line in current.lines() {
        let trimmed = line.trim_start();
        if trimmed == MANAGED_MARKER || trimmed == MANAGED_END_MARKER {
            after_marker = true;
            continue;
        }
        if after_marker {
            after_marker = false;
            if trimmed.trim().is_empty() {
                continue;
            }
        }
        if is_section_header(trimmed) {
            in_section = true;
        }
        if !in_section && managed_key(trimmed.split_whitespace().next().unwrap_or("")).is_some() {
            continue;
        }
        body.push_str(line);
        body.push('\n');
    }
    // Drop the blank lines the removed block left behind.
    let body = body.trim_matches('\n');
    if body.is_empty() {
        managed.to_string()
    } else {
        format!("{managed}\n{body}\n")
    }
}

/// First managed option that `content` also sets below the first section, with
/// its 1-based line number. btrbk reads such a line as that section's value,
/// which wins over the global managed block, so the global value would have no
/// effect at all (the shipped `FeralOS` file puts `subvolume /` above its
/// options, so every managed option can be shadowed).
fn shadowed_managed_key(content: &str, settings: &BtrbkSettings) -> Option<(usize, &'static str)> {
    let mut in_section = false;
    for (index, line) in content.lines().enumerate() {
        let trimmed = line.trim();
        if is_section_header(trimmed) {
            in_section = true;
            continue;
        }
        if !in_section || trimmed.starts_with('#') {
            continue;
        }
        let Some(position) = managed_key(trimmed.split_whitespace().next().unwrap_or("")) else {
            continue;
        };
        let Some((name, get)) = MANAGED_KEYS.get(position) else {
            continue;
        };
        if get(settings).is_some() {
            return Some((index + 1, name));
        }
    }
    None
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
        if is_section_header(trimmed) {
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
        set_key(&mut settings, index, (!value.is_empty()).then_some(value));
    }
    settings
}

/// Store one managed key. `value = None` clears it (the key was written
/// without a value), which is what [`parse_settings`] has to reproduce.
fn set_key(settings: &mut BtrbkSettings, index: usize, value: Option<String>) {
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

/// The snapshot directory a fresh config names: an existing btrbk directory
/// wins (`/btrbk/snapshots` on `FeralOS`), then an existing `/.snapshots`.
/// Otherwise `/.snapshots` is the choice and [`prepare_default_layout`]
/// creates it.
fn snapshot_dir() -> PathBuf {
    ["/btrbk/snapshots", "/.snapshots"]
        .iter()
        .map(PathBuf::from)
        .find(|path| path.is_dir())
        .unwrap_or_else(|| PathBuf::from("/.snapshots"))
}

/// The single subvolume name a fresh config falls back to when the machine
/// cannot be read: `/home` on a separate subvolume, else the root subvolume
/// as `FeralOS` and most distro layouts name it.
fn guessed_subvolume() -> &'static str {
    if Path::new("/home").exists() {
        "home"
    } else {
        "@"
    }
}

/// Top-level subvolume names in `btrfs subvolume list` output, in listing
/// order. Nested subvolumes (`@home/user`) are dropped: btrbk names one
/// subvolume per line and the parent already covers them. The subvolume the
/// snapshot directory lives in is dropped too — a config that snapshots its
/// own store nests every snapshot inside the next one.
fn layout_from_listing(raw: &str, snapshot_dir: &Path) -> Vec<String> {
    let store = snapshot_dir
        .strip_prefix("/")
        .ok()
        .and_then(|rest| rest.components().next())
        .map(|name| name.as_os_str().to_string_lossy().into_owned());
    let mut names: Vec<String> = Vec::new();
    for line in raw.lines() {
        let Some((_, path)) = line.split_once(" path ") else {
            continue;
        };
        let path = path.trim();
        if path.is_empty() || path.contains('/') || Some(path) == store.as_deref() {
            continue;
        }
        if !names.iter().any(|name| name == path) {
            names.push(path.to_string());
        }
    }
    names
}

/// Subvolumes the generated config snapshots: what btrfs reports for the
/// filesystem of `/`, or the historical single-name guess when the listing
/// cannot be read (no btrfs binary, not a btrfs root, refused as non-root).
async fn detect_layout() -> Vec<String> {
    let snapshot_dir = snapshot_dir();
    if let Ok(program) = btrbk::btrfs_path() {
        if let Ok(out) = crate::modules::packages_nix::exec::run_root_program_tolerant(
            program,
            &["subvolume", "list", "-a", "/"],
            btrbk::LIST_TIMEOUT,
            None,
        )
        .await
        {
            if out.code == 0 {
                let names = layout_from_listing(&out.stdout, &snapshot_dir);
                if !names.is_empty() {
                    return names;
                }
            }
        }
    }
    vec![guessed_subvolume().to_string()]
}

/// Make the generated default runnable before it is written: btrbk refuses
/// a missing `snapshot_dir`, and a `subvolume` that does not exist cannot be
/// snapshotted. Both failures are reported here instead of at the next
/// `create_snapshot`.
async fn prepare_default_layout(layout: &[String]) -> Result<(), ApiError> {
    let snapshot_dir = snapshot_dir();
    std::fs::create_dir_all(&snapshot_dir).map_err(|e| {
        ApiError::internal(format!(
            "create btrbk snapshot dir {}: {e}",
            snapshot_dir.display()
        ))
    })?;
    // btrfs is the source of truth; skip the probe when the tool is absent
    // (no snapshots are possible anyway and init already skips btrbk).
    let probe = btrbk::btrfs_path().ok();
    for name in layout {
        let path = format!("/{name}");
        if !Path::new(&path).exists() {
            return Err(
                ApiError::invalid(format!("btrbk subvolume {path} does not exist"))
                    .with_fix("mount the btrfs subvolume, or set the config by hand"),
            );
        }
        let Some(program) = probe else { continue };
        let out = crate::modules::packages_nix::exec::run_root_program_tolerant(
            program,
            &["subvolume", "show", &path],
            btrbk::LIST_TIMEOUT,
            None,
        )
        .await?;
        if out.code != 0 {
            let detail = format!("{}{}", out.stdout, out.stderr);
            return Err(ApiError::invalid(format!(
                "btrbk subvolume {path} is not a btrfs subvolume: {}",
                detail.trim()
            ))
            .with_fix("mount the btrfs top level and set the config by hand"));
        }
    }
    Ok(())
}

fn default_config(layout: &[String]) -> String {
    let snapshot_dir = snapshot_dir();
    let subvolumes = layout.iter().fold(String::new(), |mut out, name| {
        out.push_str("  subvolume ");
        out.push_str(name);
        out.push('\n');
        out
    });
    format!(
        r"# Generated by Ursine. Managed defaults; keep custom sections below.
timestamp_format long
snapshot_preserve_min 24h
snapshot_preserve 24h 14d

volume /
{subvolumes}  snapshot_dir {}
",
        snapshot_dir.display()
    )
}

#[cfg(unix)]
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};

#[cfg(test)]
mod tests {
    #![allow(clippy::disallowed_methods)] // tests use the host temp dir

    use super::*;

    fn temp_dir(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("ursine-btrbk-{label}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// Settings from `key = value` pairs, through the same writer the handler
    /// uses (an unknown key fails the test, not the assertion).
    fn settings(pairs: &[(&str, &str)]) -> BtrbkSettings {
        let mut settings = BtrbkSettings::default();
        for (key, value) in pairs {
            let index = managed_key(key).unwrap_or_else(|| panic!("unknown key {key}"));
            set_key(&mut settings, index, Some((*value).to_string()));
        }
        settings
    }

    #[tokio::test]
    async fn init_creates_a_readable_config_once() {
        let dir = temp_dir("init");
        let path = dir.join("btrbk.conf");

        let view = init(&path).await.unwrap();
        assert!(view.exists);
        assert!(view.content.contains("snapshot_preserve 24h 14d"));
        assert!(init(&path).await.is_err(), "second init must refuse");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn layout_from_listing_keeps_top_level_subvolumes_only() {
        // `btrfs subvolume list -a /` output; the nested `@home/user` is
        // covered by its parent and the snapshot store must stay out.
        let listing = concat!(
            "ID 256 gen 10 top level 5 path @\n",
            "ID 257 gen 10 top level 5 path @home\n",
            "ID 258 gen 10 top level 5 path btrbk\n",
            "ID 300 gen 12 top level 256 path @home/user\n",
        );
        assert_eq!(
            layout_from_listing(listing, Path::new("/btrbk/snapshots")),
            vec!["@", "@home"]
        );
        // A listing without subvolumes falls back to the guess, not to an
        // empty section btrbk would reject.
        assert!(layout_from_listing("", Path::new("/.snapshots")).is_empty());
    }

    #[test]
    fn default_config_names_every_subvolume_of_the_layout() {
        let config = default_config(&["@".to_string(), "@home".to_string()]);
        assert_eq!(config.matches("  subvolume ").count(), 2);
        assert!(config.contains("  subvolume @\n"));
        assert!(config.contains("  subvolume @home\n"));
        assert!(config.contains("  snapshot_dir "));
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
        assert!(render_managed(&settings(&[("snapshot_preserve", "24h; rm -rf /")])).is_err());
        assert!(
            render_managed(&settings(&[("snapshot_preserve", "24h 14d")]))
                .unwrap()
                .contains("snapshot_preserve 24h 14d")
        );
    }

    #[test]
    fn merge_managed_leaves_one_block_after_four_calls() {
        // Each call used to leave one more end marker and one more blank line.
        let managed = render_managed(&settings(&[("snapshot_preserve", "10d")])).unwrap();
        let mut config = "# keep\n".to_string();
        for _ in 0..4 {
            config = merge_managed(&config, &managed);
        }
        assert_eq!(config.matches(MANAGED_MARKER).count(), 1);
        assert_eq!(config.matches(MANAGED_END_MARKER).count(), 1);
        assert_eq!(config.matches("snapshot_preserve 10d").count(), 1);
        assert!(!config.contains("\n\n\n"), "no blank run: {config}");
        assert!(config.contains("# keep"));
    }

    #[test]
    fn merge_managed_cleans_the_stacked_block_of_the_previous_release() {
        // 0.48/0.49 left one end marker per call: the merge must clear the
        // stale block instead of keeping the blank runs.
        let stale = concat!(
            "# --- Ursine managed options ---\n",
            "snapshot_preserve 10d\n",
            "# --- end Ursine managed options ---\n",
            "\n",
            "# --- end Ursine managed options ---\n",
            "\n",
            "volume /\n",
            "  subvolume @\n",
        );
        let managed = render_managed(&settings(&[("snapshot_preserve", "14d")])).unwrap();
        let merged = merge_managed(stale, &managed);
        assert_eq!(merged.matches(MANAGED_END_MARKER).count(), 1);
        assert!(!merged.contains("\n\n\n"), "no blank run: {merged}");
        assert!(merged.contains("  subvolume @"));
    }

    #[test]
    fn overlay_keeps_the_options_a_previous_call_set() {
        // First call sets retention, second changes only the format: the
        // retention line must survive.
        let first = overlay(
            BtrbkSettings::default(),
            &settings(&[("snapshot_preserve", "14d 4w")]),
        );
        let second = overlay(first, &settings(&[("timestamp_format", "short")]));
        let managed = render_managed(&second).unwrap();
        assert!(managed.contains("snapshot_preserve 14d 4w"));
        assert!(managed.contains("timestamp_format short"));
    }

    #[test]
    fn shadowed_managed_key_names_the_line_below_the_first_section() {
        // The shipped FeralOS layout: `subvolume /` above the options, so
        // btrbk reads them as that subvolume's values.
        let feral = "volume /\n  subvolume /\n  snapshot_preserve 7d 4w 3m\n";
        let merged = overlay(
            parse_settings(feral),
            &settings(&[("snapshot_preserve", "14d")]),
        );
        assert_eq!(
            shadowed_managed_key(feral, &merged),
            Some((3, "snapshot_preserve"))
        );
        // A managed key the merged set does not carry is none of our business.
        let untouched = settings(&[("timestamp_format", "short")]);
        assert_eq!(shadowed_managed_key(feral, &untouched), None);
        // Same key above the first section: nothing shadows it.
        let global = "snapshot_preserve 7d\nvolume /\n  subvolume /\n";
        assert_eq!(shadowed_managed_key(global, &merged), None);
    }

    #[tokio::test]
    async fn set_refuses_a_shadowed_managed_key() {
        let dir = temp_dir("shadow");
        let path = dir.join("btrbk.conf");
        std::fs::write(
            &path,
            "volume /\n  subvolume /\n  snapshot_preserve 7d 4w 3m\n",
        )
        .unwrap();
        let error = set(&path, &settings(&[("snapshot_preserve", "14d")]), 3)
            .await
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::Conflict);
        assert!(
            error.message.contains("snapshot_preserve"),
            "{}",
            error.message
        );
        assert!(error.fix.is_some(), "the error must tell the fix");
        // Refused before the write: the file is untouched.
        assert!(std::fs::read_to_string(&path).unwrap().contains("7d 4w 3m"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn read_missing_is_not_an_error() {
        let view = read(Path::new("/definitely/missing/btrbk.conf")).unwrap();
        assert!(!view.exists);
    }
}
