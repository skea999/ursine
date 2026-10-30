//! Crontab management types (SPEC-crontab): view + replace of the target
//! user's crontab.

use serde::{Deserialize, Serialize};

/// Current crontab for the session user.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CrontabView {
    /// `None` = no crontab exists yet for this user.
    pub crontab: Option<String>,
}

/// Maximum accepted crontab payload (bytes) on `crontab_set`.
pub fn max_crontab_bytes() -> usize {
    256 * 1024
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn view_roundtrips_and_absent_means_none() {
        let empty = CrontabView { crontab: None };
        let s = serde_json::to_string(&empty).unwrap();
        assert_eq!(serde_json::from_str::<CrontabView>(&s).unwrap(), empty);
        let full = CrontabView {
            crontab: Some("* * * * * true\n".into()),
        };
        let s = serde_json::to_string(&full).unwrap();
        assert!(serde_json::from_str::<CrontabView>(&s)
            .unwrap()
            .crontab
            .is_some());
    }

    #[test]
    fn size_cap_is_256kib() {
        assert_eq!(max_crontab_bytes(), 256 * 1024);
    }
}
