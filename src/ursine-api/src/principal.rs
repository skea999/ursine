//! Authenticated identity of a requester.

use serde::{Deserialize, Serialize};

/// Where the identity came from. `Oidc` is reserved for the v3 web flow.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PrincipalOrigin {
    /// Verified locally through the PAM handshake.
    Local,
    /// Verified through OIDC (v3; no code path yet).
    Oidc,
}

/// Who is acting. Recorded in every audit line.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Principal {
    pub origin: PrincipalOrigin,
    /// Local unix user name, or OIDC `sub` in v3.
    pub user: String,
    /// Group memberships at authentication time (e.g. `["wheel"]`).
    pub groups: Vec<String>,
}

impl Principal {
    pub fn local(user: impl Into<String>, groups: Vec<String>) -> Self {
        Self {
            origin: PrincipalOrigin::Local,
            user: user.into(),
            groups,
        }
    }

    /// Does this principal carry the given group?
    pub fn in_group(&self, group: &str) -> bool {
        self.groups.iter().any(|g| g == group)
    }
}

/// Target user for per-user operations (`flatpak --user`, `nix profile`).
/// Defaults to the authenticated principal when absent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct AsUser(pub String);

impl AsUser {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let p = Principal::local("skea999", vec!["wheel".into()]);
        let s = serde_json::to_string(&p).unwrap();
        let back: Principal = serde_json::from_str(&s).unwrap();
        assert_eq!(p, back);
        assert!(p.in_group("wheel"));
        assert!(!p.in_group("users"));
    }

    #[test]
    fn as_user_is_transparent() {
        let a = AsUser("skea999".into());
        let s = serde_json::to_string(&a).unwrap();
        assert_eq!(s, "\"skea999\"");
    }
}
