//! Authentication: the ONE seam where identity is verified.
//!
//! Desktop/local flow today is a PAM check of user+password; in v3 an OIDC
//! implementation joins behind the same trait without touching the API.

pub mod stub;

#[cfg(feature = "pam")]
pub mod pam;

use ursine_api::Principal;

/// What the daemon knows about the peer before auth.
#[derive(Debug, Clone)]
pub struct PeerInfo {
    /// uid from `SO_PEERCRED` (kernel-provided, not client-claimed).
    pub uid: u32,
    /// Resolved user name for that uid.
    pub user: String,
}

/// Authentication outcome failures (mapped to wire errors by dispatch).
#[derive(Debug, thiserror::Error)]
pub enum AuthError {
    /// Wrong credentials or unknown user — indistinguishable on purpose.
    #[error("invalid credentials")]
    Invalid,
    /// Authenticator itself failed (PAM unavailable, ...).
    #[error("authenticator unavailable: {0}")]
    Internal(String),
}

/// Verify credentials and resolve a principal.
pub trait Authenticator: Send + Sync {
    fn authenticate(
        &self,
        peer: &PeerInfo,
        user: &str,
        password: &str,
    ) -> Result<Principal, AuthError>;
}

/// Group memberships for a local user, read from `/etc/group` (read-only).
pub fn local_groups(user: &str) -> Vec<String> {
    let Ok(raw) = std::fs::read_to_string("/etc/group") else {
        return Vec::new();
    };
    let mut groups = Vec::new();
    for line in raw.lines() {
        let mut parts = line.split(':');
        let (Some(name), Some(_), Some(_), Some(members)) =
            (parts.next(), parts.next(), parts.next(), parts.next())
        else {
            continue;
        };
        if members.split(',').any(|m| m == user) {
            groups.push(name.to_string());
        }
    }
    groups
}

/// Resolve a uid to a user name via `/etc/passwd` (read-only).
pub fn user_name_for_uid(uid: u32) -> Option<String> {
    let raw = std::fs::read_to_string("/etc/passwd").ok()?;
    for line in raw.lines() {
        let mut parts = line.split(':');
        let (Some(name), Some(_), Some(uid_field)) = (parts.next(), parts.next(), parts.next())
        else {
            continue;
        };
        if uid_field.parse::<u32>().ok() == Some(uid) {
            return Some(name.to_string());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_groups_never_panics_and_returns_vec() {
        // Runs on any machine; content varies but must not panic.
        let groups = local_groups("definitely-not-a-user");
        assert!(groups.is_empty());
    }

    #[test]
    fn uid_lookup_resolves_current_user_where_possible() {
        // uid 0 always exists in /etc/passwd on unix; tolerate odd containers.
        if let Some(name) = user_name_for_uid(0) {
            assert_eq!(name, "root");
        }
        assert!(user_name_for_uid(u32::MAX - 1).is_none());
    }
}
