//! Real PAM authentication (feature `pam`, FeralOS builds).
//!
//! Uses the system PAM service `/etc/pam.d/ursine` (pam_unix) through
//! `pam-client` 0.2 (maintained fork of `pam` 0.7): pam-sys 0.5 with
//! pregenerated bindings (no bindgen/clang) and no vulnerable `users`
//! dependency. Password values live only for the duration of this call —
//! never logged, never audited, never stored.
//!
//! Session left unopened: the daemon verifies identity, it does not start a
//! login session (same auth-only pattern as the Veila locker).

use pam_client::conv_mock::Conversation;
use pam_client::{Context, Flag};

use super::{local_groups, AuthError, Authenticator, PeerInfo};
use ursine_api::Principal;

/// PAM service name; the file `/etc/pam.d/ursine` ships with the package.
const PAM_SERVICE: &str = "ursine";

/// Authenticates against linux-pam.
pub struct PamAuthenticator;

impl Authenticator for PamAuthenticator {
    fn authenticate(
        &self,
        _peer: &PeerInfo,
        user: &str,
        password: &str,
    ) -> Result<Principal, AuthError> {
        let conv = Conversation::with_credentials(user, password);
        let mut ctx = Context::new(PAM_SERVICE, Some(user), conv)
            .map_err(|e| AuthError::Internal(format!("pam init: {e}")))?;
        match ctx.authenticate(Flag::NONE) {
            Ok(()) => Ok(Principal::local(user, local_groups(user))),
            // Any authentication failure collapses to Invalid: no oracle for
            // user enumeration.
            Err(_) => Err(AuthError::Invalid),
        }
    }
}
