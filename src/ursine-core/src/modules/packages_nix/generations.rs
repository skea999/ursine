//! Profile generations: listing and prune planning (keep last N).
//!
//! Real `nix profile history` (2.31) emits ANSI styling even on pipes and does
//! NOT mark the current generation — the active generation is read from the
//! `profile-N-link` symlink instead.
//!
//! `nix profile` has no "keep last N" switch, so the plan is computed here and
//! executed with the GC-subsystem switch `nix-env -p <profile>
//! --delete-generations +N` (as root: in multi-user mode the profile lives in
//! the store, users cannot lock/write it directly).

/// Strip ANSI SGR sequences (`ESC [ … m`) emitted by nix.
fn strip_ansi(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' && matches!(chars.peek(), Some('[')) {
            chars.next();
            for inner in chars.by_ref() {
                if inner.is_ascii_alphabetic() {
                    break;
                }
            }
            continue;
        }
        out.push(c);
    }
    out
}

/// Parse generation numbers from `nix profile history` output.
/// Lines look like: `Version 33 (2026-10-14):` or `Version 34 (…) <- 33:`.
pub fn parse_history(text: &str) -> Vec<u32> {
    let mut out = Vec::new();
    for raw in text.lines() {
        let line = strip_ansi(raw);
        let line = line.trim();
        let Some(rest) = line.strip_prefix("Version ") else {
            continue;
        };
        let Some(num) = rest.split_whitespace().next() else {
            continue;
        };
        if let Ok(number) = num.parse::<u32>() {
            out.push(number);
        }
    }
    out.sort_unstable();
    out.dedup();
    out
}

/// Current generation from a symlink target like `profile-34-link`.
pub fn parse_generation_link(target: &str) -> Option<u32> {
    let name = target.rsplit('/').next()?;
    let rest = name.strip_prefix("profile-")?;
    let digits = rest.strip_suffix("-link")?;
    digits.parse().ok()
}

/// Which generations would be deleted keeping the newest `keep` (1..=1000).
/// The active generation is never included.
pub fn prune_plan(generations: &[u32], current: Option<u32>, keep: u32) -> Result<Vec<u32>, u32> {
    if !(1..=1000).contains(&keep) {
        return Err(keep);
    }
    if generations.is_empty() {
        return Ok(Vec::new());
    }
    let keep_n = (keep as usize).min(generations.len());
    let cutoff = generations.len() - keep_n;
    Ok(generations[..cutoff]
        .iter()
        .copied()
        .filter(|g| Some(*g) != current)
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    // Real output sample from nix 2.31.5 (ANSI escapes included).
    const HISTORY: &str = "Version \u{1b}[1m33\u{1b}[0m (2026-10-14):\n  flake:nixpkgs#hello: ∅ -> 2.12\n\nVersion \u{1b}[32;1m34\u{1b}[0m (2026-10-16) <- 33:\n  flake:nixpkgs#gh: ∅ -> 2.102.0\n";

    #[test]
    fn strips_ansi_and_parses_numbers() {
        assert_eq!(parse_history(HISTORY), vec![33, 34]);
    }

    #[test]
    fn parses_rollback_arrow_lines() {
        let text = "Version 5 (2025-01-01) <- 3:\nVersion 4 (2024-12-01):\n";
        assert_eq!(parse_history(text), vec![4, 5]);
    }

    #[test]
    fn parses_generation_symlink() {
        assert_eq!(
            parse_generation_link("/nix/var/nix/profiles/per-user/u/profile-34-link"),
            Some(34)
        );
        assert_eq!(parse_generation_link("profile-2-link"), Some(2));
        assert_eq!(parse_generation_link("profile-link"), None);
        assert_eq!(parse_generation_link("nonsense"), None);
    }

    #[test]
    fn keeps_last_n_newest() {
        let gens = vec![33, 34, 35];
        let plan = prune_plan(&gens, Some(35), 2).unwrap();
        assert_eq!(plan, vec![33], "keep 34,35 (35 is current)");
    }

    #[test]
    fn never_deletes_current_even_if_old() {
        let gens = vec![33, 34, 35];
        // current is the oldest (rollback case)
        let plan = prune_plan(&gens, Some(33), 1).unwrap();
        assert_eq!(
            plan,
            vec![34],
            "keep 35; 33 is current -> survives; 34 deleted"
        );
        assert!(!plan.contains(&33));
    }

    #[test]
    fn keep_larger_than_history_is_noop() {
        assert!(prune_plan(&[1, 2], Some(2), 10).unwrap().is_empty());
    }

    #[test]
    fn invalid_keep_is_rejected() {
        assert!(prune_plan(&[1], Some(1), 0).is_err());
        assert!(prune_plan(&[1], Some(1), 1001).is_err());
    }

    #[test]
    fn empty_history_is_fine() {
        assert!(parse_history("").is_empty());
        assert!(prune_plan(&[], None, 3).unwrap().is_empty());
    }
}
