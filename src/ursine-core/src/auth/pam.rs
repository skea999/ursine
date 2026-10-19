//! Real PAM authentication (feature `pam`, FeralOS builds).
//!
//! Uses the system PAM service `/etc/pam.d/ursine` (pam_unix). Password
//! values live only for the duration of this call — never logged, never
//! audited, never stored.
//!
//! Crate note: `pam` 0.7 (pam-sys 0.5, pre-generated bindings) — chosen over
//! 0.8 whose bindgen/clang build chain is hostile to musl containers.

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
        let mut auth = pam::Authenticator::with_password(PAM_SERVICE)
            .map_err(|e| AuthError::Internal(format!("pam init: {e}")))?;
        auth.get_handler().set_credentials(user, password);
        match auth.authenticate() {
            // Session left unopened: the daemon verifies identity, it does not
            // start a login session (veila-style auth-only PAM service).
            Ok(()) => Ok(Principal::local(user, local_groups(user))),
            // Any authentication failure collapses to Invalid: no oracle for
            // user enumeration. PAM-internal failures are rare here and land
            // in the same bucket by design (they are still audited upstream).
            Err(_) => Err(AuthError::Invalid),
        }
    }
}
