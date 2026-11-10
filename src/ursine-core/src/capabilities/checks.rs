//! Individual capability checks and their UI hints.
//!
//! Cheap probes only: `stat` and tiny reads. Every check names the exact
//! CLI its module runs (no D-Bus probing — invariant 3) and every hint is
//! the install command for that binary.

use std::path::{Path, PathBuf};

use ursine_api::Capability;

/// Filesystem/proc access, abstracted for unit tests.
pub trait Sysinfo: Send + Sync {
    fn path_exists(&self, path: &Path) -> bool;
    fn read_trimmed(&self, path: &Path) -> Option<String>;
    fn cmd_in_path(&self, cmd: &str) -> bool;
}

/// Real system access.
#[derive(Debug)]
pub struct SystemSysinfo;

impl Sysinfo for SystemSysinfo {
    fn path_exists(&self, path: &Path) -> bool {
        path.exists()
    }

    fn read_trimmed(&self, path: &Path) -> Option<String> {
        std::fs::read_to_string(path)
            .ok()
            .map(|s| s.trim().to_string())
    }

    fn cmd_in_path(&self, cmd: &str) -> bool {
        let Some(paths) = std::env::var_os("PATH") else {
            return false;
        };
        std::env::split_paths(&paths).any(|dir| {
            let candidate = dir.join(cmd);
            candidate.is_file()
        })
    }
}

/// One capability, live check.
pub fn check_capability(sys: &dyn Sysinfo, cap: Capability) -> bool {
    match cap {
        Capability::Btrbk => sys.cmd_in_path("btrbk"),
        Capability::Btrfs => sys.path_exists(Path::new("/sys/fs/btrfs")),
        Capability::Nix => sys.path_exists(Path::new("/nix/store")) && sys.cmd_in_path("nix"),
        Capability::Apk => {
            sys.cmd_in_path("apk")
                || sys.path_exists(Path::new("/sbin/apk"))
                || sys.path_exists(Path::new("/usr/sbin/apk"))
        }
        Capability::Flatpak => {
            sys.cmd_in_path("flatpak")
                || sys.path_exists(Path::new("/usr/bin/flatpak"))
                || sys.path_exists(Path::new("/usr/local/bin/flatpak"))
        }
        Capability::Cups => {
            sys.cmd_in_path("lpstat")
                || sys.path_exists(Path::new("/usr/bin/lpstat"))
                || sys.path_exists(Path::new("/run/cups/cups.sock"))
        }
        Capability::Sane => {
            sys.cmd_in_path("scanimage")
                || sys.path_exists(Path::new("/usr/bin/scanimage"))
                || sys.path_exists(Path::new("/usr/local/bin/scanimage"))
        }
        Capability::Firewalld => {
            sys.cmd_in_path("firewall-cmd")
                || sys.path_exists(Path::new("/usr/bin/firewall-cmd"))
                || sys.path_exists(Path::new("/run/firewalld"))
        }
        Capability::OpenRc => {
            sys.path_exists(Path::new("/run/openrc"))
                || sys.read_trimmed(Path::new("/proc/1/comm")).as_deref() == Some("openrc-init")
        }
        // The disks module runs `parted` for partition/format; listing
        // reads kernel sysfs, mount/umount are base system tools.
        Capability::Parted => {
            sys.cmd_in_path("parted")
                || ["/usr/sbin", "/usr/bin", "/sbin"]
                    .iter()
                    .any(|dir| sys.path_exists(&Path::new(dir).join("parted")))
        }
        Capability::Bluez => {
            sys.cmd_in_path("bluetoothctl") || sys.path_exists(Path::new("/usr/bin/bluetoothctl"))
        }
        Capability::NetworkManager => {
            sys.cmd_in_path("nmcli") || sys.path_exists(Path::new("/usr/bin/nmcli"))
        }
        // The module runs `useradd`/`userdel` (shadow) with BusyBox
        // `adduser`/`deluser` as the fallback — same candidate paths
        // accounts.rs probes (keep the two in lockstep).
        Capability::Shadow => ["useradd", "adduser"].iter().any(|cmd| {
            if sys.cmd_in_path(cmd) {
                return true;
            }
            ["/usr/sbin", "/sbin", "/usr/bin"]
                .iter()
                .any(|dir| sys.path_exists(&Path::new(dir).join(cmd)))
        }),
        Capability::Niri => sys.cmd_in_path("niri"),
        Capability::Dinit => {
            sys.read_trimmed(Path::new("/proc/1/comm")).as_deref() == Some("dinit")
        }
        Capability::Crontab => sys.cmd_in_path("crontab"),
        Capability::Booster => {
            sys.cmd_in_path("booster")
                || ["/usr/bin", "/usr/sbin", "/bin", "/sbin"]
                    .iter()
                    .any(|dir| sys.path_exists(&Path::new(dir).join("booster")))
        }
        // Uniform theming: ANY normal login user (uid 1000..60000, aligned
        // with the module's identity check) has a supported shell config.
        // `theming.detect` refines per target user at call time.
        Capability::Theming => passwd_homes(
            &sys.read_trimmed(Path::new("/etc/passwd"))
                .unwrap_or_default(),
        )
        .any(|home| {
            sys.path_exists(&home.join(".config/wayle/config.toml"))
                || sys.path_exists(&home.join(".config/DankMaterialShell/settings.json"))
        }),
    }
}

/// Normal login users' homes from a `/etc/passwd` dump (uid 1000..60000).
fn passwd_homes(passwd: &str) -> impl Iterator<Item = PathBuf> + '_ {
    passwd.lines().filter_map(|line| {
        let fields: Vec<&str> = line.split(':').collect();
        if fields.len() < 6 {
            return None;
        }
        let uid: u32 = fields.get(2)?.parse().ok()?;
        let home = fields.get(5)?;
        ((1000..60000).contains(&uid) && home.starts_with('/')).then(|| PathBuf::from(home))
    })
}

/// Human reason + exact fix command for an unavailable capability.
pub fn hints(cap: Capability) -> (&'static str, &'static str) {
    match cap {
        Capability::Btrbk => ("btrbk not installed", "apk add btrbk"),
        Capability::Btrfs => (
            "no btrfs filesystem detected",
            "reinstall FeralOS with a btrfs layout",
        ),
        Capability::Nix => (
            "nix is not available",
            "enable nix in the FeralOS installer / run: apk add nix",
        ),
        Capability::Apk => ("apk is not available", "boot a FeralOS/Alpine system"),
        Capability::Flatpak => ("flatpak is not installed", "apk add flatpak"),
        Capability::Cups => (
            "CUPS is not available",
            "apk add cups && rc-service cupsd start",
        ),
        Capability::Sane => ("SANE is not available", "apk add sane-utils"),
        Capability::Firewalld => (
            "firewalld is not available",
            "apk add firewalld && rc-service firewalld start",
        ),
        Capability::OpenRc => ("OpenRC is not available", "boot an OpenRC system"),
        Capability::Parted => ("parted not installed", "apk add parted"),
        Capability::Bluez => (
            "BlueZ is not available",
            "apk add bluez && rc-service bluetooth start",
        ),
        Capability::NetworkManager => (
            "NetworkManager is not available",
            "apk add networkmanager && rc-service networkmanager start",
        ),
        Capability::Shadow => ("user CLI not found", "apk add shadow"),
        Capability::Niri => ("niri not installed", "apk add niri"),
        Capability::Dinit => ("PID 1 is not dinit", "boot with dinit (OpenRC fallback)"),
        Capability::Theming => (
            "no Wayle or DankMaterialShell config found",
            "install wayle or DankMaterialShell and sign in once",
        ),
        Capability::Crontab => (
            "crontab is not installed",
            "apk add dcron  # or: apt install cron",
        ),
        Capability::Booster => ("booster is not installed", "apk add booster"),
    }
}
