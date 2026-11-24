//! power — machine power state and identity: `reboot`, `poweroff`,
//! `set_hostname`, `set_timezone` (SPEC-system).
//!
//! The init owns the shutdown. `FeralOS` boots dinit, where the `BusyBox`
//! `reboot` and `poweroff` applets return without doing anything while
//! `dinit-reboot` and `dinit-poweroff` drive the stop sequence, so the
//! candidates are resolved the way the services module resolves `dinitctl`:
//! absolute paths, dinit first, and an `unavailable` answer with the install
//! hint when nothing matches. A power action never silently does nothing.
//!
//! The identity actions write `/etc` through the shared backup rotation and
//! validate their payload first, because they need no `--yes` confirmation
//! (a wrong host name or zone must be refused, not confirmed).

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::json;
use ursine_api::{Action, ApiError};

use super::ModuleCtx;
use crate::modules::packages_nix::exec;

/// File the kernel name is read from at boot (`sethostname` seeds it).
const HOSTNAME_FILE: &str = "/etc/hostname";
/// File that records the zone NAME (`/etc/localtime` records the rule set).
const TIMEZONE_FILE: &str = "/etc/timezone";
/// Symlink every process resolves the local zone through.
const LOCALTIME_LINK: &str = "/etc/localtime";
/// Tree the zone names are validated against.
const ZONEINFO_ROOT: &str = "/usr/share/zoneinfo";
/// A power call returns as soon as the init accepted it; the identity
/// commands finish in milliseconds. Both budgets are generous for a machine
/// that is already paging its services out.
const TIMEOUT: Duration = Duration::from_secs(30);

/// dinit's own reboot tool. It comes first in the candidate list because on
/// `FeralOS` the `BusyBox` applet returns success and reboots nothing.
const DINIT_REBOOT: [&str; 4] = [
    "/usr/bin/dinit-reboot",
    "/usr/sbin/dinit-reboot",
    "/sbin/dinit-reboot",
    "/bin/dinit-reboot",
];
/// dinit's shutdown counterpart.
const DINIT_POWEROFF: [&str; 4] = [
    "/usr/bin/dinit-poweroff",
    "/usr/sbin/dinit-poweroff",
    "/sbin/dinit-poweroff",
    "/bin/dinit-poweroff",
];
/// `sysvinit`/`OpenRC` reboot; `sbin` first, then `bin` (a `BusyBox` box
/// keeps the applets in `/bin`).
const REBOOT: [&str; 4] = [
    "/sbin/reboot",
    "/usr/sbin/reboot",
    "/usr/bin/reboot",
    "/bin/reboot",
];
/// `sysvinit`/`OpenRC` poweroff.
const POWEROFF: [&str; 4] = [
    "/sbin/poweroff",
    "/usr/sbin/poweroff",
    "/usr/bin/poweroff",
    "/bin/poweroff",
];
/// The tool that renames the running kernel host (`hostname -F` reads a file,
/// a bare argument sets the live name).
const HOSTNAME: [&str; 4] = [
    "/usr/bin/hostname",
    "/bin/hostname",
    "/usr/sbin/hostname",
    "/sbin/hostname",
];

pub async fn handle(action: &Action, ctx: &ModuleCtx<'_>) -> Result<serde_json::Value, ApiError> {
    let paths = PowerPaths::host();
    let copies = crate::backup_configs::copies_from(&ctx.config);
    match action {
        Action::Reboot => power(PowerOp::Reboot).await,
        Action::PowerOff => power(PowerOp::PowerOff).await,
        Action::SetHostname { name } => set_hostname(&paths, name, copies).await,
        Action::SetTimezone { zone } => set_timezone(&paths, zone, copies).await,
        _ => Err(ApiError::invalid("action is not valid for module system")),
    }
}

/// One power operation: the two differ only in their command table, so the
/// answer text and the candidate lookup are derived from one value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PowerOp {
    Reboot,
    PowerOff,
}

impl PowerOp {
    /// Name used in the answer and in the error message.
    fn label(self) -> &'static str {
        match self {
            Self::Reboot => "reboot",
            Self::PowerOff => "poweroff",
        }
    }

    /// Candidate programs, dinit first (see the module doc).
    fn candidates(self) -> [&'static [&'static str]; 2] {
        match self {
            Self::Reboot => [DINIT_REBOOT.as_slice(), REBOOT.as_slice()],
            Self::PowerOff => [DINIT_POWEROFF.as_slice(), POWEROFF.as_slice()],
        }
    }

    /// What installs a command that drives this init.
    fn fix(self) -> &'static str {
        match self {
            Self::Reboot => "apk add dinit  # provides dinit-reboot, or install sysvinit/OpenRC",
            Self::PowerOff => {
                "apk add dinit  # provides dinit-poweroff, or install sysvinit/OpenRC"
            }
        }
    }
}

/// A resolved power command: an absolute program, no arguments.
///
/// `reboot` and `poweroff` act immediately and take no option we set, so the
/// whole call is the program name and the argv is empty.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct PowerCommand {
    program: &'static str,
}

/// Resolve the program for `op` through an existence predicate.
///
/// The predicate is the seam: `is_file` in production, a fixed answer in the
/// tests ("`FeralOS`", "`OpenRC` box", "no tool at all") so the lookup is checked
/// without the host.
fn power_command(op: PowerOp, exists: impl Fn(&str) -> bool) -> Result<PowerCommand, ApiError> {
    for group in op.candidates() {
        if let Some(program) = group.iter().copied().find(|path| exists(path)) {
            return Ok(PowerCommand { program });
        }
    }
    Err(ApiError::unavailable(
        format!("no {} command on this machine", op.label()),
        op.fix(),
    ))
}

/// Ask the init for one power operation and return the command that was used.
async fn power(op: PowerOp) -> Result<serde_json::Value, ApiError> {
    let command = power_command(op, |path| Path::new(path).is_file())?;
    // The answer usually never reaches the client: the machine stops while
    // the call is in flight. A returned error therefore means the init
    // refused, which is exactly what the caller must hear.
    exec::run_root_program(command.program, &[], TIMEOUT)
        .await
        .map_err(|error| {
            error
                .with_fix("read the init log (dmesg, /var/log) and retry once the refusal is fixed")
        })?;
    Ok(json!({ "ok": true, "op": op.label(), "program": command.program }))
}

/// The host paths a power/identity action touches.
///
/// A struct rather than constants so the handler test can point every write at
/// a temporary directory: no test ever writes `/etc`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PowerPaths {
    pub hostname: PathBuf,
    pub timezone: PathBuf,
    pub localtime: PathBuf,
    pub zoneinfo: PathBuf,
}

impl PowerPaths {
    /// The real files of this machine.
    fn host() -> Self {
        Self {
            hostname: PathBuf::from(HOSTNAME_FILE),
            timezone: PathBuf::from(TIMEZONE_FILE),
            localtime: PathBuf::from(LOCALTIME_LINK),
            zoneinfo: PathBuf::from(ZONEINFO_ROOT),
        }
    }
}

/// Validate a host name as one RFC 1123 label.
///
/// Uppercase is refused on purpose: the name is written to `/etc/hostname` and
/// read back as the local part of every reported host name, so two spellings
/// of one machine would be two different machines to a client.
fn validate_hostname(name: &str) -> Result<(), ApiError> {
    let characters = name
        .bytes()
        .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-');
    let edges = !name.starts_with('-') && !name.ends_with('-');
    if !(1..=63).contains(&name.len()) || !characters || !edges {
        return Err(ApiError::invalid(format!(
            "invalid host name {name:?}: one label of 1 to 63 characters from [a-z0-9-], no leading or trailing dash"
        ))
        .with_fix("name the machine with lowercase letters, digits and inner dashes, e.g. `feral-box`"));
    }
    Ok(())
}

/// File text for a host name: one newline-terminated line.
fn hostname_text(name: &str) -> String {
    format!("{name}\n")
}

/// Rotate the live file into the backup chain, then replace it.
///
/// The order is binding: a failed rotation aborts before the live file is
/// touched. The blocking IO runs on the blocking pool so a slow `/etc` never
/// stalls the async executor.
async fn replace_config(path: PathBuf, text: String, copies: usize) -> Result<(), ApiError> {
    crate::tooling::blocking("config write", move || {
        crate::backup_configs::rotate(&path, copies, None)?;
        crate::atomic::write(&path, &text)
    })
    .await
}

/// Validate the name and replace `/etc/hostname`, rotating the old file into
/// the backup chain.
async fn write_hostname(paths: &PowerPaths, name: &str, copies: usize) -> Result<(), ApiError> {
    // The seam validates on its own: no caller can replace the file with a
    // name the boot would read back unchanged and reject.
    validate_hostname(name)?;
    replace_config(paths.hostname.clone(), hostname_text(name), copies).await
}

/// Write `/etc/hostname`, then apply the name to the running kernel.
///
/// The command is resolved BEFORE the file changes: a machine without
/// `hostname` keeps its old file instead of a file nothing applied.
async fn set_hostname(
    paths: &PowerPaths,
    name: &str,
    copies: usize,
) -> Result<serde_json::Value, ApiError> {
    validate_hostname(name)?;
    let program = crate::tooling::resolve(
        &HOSTNAME,
        "hostname",
        "apk add busybox  # provides /bin/hostname",
    )?;
    write_hostname(paths, name, copies).await?;
    exec::run_root_program(program, &[name], TIMEOUT)
        .await
        .map_err(|error| {
            error.with_fix(
                "the new name is already in /etc/hostname and applies at the next boot; run `hostname <name>` yourself to apply it now",
            )
        })?;
    Ok(json!({ "ok": true, "name": name }))
}

/// The zoneinfo file a zone name resolves to, inside `root`.
///
/// Shape first, so a traversal never reaches the file system; then a metadata
/// check that FOLLOWS symlinks, because `US/Eastern` is a link to
/// `America/New_York` inside the tree and must stay accepted while a directory
/// (`Europe/`) and a missing name are refused. A link that leaves the tree is
/// refused too: the name must select a rule set, not an arbitrary file.
fn zone_file(root: &Path, zone: &str) -> Result<PathBuf, ApiError> {
    validate_zone_name(zone)?;
    let path = root.join(zone);
    match std::fs::metadata(&path) {
        Ok(meta) if meta.is_file() => inside_tree(root, &path, zone),
        Ok(_) => Err(invalid_zone(zone, "is not a regular file")),
        Err(_) => Err(invalid_zone(zone, "is not in the zoneinfo tree")),
    }
}

/// Accept `path` only when its resolved target stays under `root`.
fn inside_tree(root: &Path, path: &Path, zone: &str) -> Result<PathBuf, ApiError> {
    let real_root = std::fs::canonicalize(root)
        .map_err(|e| ApiError::internal(format!("read {}: {e}", root.display())))?;
    let real_file = std::fs::canonicalize(path)
        .map_err(|_| invalid_zone(zone, "cannot be resolved inside the zoneinfo tree"))?;
    if real_file.starts_with(&real_root) {
        Ok(path.to_path_buf())
    } else {
        Err(invalid_zone(zone, "resolves outside the zoneinfo tree"))
    }
}

/// Validate the shape of a zone name: slash-separated components of
/// `[A-Za-z0-9_+-]`.
///
/// `.` is not in the class, which is what rejects both `..` traversal and a
/// hidden entry; a leading slash is refused so the name cannot leave the
/// zoneinfo tree through `join`.
fn validate_zone_name(zone: &str) -> Result<(), ApiError> {
    let shaped = !zone.is_empty()
        && !zone.starts_with('/')
        && zone.split('/').all(|part| {
            !part.is_empty()
                && part
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'+' | b'-'))
        });
    if !shaped {
        return Err(invalid_zone(zone, "is not a zone name"));
    }
    Ok(())
}

/// One refusal, with the command that lists the valid answers.
fn invalid_zone(zone: &str, why: &str) -> ApiError {
    ApiError::invalid(format!("invalid time zone {zone:?}: {why}")).with_fix(
        "list the valid names with `timedatectl list-timezones`, which are the files under /usr/share/zoneinfo",
    )
}

/// Point `/etc/localtime` at the zone and record the name in `/etc/timezone`;
/// returns the zoneinfo file the link now names.
///
/// Both edits run under one validation: the zone is resolved inside the tree
/// first, and the link is replaced before the name is written, so a machine
/// that dies between the two writes holds the rule set that matters.
async fn install_timezone(
    paths: &PowerPaths,
    zone: &str,
    copies: usize,
) -> Result<PathBuf, ApiError> {
    let target = zone_file(&paths.zoneinfo, zone)?;
    let localtime = paths.localtime.clone();
    let timezone = paths.timezone.clone();
    let text = format!("{zone}\n");
    let installed = target.clone();
    crate::tooling::blocking("time zone write", move || {
        crate::backup_configs::rotate(&timezone, copies, None)?;
        crate::atomic::symlink(&localtime, &target)?;
        crate::atomic::write(&timezone, &text)
    })
    .await?;
    Ok(installed)
}

/// Set the system time zone and answer the file the link points at.
async fn set_timezone(
    paths: &PowerPaths,
    zone: &str,
    copies: usize,
) -> Result<serde_json::Value, ApiError> {
    let target = install_timezone(paths, zone, copies).await?;
    Ok(json!({
        "ok": true,
        "zone": zone,
        "localtime": target.display().to_string(),
    }))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::disallowed_methods)] // the fixtures need a temp dir

    use super::*;

    /// A temp root for one test; the label keeps two tests in this module from
    /// sharing a directory when they run in parallel.
    fn temp_root(label: &str) -> PathBuf {
        let root =
            std::env::temp_dir().join(format!("ursine-power-{label}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        root
    }

    /// Paths under `root`, so no test can reach a real `/etc` file.
    fn paths_under(root: &Path) -> PowerPaths {
        PowerPaths {
            hostname: root.join("hostname"),
            timezone: root.join("timezone"),
            localtime: root.join("localtime"),
            zoneinfo: root.join("zoneinfo"),
        }
    }

    /// Zoneinfo tree with one zone, one directory and one in-tree symlink
    /// (`US/Eastern` is a link to `America/New_York` in the real tree).
    fn zoneinfo_fixture(root: &Path) -> PathBuf {
        let zoneinfo = root.join("zoneinfo");
        std::fs::create_dir_all(zoneinfo.join("Europe")).unwrap();
        std::fs::write(zoneinfo.join("Europe").join("Rome"), "TZif2 fixture").unwrap();
        std::os::unix::fs::symlink("Europe/Rome", zoneinfo.join("Rome-link")).unwrap();
        zoneinfo
    }

    #[test]
    fn dinit_tools_win_when_both_inits_are_installed() {
        // FeralOS ships busybox `reboot`/`poweroff` next to dinit's tools; the
        // applets return success without rebooting anything.
        let exists = |path: &str| {
            matches!(
                path,
                "/usr/bin/dinit-reboot"
                    | "/usr/bin/dinit-poweroff"
                    | "/sbin/reboot"
                    | "/sbin/poweroff"
            )
        };
        assert_eq!(
            power_command(PowerOp::Reboot, exists).unwrap().program,
            "/usr/bin/dinit-reboot"
        );
        assert_eq!(
            power_command(PowerOp::PowerOff, exists).unwrap().program,
            "/usr/bin/dinit-poweroff"
        );
    }

    #[test]
    fn an_openrc_box_uses_the_classic_binaries() {
        let exists = |path: &str| matches!(path, "/sbin/reboot" | "/sbin/poweroff");
        assert_eq!(
            power_command(PowerOp::Reboot, exists).unwrap().program,
            "/sbin/reboot"
        );
        assert_eq!(
            power_command(PowerOp::PowerOff, exists).unwrap().program,
            "/sbin/poweroff"
        );
    }

    #[test]
    fn a_machine_without_either_init_answers_unavailable_with_a_fix() {
        // The whole point of the entry: a power action never silently does
        // nothing, so the missing tool is an answer with an install hint.
        for op in [PowerOp::Reboot, PowerOp::PowerOff] {
            let error = power_command(op, |_| false).unwrap_err();
            assert_eq!(error.code, ursine_api::ErrorCode::Unavailable);
            assert!(error.message.contains(op.label()), "{error:?}");
            let fix = error.fix.unwrap_or_default();
            assert!(fix.contains("dinit"), "{fix}");
        }
    }

    #[test]
    fn hostname_validation_accepts_one_rfc1123_label() {
        for name in ["a", "feral-box", "box1", "0rx", &"a".repeat(63)] {
            assert!(validate_hostname(name).is_ok(), "{name} must be valid");
        }
    }

    #[test]
    fn hostname_validation_refuses_anything_that_is_not_a_label() {
        for name in [
            "",
            "-box",
            "box-",
            "fer al",
            "box_1",
            "box.example.com",
            "FERAL",
            "caffè",
            &"a".repeat(64),
        ] {
            let error = validate_hostname(name).unwrap_err();
            assert_eq!(error.code, ursine_api::ErrorCode::Invalid, "{name}");
            assert!(error.fix.is_some(), "{name} needs a fix");
        }
    }

    #[test]
    fn zone_names_reject_traversal_and_absolute_paths() {
        for zone in [
            "",
            "/etc/localtime",
            "..",
            "../zoneinfo/UTC",
            "Europe/../UTC",
        ] {
            let error = validate_zone_name(zone).unwrap_err();
            assert_eq!(error.code, ursine_api::ErrorCode::Invalid, "{zone}");
            assert!(error.fix.is_some(), "{zone} needs a fix");
        }
        for zone in [
            "UTC",
            "Europe/Rome",
            "Etc/GMT+5",
            "America/Argentina/Buenos_Aires",
        ] {
            assert!(validate_zone_name(zone).is_ok(), "{zone} must be valid");
        }
    }

    #[test]
    fn zone_file_requires_a_regular_file_inside_the_tree() {
        let root = temp_root("zoneinfo");
        let zoneinfo = zoneinfo_fixture(&root);
        // A real zone and an in-tree symlink to one are accepted.
        assert_eq!(
            zone_file(&zoneinfo, "Europe/Rome").unwrap(),
            zoneinfo.join("Europe").join("Rome")
        );
        assert!(zone_file(&zoneinfo, "Rome-link").is_ok());
        // A directory, a name that is not in the tree and a traversal are not.
        assert!(zone_file(&zoneinfo, "Europe").is_err());
        let missing = zone_file(&zoneinfo, "Mars/Base").unwrap_err();
        assert_eq!(missing.code, ursine_api::ErrorCode::Invalid);
        assert!(zone_file(&zoneinfo, "../zoneinfo/Europe/Rome").is_err());
        // A link that leaves the tree selects no rule set, so it is refused
        // even though it resolves to a readable file.
        std::os::unix::fs::symlink("../../hostname", zoneinfo.join("escape")).unwrap();
        std::fs::write(root.join("hostname"), "x").unwrap();
        let escaped = zone_file(&zoneinfo, "escape").unwrap_err();
        assert_eq!(escaped.code, ursine_api::ErrorCode::Invalid);
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[tokio::test]
    async fn write_hostname_replaces_a_temporary_file_and_keeps_the_old_one() {
        // The handler path with every real path swapped for a temp one: no
        // `/etc/hostname` is touched.
        let root = temp_root("hostname");
        let paths = paths_under(&root);
        std::fs::write(&paths.hostname, "old-name\n").unwrap();
        write_hostname(&paths, "feral-box", 2).await.unwrap();
        assert_eq!(
            std::fs::read_to_string(&paths.hostname).unwrap(),
            "feral-box\n"
        );
        // The previous name is the newest backup copy, so a machine renamed by
        // mistake can be put back by hand.
        let newest = crate::backup_configs::slot_path(&paths.hostname, 0);
        assert_eq!(std::fs::read_to_string(newest).unwrap(), "old-name\n");
        // A refused name never reaches the file.
        assert!(write_hostname(&paths, "Bad Name", 2).await.is_err());
        assert_eq!(
            std::fs::read_to_string(&paths.hostname).unwrap(),
            "feral-box\n"
        );
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[tokio::test]
    async fn install_timezone_links_the_zone_and_records_its_name() {
        let root = temp_root("timezone");
        let paths = paths_under(&root);
        zoneinfo_fixture(&root);
        let installed = install_timezone(&paths, "Europe/Rome", 2).await.unwrap();
        assert_eq!(installed, paths.zoneinfo.join("Europe").join("Rome"));
        assert_eq!(
            std::fs::read_link(&paths.localtime).unwrap(),
            installed,
            "the local zone link points at the zoneinfo file"
        );
        assert_eq!(
            std::fs::read_to_string(&paths.timezone).unwrap(),
            "Europe/Rome\n"
        );
        // A second zone replaces the link (rename over a symlink), never a
        // stale one and never a missing link.
        std::fs::write(paths.zoneinfo.join("UTC"), "TZif2 fixture").unwrap();
        install_timezone(&paths, "UTC", 2).await.unwrap();
        assert_eq!(
            std::fs::read_link(&paths.localtime).unwrap(),
            paths.zoneinfo.join("UTC")
        );
        std::fs::remove_dir_all(&root).unwrap();
    }
}
