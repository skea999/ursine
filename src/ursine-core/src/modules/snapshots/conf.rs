//! `/etc/btrbk/btrbk.conf` read/create with a conservative managed template.

use std::io::Write;
use std::path::{Path, PathBuf};

use ursine_api::{ApiError, BtrbkConfigView, BtrbkSettings, ErrorCode};

use super::btrbk;

const MAX_CONFIG_BYTES: u64 = 1024 * 1024;

/// Start marker of the managed block; the block is replaced in place.
const MANAGED_START: &str = "# --- Ursine managed options ---";
/// End marker of the managed block.
const MANAGED_END: &str = "# --- end Ursine managed options ---";
/// Source the generated config snapshots when the machine has `/home`.
const HOME: &str = "/home";
/// Directories btrbk may already use on this machine, most specific first:
/// the `FeralOS` layout keeps snapshots in `/btrbk/snapshots`, upstream btrbk
/// defaults to `/.snapshots`.
const SNAPSHOT_DIR_CANDIDATES: [&str; 2] = ["/btrbk/snapshots", "/.snapshots"];
/// Snapshot directory written (and created) when no candidate exists.
const DEFAULT_SNAPSHOT_DIR: &str = "/.snapshots";

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
    let layout = SnapLayout::detect();
    init_with(path, &layout).await
}

/// Init against a given layout, so tests never create a directory on the host.
async fn init_with(path: &Path, layout: &SnapLayout) -> Result<BtrbkConfigView, ApiError> {
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
    // btrbk stops on a missing snapshot directory, so a config that names one
    // must bring it into existence (or fail here, not on the next snapshot).
    ensure_snapshot_dir(&layout.snapshot_dir)?;

    let template = layout.render();
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

    // btrbk itself is the only proof that a layout can run: `list snapshots`
    // parses the config and resolves the source subvolume and the snapshot
    // directory, so a config btrbk cannot read is refused instead of reported
    // as written. Skipped when btrbk is absent (the capability gate owns that
    // case) — the layout is then the best the machine allowed.
    let config = path
        .to_str()
        .ok_or_else(|| ApiError::internal("btrbk config path is not UTF-8"))?;
    if let Ok(program) = btrbk::btrbk_path() {
        if let Err(error) = crate::modules::packages_nix::exec::run_root_program(
            program,
            &["-c", config, "--format", "raw", "list", "snapshots"],
            btrbk::LIST_TIMEOUT,
        )
        .await
        {
            let _ = std::fs::remove_file(path);
            return Err(ApiError::invalid(format!(
                "btrbk cannot use the generated configuration: {}",
                first_line(&error.message)
            ))
            .with_fix(format!(
                "check that {} and the subvolume {} exist, then run init again",
                layout.snapshot_dir, layout.subvolume
            )));
        }
    }

    read(path)
}

/// Create the snapshot directory the generated config names.
fn ensure_snapshot_dir(dir: &str) -> Result<(), ApiError> {
    let path = Path::new(dir);
    if path.is_dir() {
        return Ok(());
    }
    std::fs::create_dir_all(path).map_err(|e| {
        ApiError::invalid(format!("cannot create the snapshot directory {dir}: {e}")).with_fix(
            format!("create it as root with `mkdir -p {dir}` and run init again"),
        )
    })
}

/// Keep btrbk's multi-line complaint out of a single-line error message.
fn first_line(message: &str) -> String {
    let line = message
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or("btrbk rejected the configuration");
    line.chars().take(240).collect()
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
        SnapLayout::detect().render()
    };
    let managed = render_managed(settings)?;
    let updated = apply_managed_block(&current, &managed);

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

fn render_managed(settings: &BtrbkSettings) -> Result<String, ApiError> {
    let mut out = String::from(MANAGED_START);
    out.push('\n');
    for (name, get) in MANAGED_KEYS {
        if let Some(value) = get(settings) {
            validate_value(name, value)?;
            out.push_str(&format!("{name} {value}\n"));
        }
    }
    out.push_str(MANAGED_END);
    out.push('\n');
    Ok(out)
}

/// Put `block` into `content` as the single managed block: an existing block is
/// replaced where it stands, every later copy is deleted together with the
/// blank run it separated, and a file without a block gets one on top. User
/// content outside the markers is copied through unchanged, so a second call
/// with the same settings returns the same bytes.
fn apply_managed_block(content: &str, block: &str) -> String {
    let lines: Vec<&str> = content.lines().collect();
    let blocks = managed_blocks(&lines);
    let mut out: Vec<&str> = Vec::new();
    let mut inserted = false;
    let mut index = 0;
    while let Some(line) = lines.get(index) {
        let Some((_, end)) = blocks.iter().find(|(start, _)| *start == index) else {
            out.push(line);
            index += 1;
            continue;
        };
        if inserted {
            // A stray duplicate: drop it and the blank lines it separated.
            index = *end;
            while lines.get(index).is_some_and(|line| line.trim().is_empty()) {
                index += 1;
            }
        } else {
            out.extend(block.lines());
            inserted = true;
            index = *end;
        }
    }
    let body = join_lines(&out);
    if body.is_empty() {
        return block.to_string();
    }
    if inserted {
        body
    } else {
        format!("{block}\n{body}")
    }
}

/// Line ranges `start … end` (end exclusive) of the managed block markers. An
/// unterminated start marker owns the rest of the file: those are still global
/// managed options, which can only live above the first section.
fn managed_blocks(lines: &[&str]) -> Vec<(usize, usize)> {
    let mut blocks = Vec::new();
    let mut index = 0;
    while let Some(line) = lines.get(index) {
        if line.trim() != MANAGED_START {
            index += 1;
            continue;
        }
        let end = lines
            .iter()
            .skip(index)
            .position(|line| line.trim() == MANAGED_END)
            .map_or(lines.len(), |offset| index + offset + 1);
        blocks.push((index, end));
        index = end;
    }
    blocks
}

fn join_lines(lines: &[&str]) -> String {
    let mut text = lines.join("\n");
    if !text.is_empty() {
        text.push('\n');
    }
    text
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

/// Layout the generated configuration describes.
struct SnapLayout {
    /// Source subvolume, relative to the volume `/`.
    subvolume: String,
    /// Directory btrbk writes snapshots into; created when missing.
    snapshot_dir: String,
}

impl SnapLayout {
    /// Take the layout from the machine: the sources it actually has and a
    /// snapshot directory btrbk already uses, else the conservative default.
    fn detect() -> Self {
        let mountinfo = std::fs::read_to_string("/proc/self/mountinfo").unwrap_or_default();
        Self::choose(&mounted_subvolumes(&mountinfo), |path| {
            Path::new(path).is_dir()
        })
    }

    /// Pure layout choice over an injected subvolume list and directory probe.
    fn choose(mounted: &[PathBuf], exists: impl Fn(&str) -> bool) -> Self {
        // `/home` is the source the machine cares about: its own subvolume on a
        // split layout, a plain directory otherwise. Mounted subvolumes supply
        // the fallback when the machine has no `/home` at all.
        let subvolume = if mounted.iter().any(|path| path == Path::new(HOME)) || exists(HOME) {
            "home".to_string()
        } else if let Some(first) = mounted.iter().find_map(|path| {
            path.to_str()
                .and_then(|path| path.strip_prefix('/'))
                .filter(|name| !name.is_empty())
                .map(str::to_string)
        }) {
            first
        } else {
            // No detected subvolume: the root subvolume itself is the conservative
            // assumption older releases shipped.
            "@".to_string()
        };
        let snapshot_dir = SNAPSHOT_DIR_CANDIDATES
            .iter()
            .find(|dir| exists(dir))
            .map_or_else(
                || DEFAULT_SNAPSHOT_DIR.to_string(),
                |dir| (*dir).to_string(),
            );
        Self {
            subvolume,
            snapshot_dir,
        }
    }

    /// The generated text; stable for the common `/home` + `/.snapshots` case.
    fn render(&self) -> String {
        format!(
            r"# Generated by Ursine. Managed defaults; keep custom sections below.
timestamp_format long
snapshot_preserve_min 24h
snapshot_preserve 24h 14d

volume /
  subvolume {}
  snapshot_dir {}
",
            self.subvolume, self.snapshot_dir
        )
    }
}

/// Mount points where the machine mounted its own btrfs subvolume: the
/// mountinfo root field is not `/` for those. Only btrfs entries count;
/// `/boot/efi` and friends are other filesystems.
fn mounted_subvolumes(mountinfo: &str) -> Vec<PathBuf> {
    mountinfo
        .lines()
        .filter_map(|line| {
            let (fields, tail) = line.split_once(" - ")?;
            let mut parts = fields.split_whitespace();
            let _mount_id = parts.next()?;
            let _parent_id = parts.next()?;
            let _device = parts.next()?;
            let root = parts.next()?;
            let point = parts.next()?;
            let filesystem = tail.split_whitespace().next()?;
            (filesystem == "btrfs" && root != "/").then(|| PathBuf::from(point))
        })
        .collect()
}

#[cfg(unix)]
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};

#[cfg(test)]
mod tests {
    #![allow(clippy::disallowed_methods)] // tests use the host temp dir

    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("ursine-btrbk-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn layout(dir: &Path) -> SnapLayout {
        SnapLayout {
            subvolume: "home".to_string(),
            snapshot_dir: dir.join("snapshots").display().to_string(),
        }
    }

    #[tokio::test]
    async fn init_creates_a_readable_config_once() {
        let dir = temp_dir("init");
        let path = dir.join("btrbk.conf");

        let view = init_with(&path, &layout(&dir)).await.unwrap();
        assert!(view.exists);
        assert!(view.content.contains("snapshot_preserve 24h 14d"));
        assert!(
            init_with(&path, &layout(&dir)).await.is_err(),
            "second init must refuse"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn init_creates_the_snapshot_directory_it_names() {
        let dir = temp_dir("init-dir");
        let path = dir.join("btrbk.conf");
        let snapshot_dir = dir.join("btrbk/snapshots");
        assert!(!snapshot_dir.exists());

        let view = init_with(
            &path,
            &SnapLayout {
                subvolume: "home".to_string(),
                snapshot_dir: snapshot_dir.display().to_string(),
            },
        )
        .await
        .unwrap();
        assert!(snapshot_dir.is_dir(), "btrbk gets the directory it names");
        assert!(view
            .content
            .contains(&format!("snapshot_dir {}", snapshot_dir.display())));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn init_refuses_when_the_snapshot_directory_cannot_be_created() {
        let dir = temp_dir("init-blocked");
        // A regular file in the way makes `create_dir_all` fail.
        std::fs::write(dir.join("blocker"), "x").unwrap();
        let path = dir.join("btrbk.conf");
        let error = init_with(
            &path,
            &SnapLayout {
                subvolume: "home".to_string(),
                snapshot_dir: dir.join("blocker/snapshots").display().to_string(),
            },
        )
        .await
        .unwrap_err();
        assert_eq!(error.code, ErrorCode::Invalid);
        assert!(error.fix.is_some(), "the refusal names the mkdir to run");
        assert!(
            !path.exists(),
            "no config for a directory that does not exist"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn layout_prefers_home_and_reuses_an_existing_snapshot_directory() {
        let mounted = vec![PathBuf::from("/home"), PathBuf::from("/nix")];
        let chosen = SnapLayout::choose(&mounted, |path| path == "/btrbk/snapshots");
        assert_eq!(chosen.subvolume, "home");
        assert_eq!(
            chosen.snapshot_dir, "/btrbk/snapshots",
            "the directory btrbk already uses wins over the default"
        );

        // No existing directory: the conservative default, which init creates.
        let chosen = SnapLayout::choose(&mounted, |_| false);
        assert_eq!(chosen.snapshot_dir, DEFAULT_SNAPSHOT_DIR);
        assert!(chosen.render().contains("snapshot_dir /.snapshots"));

        // Plain `/home` directory, no mounted subvolume list at all.
        let chosen = SnapLayout::choose(&[], |path| path == HOME || path == "/.snapshots");
        assert_eq!(chosen.subvolume, "home");
        assert_eq!(chosen.snapshot_dir, "/.snapshots");
    }

    #[test]
    fn layout_falls_back_to_a_mounted_subvolume_then_the_root_subvolume() {
        let chosen = SnapLayout::choose(&[PathBuf::from("/var")], |_| false);
        assert_eq!(chosen.subvolume, "var");
        let chosen = SnapLayout::choose(&[], |_| false);
        assert_eq!(chosen.subvolume, "@");
    }

    #[test]
    fn mounted_subvolumes_read_the_mountinfo_root_field() {
        let mountinfo = "\
25 0 8:1 / / rw - btrfs /dev/vda2 rw\n\
26 25 8:1 /home /home rw - btrfs /dev/vda2 rw\n\
27 25 8:16 / /boot/efi rw - vfat /dev/vda1 rw\n\
28 25 8:1 /nix /nix rw - btrfs /dev/vda2 rw\n";
        let mounted = mounted_subvolumes(mountinfo);
        assert_eq!(mounted, vec![PathBuf::from("/home"), PathBuf::from("/nix")]);
    }

    #[test]
    fn managed_block_is_added_on_top_when_absent() {
        let block = render_managed(&settings("7d")).unwrap();
        let updated = apply_managed_block("volume /\n  subvolume home\n", &block);
        assert_eq!(
            updated,
            "# --- Ursine managed options ---\nsnapshot_preserve 7d\n# --- end Ursine managed options ---\n\nvolume /\n  subvolume home\n"
        );
    }

    #[test]
    fn managed_block_is_replaced_in_place_and_keeps_user_content() {
        let content = "# my header\ntimestamp_format long\n# --- Ursine managed options ---\nsnapshot_preserve 1d\n# --- end Ursine managed options ---\n\n# my note\nvolume /\n  subvolume home\n  snapshot_dir /.snapshots\n";
        let block = render_managed(&settings("7d 4w")).unwrap();
        let updated = apply_managed_block(content, &block);
        assert_eq!(
            updated,
            "# my header\ntimestamp_format long\n# --- Ursine managed options ---\nsnapshot_preserve 7d 4w\n# --- end Ursine managed options ---\n\n# my note\nvolume /\n  subvolume home\n  snapshot_dir /.snapshots\n"
        );
    }

    #[test]
    fn duplicate_managed_blocks_and_their_blank_runs_are_removed() {
        // The shape the old writer left behind: an empty block per call.
        let content = "# --- Ursine managed options ---\nsnapshot_preserve 7d\n# --- end Ursine managed options ---\n\n# --- Ursine managed options ---\n# --- end Ursine managed options ---\n\n# --- Ursine managed options ---\n# --- end Ursine managed options ---\n\nvolume /\n  subvolume home\n";
        let block = render_managed(&settings("7d")).unwrap();
        let updated = apply_managed_block(content, &block);
        assert_eq!(
            updated,
            "# --- Ursine managed options ---\nsnapshot_preserve 7d\n# --- end Ursine managed options ---\n\nvolume /\n  subvolume home\n"
        );
        assert_eq!(updated.matches(MANAGED_START).count(), 1, "one block only");
        assert!(!updated.contains("\n\n\n"), "no blank run left behind");
    }

    #[test]
    fn applying_the_same_block_twice_is_byte_identical() {
        let block = render_managed(&settings("7d 4w")).unwrap();
        let content = "timestamp_format long\nsnapshot_preserve 1d\n\nvolume /\n  subvolume home\n";
        let first = apply_managed_block(content, &block);
        let second = apply_managed_block(&first, &block);
        assert_eq!(first, second);
        assert_eq!(second.matches(MANAGED_START).count(), 1);
    }

    #[test]
    fn unterminated_managed_block_is_replaced_to_the_end_of_the_file() {
        let content = "# --- Ursine managed options ---\nsnapshot_preserve 1d\n";
        let block = render_managed(&settings("7d")).unwrap();
        assert_eq!(apply_managed_block(content, &block), block);
    }

    fn settings(preserve: &str) -> BtrbkSettings {
        BtrbkSettings {
            snapshot_preserve: Some(preserve.to_string()),
            ..BtrbkSettings::default()
        }
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
