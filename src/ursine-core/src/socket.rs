//! Unix socket server: newline-delimited JSON, peer credentials, 0666 socket.

use std::os::unix::fs::{FileTypeExt, PermissionsExt};
use std::path::Path;
use std::sync::Arc;

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{UnixListener, UnixStream};

use crate::dispatch::Daemon;
use crate::error::CoreError;

/// Serve until `shutdown` resolves. One task per connection.
pub async fn serve<F>(daemon: Arc<Daemon>, path: &Path, shutdown: F) -> Result<(), CoreError>
where
    F: std::future::Future<Output = ()> + Send,
{
    // Stale socket from a previous run: remove (only if it IS a socket).
    if path.exists() {
        let meta = std::fs::metadata(path)?;
        if meta.file_type().is_socket() {
            std::fs::remove_file(path)?;
        } else {
            return Err(CoreError::socket(format!(
                "{} exists and is not a socket — refusing to remove",
                path.display()
            )));
        }
    }
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }

    let listener = UnixListener::bind(path)?;
    // Any local user may CONNECT; the PAM handshake is the gate (policy).
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o666))?;
    tracing::info!(socket = %path.display(), "listening");

    tokio::pin!(shutdown);
    loop {
        tokio::select! {
            accepted = listener.accept() => {
                match accepted {
                    Ok((stream, _addr)) => {
                        let d = daemon.clone();
                        tokio::spawn(async move {
                            if let Err(e) = handle_connection(d, stream).await {
                                tracing::debug!(error = %e, "connection ended with error");
                            }
                        });
                    }
                    Err(e) => tracing::warn!(error = %e, "accept failed"),
                }
            }
            _ = &mut shutdown => {
                tracing::info!("shutdown requested");
                break;
            }
        }
    }
    Ok(())
}

async fn handle_connection(daemon: Arc<Daemon>, stream: UnixStream) -> Result<(), CoreError> {
    let cred = stream
        .peer_cred()
        .map_err(|e| CoreError::socket(format!("peer_cred: {e}")))?;
    let peer = crate::auth::PeerInfo {
        uid: cred.uid(),
        user: crate::auth::user_name_for_uid(cred.uid())
            .unwrap_or_else(|| format!("uid:{}", cred.uid())),
    };
    tracing::debug!(uid = peer.uid, user = %peer.user, "client connected");

    let (read_half, mut write_half) = stream.into_split();
    let mut lines = BufReader::new(read_half).lines();

    while let Some(line) = lines.next_line().await.map_err(CoreError::Io)? {
        if line.trim().is_empty() {
            continue;
        }
        let response = match serde_json::from_str::<ursine_api::Request>(&line) {
            Ok(req) => daemon.handle(req, &peer),
            Err(e) => ursine_api::Response::err(
                None,
                ursine_api::ApiError::invalid(format!("bad request line: {e}")),
            ),
        };
        let mut out = serde_json::to_string(&response)
            .map_err(|e| CoreError::socket(format!("serialize response: {e}")))?;
        out.push('\n');
        write_half
            .write_all(out.as_bytes())
            .await
            .map_err(CoreError::Io)?;
        write_half.flush().await.map_err(CoreError::Io)?;
    }
    Ok(())
}
