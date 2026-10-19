//! Dev-only authenticator: accepts anything, logs loudly, NEVER ships.
//!
//! Used when `ursined` is built without the `pam` feature (dev machines
//! without libpam). The wheel group is granted so the GUI flow is testable;
//! every successful stub login emits a WARN.

use tracing::warn;

use super::{local_groups, AuthError, Authenticator, PeerInfo};
use ursine_api::Principal;

/// Accept-everything authenticator for development builds.
pub struct StubAuthenticator;

impl Authenticator for StubAuthenticator {
    fn authenticate(
        &self,
        peer: &PeerInfo,
        user: &str,
        password: &str,
    ) -> Result<Principal, AuthError> {
        warn!(
            peer_uid = peer.uid,
            user, "STUB AUTH: accepting credentials (dev build, no PAM)"
        );
        if user.is_empty() || password.is_empty() {
            return Err(AuthError::Invalid);
        }
        let mut groups = local_groups(user);
        if !groups.iter().any(|g| g == "wheel") {
            groups.push("wheel".to_string());
        }
        Ok(Principal::local(user, groups))
    }
}
