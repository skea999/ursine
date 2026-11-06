//! `BlueZ` pairing agent (`org.bluez.Agent1`).
//!
//! Answers `RequestPinCode`/`RequestPasskey` with a PIN the admin set
//! through `set_bluetooth_pairing_pin`. The value lives in RAM only and
//! is never written to logs or disk (project rule: session secrets stay
//! ephemeral). Registration happens lazily on the first `pair` that has
//! a PIN configured; the agent stays registered for the daemon's life.

use std::sync::atomic::{AtomicBool, Ordering};

use zbus::zvariant::OwnedObjectPath;

use ursine_api::ApiError;

/// Object path `BlueZ` calls back on.
pub(super) const AGENT_PATH: &str = "/org/ursine/bluetooth/agent1";
const CAPABILITY: &str = "KeyboardDisplay";

/// The pairing PIN/passkey (daemon RAM only).
static PIN: tokio::sync::RwLock<Option<String>> = tokio::sync::RwLock::const_new(None);
static REGISTERED: AtomicBool = AtomicBool::new(false);

/// Validate a PIN/passkey candidate: 1..=16 printable non-space ASCII
/// characters (legacy `BlueZ` PINs are that shape; passkeys are digits of
/// that length).
pub(super) fn validate_pin(pin: &str) -> Result<(), ApiError> {
    // `is_ascii_graphic` already excludes every whitespace character.
    if pin.is_empty() || pin.len() > 16 || !pin.chars().all(|c| c.is_ascii_graphic()) {
        return Err(ApiError::invalid(
            "invalid pairing PIN (1-16 printable ASCII characters, no spaces)",
        ));
    }
    Ok(())
}

/// Store the PIN (empty/`None` clears it) and make sure the agent is
/// registered with `BlueZ` while a PIN is active.
pub(super) async fn set_pin(pin: Option<String>) -> Result<bool, ApiError> {
    match pin {
        Some(pin) if !pin.is_empty() => {
            validate_pin(&pin)?;
            // Register first: a bluetoothd failure must not leave a
            // PIN stored that no agent will ever answer with.
            ensure_registered().await?;
            *PIN.write().await = Some(pin);
            Ok(true)
        }
        _ => {
            *PIN.write().await = None;
            unregister().await;
            Ok(false)
        }
    }
}

/// True when a PIN is currently configured.
pub(super) async fn pin_is_set() -> bool {
    PIN.read().await.is_some()
}

/// Export the agent on the system bus and register it with `BlueZ`.
/// Export the agent on the system bus and register it with `BlueZ`.
/// Serialized by a mutex: a failed `RegisterAgent` rolls the export
/// back so the next attempt starts clean.
static REGISTERING: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

pub(super) async fn ensure_registered() -> Result<(), ApiError> {
    if REGISTERED.load(Ordering::SeqCst) {
        return Ok(());
    }
    let _guard = REGISTERING.lock().await;
    if REGISTERED.load(Ordering::SeqCst) {
        return Ok(());
    }
    let conn = zbus::Connection::system().await.map_err(|error| {
        ApiError::unavailable(
            "system D-Bus is not available for the pairing agent",
            format!("start dbus: {error}"),
        )
    })?;
    conn.object_server()
        .at(AGENT_PATH, PairingAgent)
        .await
        .map_err(|error| ApiError::internal(format!("export pairing agent: {error}")))?;
    let manager = zbus::Proxy::new(&conn, "org.bluez", "/org/bluez", "org.bluez.AgentManager1")
        .await
        .map_err(|error| ApiError::unavailable("bluetoothd is not running", format!("{error}")))?;
    if let Err(error) = manager
        .call::<_, _, ()>("RegisterAgent", &(AGENT_PATH, CAPABILITY))
        .await
    {
        // Undo the export so a retry does not hit AlreadyExists.
        let _ = conn
            .object_server()
            .remove::<PairingAgent, _>(AGENT_PATH)
            .await;
        return Err(ApiError::internal(format!("RegisterAgent: {error}")));
    }
    // Also make it the default so incoming pairings reach us.
    let _ = manager
        .call::<_, _, ()>("SetDefaultAgent", &(AGENT_PATH,))
        .await;
    REGISTERED.store(true, Ordering::SeqCst);
    Ok(())
}

/// Unregister the agent and drop the export (PIN cleared): pairings
/// fall back to bluetoothctl's own per-call agent.
pub(super) async fn unregister() {
    if !REGISTERED.swap(false, Ordering::SeqCst) {
        return;
    }
    if let Ok(conn) = zbus::Connection::system().await {
        if let Ok(manager) =
            zbus::Proxy::new(&conn, "org.bluez", "/org/bluez", "org.bluez.AgentManager1").await
        {
            let _ = manager
                .call::<_, _, ()>("UnregisterAgent", &(AGENT_PATH,))
                .await;
        }
        let _ = conn
            .object_server()
            .remove::<PairingAgent, _>(AGENT_PATH)
            .await;
    }
}

async fn current_pin() -> Result<String, zbus::fdo::Error> {
    PIN.read()
        .await
        .clone()
        .ok_or_else(|| zbus::fdo::Error::Failed("no pairing PIN configured in Ursine".into()))
}

/// `BlueZ` `org.bluez.Agent1`.
struct PairingAgent;

#[zbus::interface(name = "org.bluez.Agent1")]
// The D-Bus trait dictates the method shapes: several callbacks are
// intentionally empty and the macro references the parameters.
#[allow(
    clippy::unused_async,
    clippy::unused_self,
    clippy::unnecessary_wraps,
    clippy::used_underscore_binding
)]
impl PairingAgent {
    async fn request_pin_code(&self) -> zbus::fdo::Result<String> {
        current_pin().await
    }

    async fn request_passkey(&self) -> zbus::fdo::Result<u32> {
        let pin = current_pin().await?;
        pin.parse::<u32>().map_err(|_| {
            zbus::fdo::Error::Failed("the configured pairing PIN is not numeric".into())
        })
    }

    /// Numeric comparison against the configured PIN. `None` means the
    /// PIN was cleared while we were still registered (the clear path
    /// unregisters; this is the race window): deny, never auto-accept.
    async fn request_confirmation(&self, passkey: u32) -> zbus::fdo::Result<()> {
        match PIN.read().await.as_deref() {
            None => Err(zbus::fdo::Error::Failed("no pairing PIN configured".into())),
            // Parse both sides so a leading-zero PIN ("0042") matches
            // the numeric passkey 42.
            Some(pin) if pin.parse::<u32>() == Ok(passkey) => Ok(()),
            Some(_) => Err(zbus::fdo::Error::Failed(
                "passkey does not match the configured pairing PIN".into(),
            )),
        }
    }

    /// Display-only methods: the device shows the number itself; we
    /// deliberately never log it.
    fn display_passkey(&self, passkey: u32, entered: u16) {
        let _ = (passkey, entered);
    }

    fn display_pin_code(&self, pin: String) {
        drop(pin);
    }

    fn request_authorization(&self, device: OwnedObjectPath) -> zbus::fdo::Result<()> {
        drop(device);
        Ok(())
    }

    fn authorize_service(&self, device: OwnedObjectPath, uuid: String) -> zbus::fdo::Result<()> {
        drop((device, uuid));
        Ok(())
    }

    fn cancel(&self) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_pins() {
        assert!(validate_pin("1234").is_ok());
        assert!(validate_pin("0000").is_ok());
        assert!(validate_pin("abc-def_9").is_ok());
        assert!(validate_pin("").is_err());
        assert!(validate_pin("12345678901234567").is_err()); // 17 chars
        assert!(validate_pin("has space").is_err());
        // Quotes are legal: zbus escapes them on the wire.
        assert!(validate_pin("bad\"quote").is_ok());
        assert!(validate_pin("tab\there").is_err());
    }
}
