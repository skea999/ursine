//! Unix socket server: newline-delimited JSON, peer credentials, 0666 socket.

use std::collections::HashMap;
use std::os::unix::fs::{FileTypeExt, PermissionsExt};
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::Semaphore;
use ursine_api::{ApiError, Response};

use crate::dispatch::Daemon;
use crate::error::CoreError;

/// Concurrent connections accepted from the kernel; extra clients are dropped.
const MAX_CONNECTIONS: usize = 64;
/// One local user cannot hold every connection permit: leave room for others.
const MAX_CONNECTIONS_PER_UID: usize = 8;
/// One request line must fit in this many bytes.
const MAX_REQUEST_BYTES: usize = 1024 * 1024;
/// A connected client must send its next request within five minutes.
const IDLE_TIMEOUT: Duration = Duration::from_secs(300);
/// The first request must arrive quickly: a connection is not an idle session.
const FIRST_REQUEST_TIMEOUT: Duration = Duration::from_secs(10);
/// A non-reading client must not hold a semaphore permit forever.
const WRITE_TIMEOUT: Duration = Duration::from_secs(30);

/// Counts one accepted connection against its peer uid, releasing on drop.
struct UidSlot {
    uid: u32,
    counts: Arc<Mutex<HashMap<u32, usize>>>,
}

impl UidSlot {
    fn acquire(counts: Arc<Mutex<HashMap<u32, usize>>>, uid: u32) -> Option<Self> {
        let mut map = counts
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let count = map.entry(uid).or_insert(0);
        if *count >= MAX_CONNECTIONS_PER_UID {
            return None;
        }
        *count += 1;
        drop(map);
        Some(Self { uid, counts })
    }
}

impl Drop for UidSlot {
    fn drop(&mut self) {
        let mut map = self
            .counts
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(count) = map.get_mut(&self.uid) {
            *count = count.saturating_sub(1);
            if *count == 0 {
                map.remove(&self.uid);
            }
        }
    }
}

/// Best-effort protocol error so a rejected client sees why, not EOF.
fn send_busy(stream: &UnixStream) {
    let Ok(mut line) = serde_json::to_vec(&Response::err(
        None,
        ApiError::internal("daemon busy; retry shortly"),
    )) else {
        return;
    };
    line.push(b'\n');
    let _ = stream.try_write(&line);
}

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

    let permits = Arc::new(Semaphore::new(MAX_CONNECTIONS));
    let uid_counts: Arc<Mutex<HashMap<u32, usize>>> = Arc::new(Mutex::new(HashMap::new()));
    tokio::pin!(shutdown);
    loop {
        tokio::select! {
            accepted = listener.accept() => {
                match accepted {
                    Ok((stream, _addr)) => {
                        let uid = match stream.peer_cred() {
                            Ok(cred) => cred.uid(),
                            Err(e) => {
                                tracing::warn!(error = %e, "peer credentials unavailable; dropping client");
                                continue;
                            }
                        };
                        let Some(slot) = UidSlot::acquire(uid_counts.clone(), uid) else {
                            tracing::warn!(
                                uid,
                                limit = MAX_CONNECTIONS_PER_UID,
                                "per-user connection limit reached; rejecting client"
                            );
                            send_busy(&stream);
                            continue;
                        };
                        if let Ok(permit) = permits.clone().try_acquire_owned() {
                            let d = daemon.clone();
                            tokio::spawn(async move {
                                // Permit and per-uid slot live as long as
                                // the connection task.
                                let _permit = permit;
                                let _slot = slot;
                                if let Err(e) = handle_connection(d, stream).await {
                                    tracing::debug!(error = %e, "connection ended with error");
                                }
                            });
                        } else {
                            tracing::warn!(
                                limit = MAX_CONNECTIONS,
                                "connection limit reached; rejecting client"
                            );
                            send_busy(&stream);
                        }
                    }
                    Err(e) => tracing::warn!(error = %e, "accept failed"),
                }
            }
            () = &mut shutdown => {
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
    let mut reader = BufReader::new(read_half);

    let mut first_request = true;
    loop {
        let read_timeout = if first_request {
            FIRST_REQUEST_TIMEOUT
        } else {
            IDLE_TIMEOUT
        };
        let line = match tokio::time::timeout(
            read_timeout,
            read_request_line(&mut reader, MAX_REQUEST_BYTES),
        )
        .await
        {
            Err(_) => {
                tracing::debug!(uid = peer.uid, "request read timed out; closing connection");
                break;
            }
            Ok(Err(e)) => return Err(CoreError::Io(e)),
            Ok(Ok(RequestLine::Eof)) => break,
            Ok(Ok(RequestLine::TooLong)) => {
                let response = Response::err(
                    None,
                    ApiError::invalid(format!("request line exceeds {MAX_REQUEST_BYTES} bytes")),
                );
                write_response(&mut write_half, &response).await?;
                break;
            }
            Ok(Ok(RequestLine::Line(line))) => line,
        };
        first_request = false;
        if line.iter().all(u8::is_ascii_whitespace) {
            // A blank line is not a keep-alive: answer and close, so it cannot
            // reset the idle window without doing any work.
            let response = Response::err(None, ApiError::invalid("empty request line"));
            write_response(&mut write_half, &response).await?;
            break;
        }
        let response = match serde_json::from_slice::<ursine_api::Request>(&line) {
            Ok(req) => daemon.handle(req, &peer).await,
            Err(e) => Response::err(None, ApiError::invalid(format!("bad request line: {e}"))),
        };
        write_response(&mut write_half, &response).await?;
    }
    Ok(())
}

/// One newline-terminated request line, capped while it grows.
enum RequestLine {
    Line(Vec<u8>),
    Eof,
    TooLong,
}

async fn read_request_line<R: AsyncBufRead + Unpin>(
    reader: &mut R,
    max_bytes: usize,
) -> std::io::Result<RequestLine> {
    let mut line = Vec::new();
    loop {
        let buf = reader.fill_buf().await?;
        if buf.is_empty() {
            return if line.is_empty() {
                Ok(RequestLine::Eof)
            } else {
                Ok(RequestLine::Line(line))
            };
        }
        if let Some(newline) = buf.iter().position(|byte| *byte == b'\n') {
            if line.len() + newline > max_bytes {
                return Ok(RequestLine::TooLong);
            }
            line.extend_from_slice(buf.get(..newline).unwrap_or_default());
            reader.consume(newline + 1);
            return Ok(RequestLine::Line(line));
        }
        let take = buf.len();
        if line.len() + take > max_bytes {
            return Ok(RequestLine::TooLong);
        }
        line.extend_from_slice(buf);
        reader.consume(take);
    }
}

async fn write_response<W: AsyncWrite + Unpin>(
    writer: &mut W,
    response: &Response,
) -> Result<(), CoreError> {
    let mut out = serde_json::to_string(response)
        .map_err(|e| CoreError::socket(format!("serialize response: {e}")))?;
    out.push('\n');
    let write = async {
        writer
            .write_all(out.as_bytes())
            .await
            .map_err(CoreError::Io)?;
        writer.flush().await.map_err(CoreError::Io)
    };
    tokio::time::timeout(WRITE_TIMEOUT, write)
        .await
        .map_err(|_| CoreError::socket("response write timed out"))?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn reads_one_line_at_a_time() {
        let mut reader = BufReader::new(&b"hello\nworld\n"[..]);
        match read_request_line(&mut reader, 32).await.unwrap() {
            RequestLine::Line(line) => assert_eq!(line, b"hello"),
            _ => panic!("want a line"),
        }
        match read_request_line(&mut reader, 32).await.unwrap() {
            RequestLine::Line(line) => assert_eq!(line, b"world"),
            _ => panic!("want a line"),
        }
        assert!(matches!(
            read_request_line(&mut reader, 32).await.unwrap(),
            RequestLine::Eof
        ));
    }

    #[tokio::test]
    async fn rejects_a_line_over_the_limit() {
        let mut reader = BufReader::new(&b"0123456789abcdef\n"[..]);
        assert!(matches!(
            read_request_line(&mut reader, 8).await.unwrap(),
            RequestLine::TooLong
        ));
    }

    #[test]
    fn per_uid_slots_are_capped_and_released() {
        let counts = Arc::new(Mutex::new(HashMap::new()));
        let slots: Vec<UidSlot> = (0..MAX_CONNECTIONS_PER_UID)
            .map(|_| UidSlot::acquire(counts.clone(), 1000).expect("within limit"))
            .collect();
        assert!(
            UidSlot::acquire(counts.clone(), 1000).is_none(),
            "over the per-uid cap"
        );
        assert!(
            UidSlot::acquire(counts.clone(), 1001).is_some(),
            "other uids unaffected"
        );
        drop(slots);
        assert!(
            UidSlot::acquire(counts, 1000).is_some(),
            "released slots are reusable"
        );
    }

    #[tokio::test]
    async fn reports_eof_without_data() {
        let mut reader = BufReader::new(&b""[..]);
        assert!(matches!(
            read_request_line(&mut reader, 8).await.unwrap(),
            RequestLine::Eof
        ));
    }
}
