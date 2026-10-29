//! Uniform desktop theming: read the winning shell config (Wayle or
//! `DankMaterialShell`) and propagate one normalized theme to every toolkit
//! and application (`docs/specs/SPEC-theming.md`).
//!
//! Read surfaces are the shell's own files — never written. All writes are
//! user-scoped, atomic and reported per target.

use std::path::{Path, PathBuf};

use ursine_api::{
    Action, ApiError, TargetStatus, ThemeSnapshot, ThemeSource, ThemingStatus, ThemingTarget,
};

mod apply;
mod apps;
mod color;
mod cosmic;
mod dms;
mod wayle;
mod write;

use write::read_or_empty;

const WAYLE_CONFIG: &str = ".config/wayle/config.toml";
const DMS_SETTINGS: &str = ".config/DankMaterialShell/settings.json";
const DMS_CSS: &str = ".config/gtk-3.0/dank-colors.css";

/// Route a theming action (blocking work runs off the async executor).
pub async fn handle(
    action: &Action,
    ctx: &crate::modules::ModuleCtx<'_>,
) -> Result<serde_json::Value, ApiError> {
    match action {
        Action::ThemeDetect => {
            let (home, uid, gid) = target_identity(ctx.as_user)?;
            let status = blocking(move || detect_blocking(&home, uid, gid)).await?;
            serde_json::to_value(status)
                .map_err(|e| ApiError::internal(format!("encode theming status: {e}")))
        }
        Action::ThemeGet => {
            let (home, uid, gid) = target_identity(ctx.as_user)?;
            let snapshot = blocking(move || snapshot_blocking(&home, uid, gid)).await?;
            serde_json::to_value(snapshot)
                .map_err(|e| ApiError::internal(format!("encode theme snapshot: {e}")))
        }
        Action::ThemeApply { show_minimize } => {
            let (home, uid, gid) = target_identity(ctx.as_user)?;
            let user = ctx.as_user.to_string();
            let show_minimize = *show_minimize;
            let report =
                blocking(move || apply::run_blocking(&user, &home, uid, gid, show_minimize))
                    .await?;
            serde_json::to_value(report)
                .map_err(|e| ApiError::internal(format!("encode apply report: {e}")))
        }
        _ => Err(ApiError::invalid(format!(
            "action {} is not valid for module theming",
            crate::modules::action_name(action)
        ))),
    }
}

/// Run a blocking closure on the blocking pool (config parsing + file IO
/// must never stall the async executor).
async fn blocking<F, T>(work: F) -> Result<T, ApiError>
where
    F: FnOnce() -> Result<T, ApiError> + Send + 'static,
    T: Send + 'static,
{
    tokio::task::spawn_blocking(work)
        .await
        .map_err(|e| ApiError::internal(format!("theming task failed: {e}")))?
}

/// Resolve the target user to (home, uid, gid); rejects root/system users.
fn target_identity(user: &str) -> Result<(PathBuf, u32, u32), ApiError> {
    let passwd = read_or_empty(Path::new("/etc/passwd"));
    identity_from_passwd(&passwd, user)
}

/// Parse `/etc/passwd` for `user`: normal login users only (uid 1000..60000).
fn identity_from_passwd(passwd: &str, user: &str) -> Result<(PathBuf, u32, u32), ApiError> {
    for line in passwd.lines() {
        let fields: Vec<&str> = line.split(':').collect();
        if fields.len() < 7 || fields.first() != Some(&user) {
            continue;
        }
        let uid: u32 = fields
            .get(2)
            .ok_or_else(|| ApiError::internal(format!("bad passwd line for {user}")))?
            .parse()
            .map_err(|_| ApiError::internal(format!("bad uid for {user}")))?;
        let gid: u32 = fields
            .get(3)
            .ok_or_else(|| ApiError::internal(format!("bad passwd line for {user}")))?
            .parse()
            .map_err(|_| ApiError::internal(format!("bad gid for {user}")))?;
        if uid == 0 || !(1000..60000).contains(&uid) {
            return Err(ApiError::invalid(format!(
                "{user} is not a normal login user"
            )));
        }
        if gid == 0 || gid >= 65534 {
            return Err(ApiError::invalid(format!("{user} has an implausible gid")));
        }
        let home = PathBuf::from(
            fields
                .get(5)
                .ok_or_else(|| ApiError::internal(format!("bad passwd line for {user}")))?,
        );
        // Home must be absolute, canonical in shape (no `..`), and not a
        // system prefix a uid>=1000 account must never own.
        if !home.is_absolute()
            || home
                .components()
                .any(|c| matches!(c, std::path::Component::ParentDir))
            || home.components().count() < 2
            || SYSTEM_PREFIXES.iter().any(|p| home.starts_with(p))
        {
            return Err(ApiError::internal(format!("bad home for {user}")));
        }
        return Ok((home, uid, gid));
    }
    Err(ApiError::invalid(format!("unknown user {user}")))
}

/// Home prefixes a normal login user must never own.
const SYSTEM_PREFIXES: [&str; 7] = ["/etc", "/usr", "/var", "/proc", "/sys", "/dev", "/root"];

/// The winning source for this user: config presence decides availability,
/// a running Wayle of the same uid beats DMS; DMS is the default when both
/// are configured (running DMS or neither running).
fn select_source(home: &Path, uid: u32) -> Option<ThemeSource> {
    let wayle_config = home.join(WAYLE_CONFIG);
    let dms_config = home.join(DMS_SETTINGS);
    let wayle_configured = wayle_config.is_file();
    let dms_configured = dms_config.is_file();
    if !wayle_configured && !dms_configured {
        return None;
    }
    prefer_source(ShellPresence {
        wayle_configured,
        dms_configured,
        wayle_running: wayle_running(uid),
    })
}

/// Which shells exist/run for the target user.
#[derive(Clone, Copy)]
struct ShellPresence {
    wayle_configured: bool,
    dms_configured: bool,
    wayle_running: bool,
}

/// Pure precedence rule (unit tested): running Wayle wins; when both are
/// configured otherwise, DMS wins deterministically.
fn prefer_source(presence: ShellPresence) -> Option<ThemeSource> {
    match (presence.wayle_configured, presence.dms_configured) {
        (false, false) => None,
        (true, false) => Some(ThemeSource::Wayle),
        (false, true) => Some(ThemeSource::Dms),
        (true, true) => {
            if presence.wayle_running {
                Some(ThemeSource::Wayle)
            } else {
                Some(ThemeSource::Dms)
            }
        }
    }
}

/// True when Wayle runs as `uid` (scan `/proc`, cheap and read-only).
fn wayle_running(uid: u32) -> bool {
    let Ok(entries) = std::fs::read_dir("/proc") else {
        return false;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let numeric = name.to_str().is_some_and(|n| n.parse::<u32>().is_ok());
        if !numeric {
            continue;
        }
        let dir = entry.path();
        let status = read_or_empty(&dir.join("status"));
        let owns = status.lines().find_map(|line| {
            line.strip_prefix("Uid:")
                .and_then(|rest| rest.split_whitespace().next())
                .and_then(|real| real.parse::<u32>().ok())
        });
        if owns != Some(uid) {
            continue;
        }
        let cmdline = std::fs::read(dir.join("cmdline")).unwrap_or_default();
        // cmdline is NUL-separated argv; match argv[0]'s file name exactly
        // (never a substring: opening a wayle config must not count).
        let argv0 = cmdline
            .split(|b| *b == 0)
            .next()
            .map(|raw| String::from_utf8_lossy(raw).into_owned())
            .unwrap_or_default();
        let base = argv0.rsplit('/').next().unwrap_or_default();
        if base == "wayle" {
            return true;
        }
    }
    false
}

/// Current state of the minimize toggle: read back from the written
/// settings.ini (our source of truth); absent key = trio on.
fn baseline_show_minimize(home: &Path) -> bool {
    for rel in [
        ".config/gtk-4.0/settings.ini",
        ".config/gtk-3.0/settings.ini",
    ] {
        let text = read_or_empty(&home.join(rel));
        if let Some(line) = text
            .lines()
            .find(|l| l.starts_with("gtk-decoration-layout="))
        {
            let value = line.split_once('=').map(|(_, v)| v).unwrap_or_default();
            return value.contains("minimize");
        }
    }
    true
}

/// Read the winning source's config into a snapshot.
fn snapshot_for(source: ThemeSource, home: &Path) -> Result<ThemeSnapshot, ApiError> {
    match source {
        ThemeSource::Wayle => wayle::snapshot(&read_or_empty(&home.join(WAYLE_CONFIG))),
        ThemeSource::Dms => dms::snapshot(
            &read_or_empty(&home.join(DMS_SETTINGS)),
            &read_or_empty(&home.join(DMS_CSS)),
        ),
    }
}

/// Per-target availability for this home (shared by detect and apply).
fn target_statuses(home: &Path) -> Vec<TargetStatus> {
    let has = |rel: &str| home.join(rel).exists();
    let bin = |candidates: &[&str]| -> bool { candidates.iter().any(|c| Path::new(c).is_file()) };
    // Gate on profiles.ini (SPEC), not just the directory.
    let firefox = has(".mozilla/firefox/profiles.ini")
        || has(".var/app/org.mozilla.firefox/.mozilla/firefox/profiles.ini");
    let mut statuses = vec![
        TargetStatus::available(ThemingTarget::Gtk3, "settings.ini + gtk.css"),
        TargetStatus::available(ThemingTarget::Gtk4, "settings.ini + gtk.css"),
        TargetStatus::available(ThemingTarget::Gtk2, "~/.gtkrc-2.0"),
        if bin(&["/usr/bin/qt5ct", "/usr/local/bin/qt5ct"]) || has(".config/qt5ct") {
            TargetStatus::available(ThemingTarget::Qt5, "~/.config/qt5ct")
        } else {
            TargetStatus::unavailable(ThemingTarget::Qt5, "qt5ct not installed")
        },
        if bin(&["/usr/bin/qt6ct", "/usr/local/bin/qt6ct"]) || has(".config/qt6ct") {
            TargetStatus::available(ThemingTarget::Qt6, "~/.config/qt6ct")
        } else {
            TargetStatus::unavailable(ThemingTarget::Qt6, "qt6ct not installed")
        },
        // The writer merges files directly (no kwriteconfig call): gate on
        // KDE presence itself (config or tooling).
        if bin(&["/usr/bin/kwriteconfig5", "/usr/bin/kwriteconfig6"])
            || has(".config/kdeglobals")
            || has(".local/share/color-schemes")
        {
            TargetStatus::available(ThemingTarget::Kde, "kdeglobals + color scheme")
        } else {
            TargetStatus::unavailable(ThemingTarget::Kde, "KDE not detected")
        },
        if bin(&["/usr/bin/flatpak"]) {
            TargetStatus::available(ThemingTarget::Flatpak, "flatpak overrides/global")
        } else {
            TargetStatus::unavailable(ThemingTarget::Flatpak, "flatpak not installed")
        },
        // Self-sufficient: the writer creates the cosmic-config tree.
        TargetStatus::available(ThemingTarget::Cosmic, "cosmic-config entries (created)"),
        TargetStatus::available(ThemingTarget::Environment, "~/.config/environment.d"),
        if has(".config/xdg-desktop-portal") || Path::new("/usr/share/xdg-desktop-portal").exists()
        {
            TargetStatus::available(ThemingTarget::Portal, "settings portal reads GTK/Qt config")
        } else {
            TargetStatus::unavailable(ThemingTarget::Portal, "no xdg portal")
        },
        if firefox {
            TargetStatus::available(ThemingTarget::Firefox, "profile chrome/userChrome.css")
        } else {
            TargetStatus::unavailable(ThemingTarget::Firefox, "no Firefox profile yet")
        },
        if has(".config/VSCodium") {
            TargetStatus::available(ThemingTarget::Vscodium, "User/settings.json")
        } else if has(".config/Code") {
            TargetStatus::available(ThemingTarget::Vscodium, "Code User/settings.json")
        } else {
            TargetStatus::unavailable(ThemingTarget::Vscodium, "VSCode family not configured")
        },
        if has(".config/alacritty") || bin(&["/usr/bin/alacritty", "/usr/local/bin/alacritty"]) {
            TargetStatus::available(ThemingTarget::Alacritty, "alacritty.toml")
        } else {
            TargetStatus::unavailable(ThemingTarget::Alacritty, "alacritty not installed")
        },
        if has(".config/kitty") || bin(&["/usr/bin/kitty"]) {
            TargetStatus::available(ThemingTarget::Kitty, "kitty.conf")
        } else {
            TargetStatus::unavailable(ThemingTarget::Kitty, "kitty not installed")
        },
        if has(".config/ghostty") || bin(&["/usr/bin/ghostty"]) {
            TargetStatus::available(ThemingTarget::Ghostty, "ghostty config")
        } else {
            TargetStatus::unavailable(ThemingTarget::Ghostty, "ghostty not installed")
        },
        if has(".config/foot") || bin(&["/usr/bin/foot"]) {
            TargetStatus::available(ThemingTarget::Foot, "foot.ini")
        } else {
            TargetStatus::unavailable(ThemingTarget::Foot, "foot not installed")
        },
        if has(".config/wezterm") || bin(&["/usr/bin/wezterm"]) {
            TargetStatus::available(ThemingTarget::Wezterm, "wezterm.lua")
        } else {
            TargetStatus::unavailable(ThemingTarget::Wezterm, "wezterm not installed")
        },
        if has(".config/cava/config") || has(".config/cava") {
            TargetStatus::available(ThemingTarget::Cava, "cava config [color]")
        } else {
            TargetStatus::unavailable(ThemingTarget::Cava, "cava not configured")
        },
        if has(".config/electron-flags.conf")
            || bin(&[
                "/usr/bin/electron",
                "/usr/bin/electron30",
                "/usr/bin/electron28",
            ])
        {
            TargetStatus::available(ThemingTarget::Electron, "electron-flags.conf")
        } else {
            TargetStatus::unavailable(ThemingTarget::Electron, "no Electron app detected")
        },
        if has(".config/galculator/galculator.conf") || bin(&["/usr/bin/galculator"]) {
            TargetStatus::available(ThemingTarget::Galculator, "galculator display colors")
        } else {
            TargetStatus::unavailable(ThemingTarget::Galculator, "galculator not installed")
        },
        if bin(&["/usr/bin/eog", "/usr/local/bin/eog"])
            || has(".local/share/applications/org.gnome.eog.desktop")
        {
            TargetStatus::available(ThemingTarget::Eog, "eog view background")
        } else {
            TargetStatus::unavailable(ThemingTarget::Eog, "eog not installed")
        },
    ];
    statuses.sort_by_key(|s| {
        ThemingTarget::ALL
            .iter()
            .position(|t| *t == s.target)
            .unwrap_or(usize::MAX)
    });
    statuses
}

#[allow(clippy::unnecessary_wraps)] // same shape as get/apply for the helper
fn detect_blocking(home: &Path, uid: u32, _gid: u32) -> Result<ThemingStatus, ApiError> {
    let source = select_source(home, uid);
    Ok(ThemingStatus {
        available: source.is_some(),
        source,
        targets: if source.is_some() {
            target_statuses(home)
        } else {
            Vec::new()
        },
        show_minimize: baseline_show_minimize(home),
    })
}

fn snapshot_blocking(home: &Path, uid: u32, _gid: u32) -> Result<ThemeSnapshot, ApiError> {
    let source = select_source(home, uid).ok_or_else(|| {
        ApiError::unavailable(
            "no supported desktop shell config (Wayle or DankMaterialShell)",
            "install wayle or DankMaterialShell and sign in once",
        )
    })?;
    snapshot_for(source, home)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::disallowed_methods)] // tests use the host temp dir
    use super::*;
    use serde_json::json;

    #[test]
    fn passwd_lookup_rejects_root_and_unknown() {
        let passwd = "root:x:0:0:root:/root:/bin/sh\ngab:x:1000:1000:gab:/home/gab:/bin/sh\n";
        let (home, uid, gid) = identity_from_passwd(passwd, "gab").unwrap();
        assert_eq!(home, PathBuf::from("/home/gab"));
        assert_eq!((uid, gid), (1000, 1000));
        assert!(identity_from_passwd(passwd, "root").is_err());
        assert!(identity_from_passwd(passwd, "ghost").is_err());
    }

    #[test]
    fn source_precedence_running_wayle_wins_otherwise_dms() {
        use ThemeSource::{Dms, Wayle};
        let presence = |wayle_cfg: bool, dms_cfg: bool, wayle_run: bool| ShellPresence {
            wayle_configured: wayle_cfg,
            dms_configured: dms_cfg,
            wayle_running: wayle_run,
        };
        assert_eq!(prefer_source(presence(false, false, false)), None);
        assert_eq!(prefer_source(presence(true, false, false)), Some(Wayle));
        assert_eq!(prefer_source(presence(false, true, false)), Some(Dms));
        // Both configured: running Wayle of the session user wins.
        assert_eq!(prefer_source(presence(true, true, true)), Some(Wayle));
        // Both configured, Wayle not running: DMS default (running or not).
        assert_eq!(prefer_source(presence(true, true, false)), Some(Dms));
        // Wayle running but not configured: ignored (config decides).
        assert_eq!(prefer_source(presence(false, true, true)), Some(Dms));
    }

    #[test]
    fn detect_is_inactive_without_any_shell_config() {
        let home = std::env::temp_dir().join(format!("ursine-theme-none-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        std::fs::create_dir_all(&home).unwrap();
        let status = detect_blocking(&home, 1000, 1000).unwrap();
        assert!(!status.available);
        assert!(status.source.is_none());
        assert!(status.targets.is_empty());
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn detect_reports_source_and_targets_with_dms_config() {
        let home = std::env::temp_dir().join(format!("ursine-theme-dms-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        let rel = home.join(".config/DankMaterialShell");
        std::fs::create_dir_all(&rel).unwrap();
        std::fs::write(rel.join("settings.json"), "{}").unwrap();
        let status = detect_blocking(&home, 1000, 1000).unwrap();
        assert!(status.available);
        assert_eq!(status.source, Some(ThemeSource::Dms));
        assert_eq!(status.targets.len(), ThemingTarget::ALL.len());
        let names: Vec<&str> = status
            .targets
            .iter()
            .filter(|t| t.available)
            .map(|t| t.target.as_str())
            .collect();
        // Home-scoped targets are writable in a fresh temp home; tool-gated
        // ones (qtct, KDE, flatpak, Firefox, terminals, Electron …) stay
        // unavailable. COSMIC is self-sufficient (creates its config tree).
        assert_eq!(names, vec!["gtk3", "gtk4", "gtk2", "cosmic", "environment"]);
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn snapshot_requires_a_configured_shell() {
        let home = std::env::temp_dir().join(format!("ursine-theme-req-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        std::fs::create_dir_all(&home).unwrap();
        let err = snapshot_blocking(&home, 1000, 1000).unwrap_err();
        assert_eq!(err.code, ursine_api::ErrorCode::Unavailable);
        let _ = std::fs::remove_dir_all(&home);
    }

    fn temp_home_dir(tag: &str) -> PathBuf {
        let home = std::env::temp_dir().join(format!("ursine-th-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        std::fs::create_dir_all(&home).unwrap();
        home
    }
    #[test]
    fn minimize_toggle_reads_back_from_settings_ini() {
        let home = temp_home_dir("min-read");
        // Absent key = trio on (default).
        assert!(baseline_show_minimize(&home));
        let dir = home.join(".config/gtk-4.0");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("settings.ini"),
            "[Settings]\ngtk-decoration-layout=:maximize,close\n",
        )
        .unwrap();
        assert!(!baseline_show_minimize(&home));
        std::fs::write(
            dir.join("settings.ini"),
            "[Settings]\ngtk-decoration-layout=:minimize,maximize,close\n",
        )
        .unwrap();
        assert!(baseline_show_minimize(&home));
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn json_helper_shapes_detect_response() {
        // Shapes the wire response the GUI renders.
        let status = ThemingStatus {
            available: false,
            source: None,
            targets: vec![],
            show_minimize: true,
        };
        assert_eq!(
            json!({"available": false}).to_string(),
            "{\"available\":false}"
        );
        assert!(serde_json::to_value(status)
            .unwrap()
            .get("source")
            .is_none());
    }

    /// Live probe (opt-in): parse a REAL shell home and apply into a
    /// throwaway copy. Run with the user's home mounted read-only:
    /// `URSTHEME_PROBE_HOME=/host-home cargo test -p ursine-core live_probe -- --ignored`
    #[test]
    #[ignore = "live probe: set URSTHEME_PROBE_HOME to a home with DMS/Wayle"]
    fn live_probe_parses_real_config_and_applies() {
        let Some(real_home) = std::env::var_os("URSTHEME_PROBE_HOME") else {
            return;
        };
        let real_home = PathBuf::from(real_home);
        let source = select_source(&real_home, 0).expect("probe home must have a shell config");
        let snapshot = snapshot_for(source, &real_home).expect("snapshot from real config");
        assert!(!snapshot.colors.background.is_empty());
        assert!(!snapshot.colors.primary.is_empty());

        let probe_home =
            std::env::temp_dir().join(format!("ursine-theme-probe-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&probe_home);
        std::fs::create_dir_all(probe_home.join(".config")).unwrap();
        // Seed the throwaway home with the same shell config so detection
        // and the snapshot both resolve inside the probe home.
        let copy = |rel: &str| -> std::io::Result<()> {
            let from = real_home.join(rel);
            let to = probe_home.join(rel);
            std::fs::create_dir_all(to.parent().unwrap())?;
            std::fs::copy(&from, to)?;
            Ok(())
        };
        match source {
            ThemeSource::Dms => {
                copy(".config/DankMaterialShell/settings.json").unwrap();
                copy(".config/gtk-3.0/dank-colors.css").unwrap();
            }
            ThemeSource::Wayle => copy(".config/wayle/config.toml").unwrap(),
        }
        let report = apply::run_blocking(
            "tester",
            &probe_home,
            std::process::id(),
            std::process::id(),
            true,
        )
        .expect("apply into throwaway home");
        let written = report
            .results
            .iter()
            .filter(|r| r.outcome == ursine_api::ApplyOutcome::Written)
            .count();
        assert!(written >= 5, "too few written targets: {report:?}");
        assert!(probe_home.join(".config/gtk-3.0/settings.ini").is_file());
        assert!(probe_home
            .join(".config/environment.d/95-ursine-theme.conf")
            .is_file());
        let _ = std::fs::remove_dir_all(&probe_home);
    }
}
