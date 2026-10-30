//! Hard root gate: `ursinectl` never runs as non-root.
//!
//! The effective uid is read from `/proc/self/status` (no libc, no `unsafe`,
//! works in a chroot where `/proc` is mounted — `FeralOS` install media mounts
//! it; without `/proc` the gate fails closed).

/// Refuse to run unless the effective uid is 0. Fails closed when the
/// status file is missing or unparseable.
pub fn ensure_root() -> Result<u32, String> {
    let status = std::fs::read_to_string("/proc/self/status")
        .map_err(|e| format!("cannot read /proc/self/status: {e}"))?;
    let uid = effective_uid(&status)
        .ok_or_else(|| "cannot parse Uid from /proc/self/status".to_string())?;
    if uid != 0 {
        return Err("ursinectl must run as root (use sudo)".to_string());
    }
    Ok(uid)
}

/// Second field of the `Uid:` line is the effective uid.
fn effective_uid(status: &str) -> Option<u32> {
    let line = status.lines().find(|l| l.starts_with("Uid:"))?;
    let mut fields = line.split_whitespace().skip(1);
    let _real = fields.next()?;
    fields.next()?.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_effective_uid() {
        let status = "Name:\tx\nUid:\t0\t0\t0\t0\n";
        assert_eq!(effective_uid(status), Some(0));
        let status = "Name:\tx\nUid:\t1000\t1000\t1000\t1000\n";
        assert_eq!(effective_uid(status), Some(1000));
    }

    #[test]
    fn missing_uid_line_is_none() {
        assert_eq!(effective_uid("Name:\tx\n"), None);
    }
}
