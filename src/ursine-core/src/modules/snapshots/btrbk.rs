//! btrbk raw-output parsing and root execution.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use ursine_api::{ApiError, Snapshot, SnapshotStats};

use crate::modules::packages_nix::exec;

pub(super) const LIST_TIMEOUT: Duration = Duration::from_mins(1);
pub(super) const MUTATE_TIMEOUT: Duration = Duration::from_mins(15);

const BTRBK_CANDIDATES: [&str; 3] = [
    "/usr/bin/btrbk",
    "/usr/sbin/btrbk",
    "/run/current-system/sw/bin/btrbk",
];
const BTRFS_CANDIDATES: [&str; 4] = [
    "/usr/bin/btrfs",
    "/sbin/btrfs",
    "/usr/sbin/btrfs",
    "/run/current-system/sw/bin/btrfs",
];

fn trusted_path(candidates: &[&'static str], label: &str) -> Result<&'static str, ApiError> {
    candidates
        .iter()
        .find(|path| Path::new(path).is_file())
        .copied()
        .ok_or_else(|| {
            ApiError::unavailable(format!("{label} not found"), format!("install {label}"))
        })
}

pub(super) fn btrbk_path() -> Result<&'static str, ApiError> {
    trusted_path(&BTRBK_CANDIDATES, "btrbk")
}

pub(super) fn btrfs_path() -> Result<&'static str, ApiError> {
    trusted_path(&BTRFS_CANDIDATES, "btrfs")
}

/// `/etc/btrbk/btrbk.conf`, overridable for tests/dev.
pub(super) fn config_path() -> PathBuf {
    std::env::var_os("URSINE_BTRBK_CONFIG")
        .map_or_else(|| PathBuf::from("/etc/btrbk/btrbk.conf"), PathBuf::from)
}

/// Run btrbk as root against the configured file.
pub(super) async fn run(args: &[&str], timeout: Duration) -> Result<String, ApiError> {
    let program = btrbk_path()?;
    let config = config_path();
    let config = config
        .to_str()
        .ok_or_else(|| ApiError::internal("btrbk config path is not UTF-8"))?;
    let mut command_args = Vec::with_capacity(args.len() + 2);
    command_args.push("-c");
    command_args.push(config);
    command_args.extend_from_slice(args);
    let out = exec::run_root_program(program, &command_args, timeout).await?;
    Ok(out.stdout)
}

/// Parse `key='value'` pairs from one btrbk raw line.
pub(super) fn parse_raw_line(line: &str) -> BTreeMap<String, String> {
    let mut map = BTreeMap::new();
    let mut rest = line.trim().to_string();
    while let Some(eq) = rest.find('=') {
        let key = rest.get(..eq).unwrap_or("").trim().to_string();
        if key.is_empty() {
            break;
        }
        rest = rest.get(eq + 1..).unwrap_or("").to_string();
        let Some(quote) = rest.chars().next().filter(|c| *c == '\'' || *c == '"') else {
            // Unquoted value: consume to the next space.
            let (value, tail) = rest
                .split_once(' ')
                .map_or((rest.as_str(), ""), |(v, t)| (v, t));
            map.insert(key, value.to_string());
            rest = tail.to_string();
            continue;
        };
        rest = rest.get(quote.len_utf8()..).unwrap_or("").to_string();
        let mut value = String::new();
        let mut chars = rest.chars().peekable();
        let mut closed = false;
        while let Some(ch) = chars.next() {
            if ch == quote {
                if quote == '\'' && chars.peek() == Some(&'\\') {
                    // btrbk quoteshell uses '\'' for a literal quote.
                    chars.next();
                    if chars.peek() == Some(&'\'') {
                        chars.next();
                    }
                    value.push('\'');
                } else {
                    closed = true;
                    break;
                }
            } else {
                value.push(ch);
            }
        }
        map.insert(key, value);
        rest = chars.collect::<String>().trim_start().to_string();
        if !closed {
            break;
        }
    }
    map
}

pub(super) fn parse_snapshots(raw: &str) -> Vec<Snapshot> {
    raw.lines()
        .map(parse_raw_line)
        .filter(|row| row.get("format").is_some_and(|f| f == "snapshots"))
        .filter_map(|row| {
            Some(Snapshot {
                source_subvolume: row.get("source_subvolume")?.clone(),
                snapshot_subvolume: row.get("snapshot_subvolume")?.clone(),
                snapshot_name: row.get("snapshot_name").cloned().filter(|v| !v.is_empty()),
                status: row.get("status").cloned().filter(|v| !v.is_empty()),
                source_url: row.get("source_url").cloned().filter(|v| !v.is_empty()),
            })
        })
        .collect()
}

pub(super) fn parse_stats(raw: &str) -> Vec<SnapshotStats> {
    raw.lines()
        .map(parse_raw_line)
        .filter(|row| row.get("format").is_some_and(|f| f == "stats"))
        .filter_map(|row| {
            Some(SnapshotStats {
                source_subvolume: row.get("source_subvolume")?.clone(),
                snapshot_subvolume: row
                    .get("snapshot_subvolume")
                    .cloned()
                    .filter(|v| !v.is_empty()),
                snapshot_status: row
                    .get("snapshot_status")
                    .cloned()
                    .filter(|v| !v.is_empty()),
                backup_status: row.get("backup_status").cloned().filter(|v| !v.is_empty()),
                snapshots: number(row.get("snapshots")),
                backups: number(row.get("backups")),
                correlated: number(row.get("correlated")),
                orphaned: number(row.get("orphaned")),
                incomplete: number(row.get("incomplete")),
            })
        })
        .collect()
}

fn number(value: Option<&String>) -> u64 {
    value.and_then(|v| v.parse().ok()).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SNAPSHOTS: &str = r#"format="snapshots" source_url='/home' source_host='' source_port='' source_subvolume='/home' snapshot_subvolume='/snapshots/home.20261022T1200' snapshot_name='home.20261022T1200' status='ok' source_rsh=''
format="snapshots" source_url='/var' source_host='' source_port='' source_subvolume='/var' snapshot_subvolume='/snapshots/var.20261022T1200' snapshot_name='var.20261022T1200' status='ok' source_rsh=''
"#;

    #[test]
    fn parses_snapshot_rows() {
        let snaps = parse_snapshots(SNAPSHOTS);
        assert_eq!(snaps.len(), 2);
        assert_eq!(snaps[0].source_subvolume, "/home");
        assert_eq!(snaps[1].snapshot_name.as_deref(), Some("var.20261022T1200"));
    }

    #[test]
    fn parses_stats_rows() {
        let raw = r#"format="stats" source_url='/home' source_host='' source_port='' source_subvolume='/home' snapshot_subvolume='/snapshots/home.20261022T1200' snapshot_name='home.20261022T1200' target_url='' target_host='' target_port='' target_subvolume='' snapshot_status='ok' backup_status='' snapshots='3' backups='0' correlated='0' orphaned='0' incomplete='0'
"#;
        let stats = parse_stats(raw);
        assert_eq!(stats.len(), 1);
        assert_eq!(stats[0].snapshots, 3);
        assert_eq!(stats[0].backups, 0);
    }

    #[test]
    fn parser_keeps_spaces_inside_quotes() {
        let row = parse_raw_line("format='x' path='/a b/c' status='ok'");
        assert_eq!(row.get("path").map(String::as_str), Some("/a b/c"));
        assert_eq!(row.get("status").map(String::as_str), Some("ok"));
    }
}
