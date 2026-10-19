//! Profile generations: listing and prune planning (keep last N).
//!
//! `nix profile` has no "keep last N" switch, so the plan is computed from
//! `nix profile history` output and executed with the generation-deletion
//! switch of the GC subsystem (`nix-env --delete-generations +N`). Package
//! operations stay new-CLI-only; this exception is documented in the spec.

use ursine_api::ApiError;

/// One profile generation as reported by `nix profile history`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Generation {
    pub number: u32,
    pub current: bool,
}

/// Parse `nix profile history` human output.
///
/// Lines look like:
/// `Version 3 (2025-01-01)` / `Version 1 (2024-11-01) (current)`.
pub fn parse_history(text: &str) -> Vec<Generation> {
    let mut out = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        let Some(rest) = line.strip_prefix("Version ") else {
            continue;
        };
        let mut parts = rest.split_whitespace();
        let Some(num) = parts.next() else { continue };
        let Ok(number) = num.parse::<u32>() else {
            continue;
        };
        out.push(Generation {
            number,
            current: line.contains("(current)"),
        });
    }
    out.sort_by_key(|g| g.number);
    out
}

/// Which generations would be deleted keeping the newest `keep` (1..=1000).
pub fn prune_plan(generations: &[Generation], keep: u32) -> Result<Vec<Generation>, ApiError> {
    if !(1..=1000).contains(&keep) {
        return Err(ApiError::invalid("keep must be between 1 and 1000"));
    }
    if generations.is_empty() {
        return Ok(Vec::new());
    }
    let keep = keep.min(generations.len() as u32) as usize;
    let cutoff = generations.len() - keep;
    Ok(generations[..cutoff]
        .iter()
        .filter(|g| !g.current) // never delete the active generation
        .copied()
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    const HISTORY: &str = "\
Version 4 (2025-02-01)
+ hello-2.12.1
Version 3 (2025-01-01)
+ jq-1.7.1
Version 2 (2024-12-01)
Version 1 (2024-11-01) (current)
";

    #[test]
    fn parses_versions_and_current() {
        let gens = parse_history(HISTORY);
        assert_eq!(gens.len(), 4);
        assert_eq!(gens[0].number, 1);
        assert!(gens[0].current);
        assert!(!gens[3].current);
    }

    #[test]
    fn keeps_last_n_newest() {
        let gens = parse_history(HISTORY); // numbers 1..4, 1 is current
        let plan = prune_plan(&gens, 2).unwrap();
        // keep 2 newest (3,4); 1 is active so it survives even if old -> [2].
        assert_eq!(plan.iter().map(|g| g.number).collect::<Vec<_>>(), vec![2]);
    }

    #[test]
    fn never_deletes_current_even_if_old() {
        let gens = parse_history(HISTORY);
        let plan = prune_plan(&gens, 1).unwrap();
        // newest = 4; older = 2,3 (1 is current and stays, but not in plan anyway)
        // keep 1 newest (4); old gens 2,3 are deleted; 1 is active -> survives.
        assert_eq!(
            plan.iter().map(|g| g.number).collect::<Vec<_>>(),
            vec![2, 3]
        );
        assert!(!plan.iter().any(|g| g.current));
    }

    #[test]
    fn keep_larger_than_history_is_noop() {
        let gens = parse_history(HISTORY);
        assert!(prune_plan(&gens, 10).unwrap().is_empty());
    }

    #[test]
    fn invalid_keep_is_rejected() {
        let gens = parse_history(HISTORY);
        assert!(prune_plan(&gens, 0).is_err());
        assert!(prune_plan(&gens, 1001).is_err());
    }

    #[test]
    fn empty_history_is_fine() {
        assert!(parse_history("").is_empty());
        assert!(prune_plan(&[], 3).unwrap().is_empty());
    }
}
