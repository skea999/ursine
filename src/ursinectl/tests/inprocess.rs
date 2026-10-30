//! In-process integration: real daemon construction + real dispatch.
//! Runs as root inside the CI container (the same condition the CLI enforces).

#![allow(clippy::unwrap_used)] // tests

use ursine_api::{Action, ModuleId, ResponseBody};
use ursine_core::auth::PeerInfo;
use ursine_core::dispatch::Daemon;

fn root_peer() -> PeerInfo {
    PeerInfo {
        uid: 0,
        user: "root".to_string(),
    }
}

async fn call(
    daemon: &Daemon,
    token: &ursine_api::SessionToken,
    module: ModuleId,
    action: Action,
) -> Result<serde_json::Value, ursine_api::ApiError> {
    ursinectl::call(daemon, token, &root_peer(), module, action, None).await
}

/// Temp config path that does not exist → daemon defaults (audit under the
/// real `/var/log/ursine`, job logs under `/run/ursinectl/jobs`; both are
/// root-owned in the container).
fn build() -> (Daemon, ursine_api::SessionToken) {
    let daemon =
        ursinectl::daemon::build(std::path::Path::new("/nonexistent/ursine-test.toml")).unwrap();
    let token = ursinectl::issue_token(&daemon);
    (daemon, token)
}

#[tokio::test]
async fn status_and_capabilities_through_dispatch() {
    let (daemon, token) = build();
    let status = call(&daemon, &token, ModuleId::System, Action::Status)
        .await
        .unwrap();
    assert!(status.get("version").is_some(), "{status}");

    let caps = call(&daemon, &token, ModuleId::Capabilities, Action::List)
        .await
        .unwrap();
    assert!(caps.is_array(), "{caps}");
}

#[tokio::test]
async fn cli_action_builder_round_trips_through_dispatch() {
    let (daemon, token) = build();
    // Same path a CLI invocation takes: key=value → Action → dispatch.
    let action = ursinectl::cli::build_action("status", &[]).unwrap();
    let result = call(&daemon, &token, ModuleId::System, action)
        .await
        .unwrap();
    assert!(result.get("version").is_some(), "{result}");
}

#[tokio::test]
async fn crontab_get_maps_absence_or_content() {
    let (daemon, token) = build();
    let action = ursinectl::cli::build_action("crontab_get", &[]).unwrap();
    // Either the container root has a crontab or not; both are valid answers.
    match call(&daemon, &token, ModuleId::Crontab, action).await {
        Ok(view) => assert!(view.get("crontab").is_some(), "{view}"),
        Err(e) => {
            // Capability missing (no crontab tool) is the other valid answer.
            assert!(
                e.message.contains("crontab"),
                "unexpected crontab error: {e:?}"
            );
        }
    }
}

#[tokio::test]
async fn response_body_err_shape_is_preserved() {
    // An illegal (module, action) pair must come back as a typed error, the
    // same shape the GUI parses.
    let (daemon, token) = build();
    let err = call(&daemon, &token, ModuleId::Logs, Action::ListInstalled)
        .await
        .unwrap_err();
    assert!(!err.message.is_empty());
    // And the raw envelope path agrees.
    let req = ursine_api::Request {
        v: ursine_api::PROTOCOL_VERSION,
        id: Some(7),
        token: Some(token.clone()),
        body: ursine_api::RequestBody::Call {
            module: ModuleId::Logs,
            action: Action::ListInstalled,
            as_user: None,
        },
    };
    let resp = daemon.handle(req, &root_peer()).await;
    assert!(matches!(resp.body, ResponseBody::Err { .. }));
}
