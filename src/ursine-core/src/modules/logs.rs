//! Log viewer backend (SPEC-logs): dynamic `/var/log` discovery and
//! cursor-based plain-text chunk reads (tail / previous / follow).
//!
//! Reads only — no mutation — so `LogRead` is not audited (high-frequency
//! follow polls; the same precedent as `job_output` in dispatch).

use std::io::{BufRead, Read, Seek, SeekFrom};
use std::path::{Component, Path, PathBuf};
use std::time::UNIX_EPOCH;

use ursine_api::{
    Action, ApiError, ErrorCode, LogChunk, LogCursor, LogFile, LogHit, LogSearchResult,
};

/// The only root that may be read (SPEC-logs security).
const LOG_ROOT: &str = "/var/log";
/// Bytes read per call (keeps one line's worth of history cheap).
const READ_CAP: u64 = 512 * 1024;
/// Never tail compressed/rotated archives as text.
const COMPRESSED: [&str; 5] = [".gz", ".xz", ".bz2", ".zip", ".zst"];
/// Max directory depth under `/var/log` (cups/, samba/, …).
const MAX_DEPTH: u32 = 3;
/// Hard clamp for one search query.
const MAX_SEARCH_LIMIT: u32 = 500;
/// Bytes scanned per search before the result is marked truncated.
const SEARCH_SCAN_CAP: u64 = 4 * 1024 * 1024;
/// Preview cap for one hit line.
const HIT_LINE_CHARS: usize = 400;

/// Route a logs action (blocking work runs off the async executor).
pub async fn handle(
    action: &Action,
    _ctx: &crate::modules::ModuleCtx<'_>,
) -> Result<serde_json::Value, ApiError> {
    match action {
        Action::LogList => {
            let list = blocking(list_logs).await?;
            serde_json::to_value(list)
                .map_err(|e| ApiError::internal(format!("encode log list: {e}")))
        }
        Action::LogSearch { file, query, limit } => {
            let file = file.clone();
            let query = query.clone();
            let limit = *limit;
            let result = blocking(move || search_file(&file, &query, limit)).await?;
            serde_json::to_value(result)
                .map_err(|e| ApiError::internal(format!("encode log search: {e}")))
        }
        Action::LogRead {
            file,
            lines,
            cursor,
        } => {
            let file = file.clone();
            let lines = *lines;
            let cursor = *cursor;
            let chunk = blocking(move || read_chunk(&file, lines, cursor)).await?;
            serde_json::to_value(chunk)
                .map_err(|e| ApiError::internal(format!("encode log chunk: {e}")))
        }
        _ => Err(ApiError::invalid(format!(
            "action {} is not valid for module logs",
            crate::modules::action_name(action)
        ))),
    }
}

/// Run a blocking closure on the blocking pool (file IO must never stall
/// the async executor).
async fn blocking<F, T>(work: F) -> Result<T, ApiError>
where
    F: FnOnce() -> Result<T, ApiError> + Send + 'static,
    T: Send + 'static,
{
    tokio::task::spawn_blocking(work)
        .await
        .map_err(|e| ApiError::internal(format!("logs task failed: {e}")))?
}

/// Fresh scan of `/var/log` (new files appear on the next list call).
fn list_logs() -> Result<Vec<LogFile>, ApiError> {
    let root = Path::new(LOG_ROOT);
    let mut files = Vec::new();
    scan_dir(root, root, 0, &mut files)?;
    files.sort_by(|a, b| b.modified.cmp(&a.modified).then(b.size.cmp(&a.size)));
    Ok(files)
}

fn scan_dir(root: &Path, dir: &Path, depth: u32, out: &mut Vec<LogFile>) -> Result<(), ApiError> {
    if depth > MAX_DEPTH {
        return Ok(());
    }
    let entries = std::fs::read_dir(dir)
        .map_err(|e| ApiError::internal(format!("read {}: {e}", dir.display())))?;
    for entry in entries.flatten() {
        let file_type = entry
            .file_type()
            .map_err(|e| ApiError::internal(format!("stat {}: {e}", entry.path().display())))?;
        // Never follow symlinks while walking a root-writable-adjacent tree.
        if file_type.is_symlink() {
            continue;
        }
        if file_type.is_dir() {
            scan_dir(root, &entry.path(), depth + 1, out)?;
            continue;
        }
        if !file_type.is_file() {
            continue;
        }
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        let lower = name.to_ascii_lowercase();
        if COMPRESSED.iter().any(|ext| lower.ends_with(ext)) {
            continue;
        }
        let Ok(meta) = entry.metadata() else {
            continue;
        };
        let Ok(rel) = path.strip_prefix(root) else {
            continue;
        };
        let modified = meta.modified().map_or(0, |t| {
            t.duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
        });
        out.push(LogFile {
            path: rel.to_string_lossy().into_owned(),
            size: meta.len(),
            modified,
        });
    }
    Ok(())
}

/// Resolve a GUI-provided relative path to a real file under `/var/log`
/// (absolute paths, `..` and symlinks leaving the root are rejected).
fn resolve_under_logroot(rel: &str) -> Result<PathBuf, ApiError> {
    let requested = Path::new(rel);
    if requested.is_absolute() {
        return Err(ApiError::invalid("log path must be relative to /var/log"));
    }
    let mut depth = 0usize;
    for component in requested.components() {
        match component {
            Component::Normal(_) => depth += 1,
            Component::CurDir => {}
            _ => {
                return Err(ApiError::invalid("log path escapes /var/log"));
            }
        }
    }
    if depth == 0 || depth > 8 {
        return Err(ApiError::invalid("invalid log path"));
    }
    let root = Path::new(LOG_ROOT)
        .canonicalize()
        .map_err(|e| ApiError::internal(format!("resolve /var/log: {e}")))?;
    let joined = root.join(requested);
    // Canonicalization resolves symlinks: the REAL target must stay inside
    // /var/log (best-effort TOCTOU coverage, same stance as theming writes).
    let resolved = joined
        .canonicalize()
        .map_err(|_| ApiError::new(ErrorCode::NotFound, "log file not found"))?;
    if !resolved.starts_with(&root) {
        return Err(ApiError::invalid("log path escapes /var/log via symlink"));
    }
    if !resolved.is_file() {
        return Err(ApiError::invalid("not a regular file"));
    }
    Ok(resolved)
}

/// Read one chunk according to `cursor` (SPEC-logs).
fn read_chunk(rel: &str, lines: u32, cursor: LogCursor) -> Result<LogChunk, ApiError> {
    let lines = lines.clamp(1, ursine_api::max_log_lines()) as usize;
    let path = resolve_under_logroot(rel)?;
    let mut file = std::fs::File::open(&path)
        .map_err(|e| ApiError::internal(format!("open {}: {e}", path.display())))?;
    let size = file
        .metadata()
        .map_err(|e| ApiError::internal(format!("stat {}: {e}", path.display())))?
        .len();

    match cursor {
        LogCursor::End => {
            if size == 0 {
                return Ok(empty_chunk(0, 0, 0));
            }
            let bound = size.saturating_sub(READ_CAP);
            file.seek(SeekFrom::Start(bound))
                .map_err(|e| ApiError::internal(format!("seek: {e}")))?;
            let mut buf = Vec::new();
            file.take(READ_CAP)
                .read_to_end(&mut buf)
                .map_err(|e| ApiError::internal(format!("read: {e}")))?;
            ensure_text(&buf)?;
            let mut start_in_buf = 0usize;
            if bound > 0 {
                // The window starts mid-line: drop the leading partial.
                match buf.iter().position(|b| *b == b'\n') {
                    Some(i) => start_in_buf = i + 1,
                    None => return Ok(empty_chunk(bound + buf.len() as u64, size, size)),
                }
            }
            let rest = buf.get(start_in_buf..).unwrap_or(&[]);
            let mut spans = line_spans(rest);
            // Drop from the FRONT until `lines` remain; `cut` = end of the
            // last dropped line, relative to the window start.
            let mut cut = 0usize;
            while spans.len() > lines {
                let (offset, len) = spans.remove(0);
                cut = offset + len + 1;
            }
            start_in_buf += cut;
            let content =
                String::from_utf8_lossy(buf.get(start_in_buf..).unwrap_or(&[])).into_owned();
            let start = bound + start_in_buf as u64;
            Ok(LogChunk {
                content,
                start,
                end: size,
                size,
                has_before: start > 0,
            })
        }
        LogCursor::Before { offset } => {
            let offset = offset.min(size);
            if offset == 0 {
                return Ok(empty_chunk(0, 0, size));
            }
            let lower = offset.saturating_sub(READ_CAP);
            file.seek(SeekFrom::Start(lower))
                .map_err(|e| ApiError::internal(format!("seek: {e}")))?;
            let mut buf = Vec::new();
            file.take(offset - lower)
                .read_to_end(&mut buf)
                .map_err(|e| ApiError::internal(format!("read: {e}")))?;
            ensure_text(&buf)?;
            let mut start_in_buf = 0usize;
            if lower > 0 {
                match buf.iter().position(|b| *b == b'\n') {
                    Some(i) => start_in_buf = i + 1,
                    None => return Ok(empty_chunk(offset, offset, size)),
                }
            }
            let rest = buf.get(start_in_buf..).unwrap_or(&[]);
            let mut spans = line_spans(rest);
            let mut cut = 0usize;
            while spans.len() > lines {
                let (offset, len) = spans.remove(0);
                cut = offset + len + 1;
            }
            start_in_buf += cut;
            let content =
                String::from_utf8_lossy(buf.get(start_in_buf..).unwrap_or(&[])).into_owned();
            let start = lower + start_in_buf as u64;
            Ok(LogChunk {
                content,
                start,
                end: offset,
                size,
                has_before: start > 0,
            })
        }
        LogCursor::From { offset } => {
            let offset = offset.min(size);
            if offset >= size {
                return Ok(empty_chunk(offset, offset, size));
            }
            file.seek(SeekFrom::Start(offset))
                .map_err(|e| ApiError::internal(format!("seek: {e}")))?;
            let mut buf = Vec::new();
            file.take(READ_CAP)
                .read_to_end(&mut buf)
                .map_err(|e| ApiError::internal(format!("read: {e}")))?;
            ensure_text(&buf)?;
            // Complete lines only: a partial tail waits for the next poll.
            let last_newline = buf.iter().rposition(|b| *b == b'\n');
            let Some(last_newline) = last_newline else {
                // No newline at all: an unterminated tail (last line of the
                // file, or a line still being written) is delivered whole so
                // search jumps to it actually show content; the cursor
                // advances past these bytes (no duplicates on the next poll).
                if buf.is_empty() {
                    return Ok(empty_chunk(offset, offset, size));
                }
                let content = String::from_utf8_lossy(&buf).into_owned();
                let end = offset + buf.len() as u64;
                return Ok(LogChunk {
                    content,
                    start: offset,
                    end,
                    size,
                    has_before: offset > 0,
                });
            };
            let mut complete = last_newline + 1;
            let mut seen = 0usize;
            for (index, byte) in buf.iter().enumerate() {
                if *byte == b'\n' {
                    seen += 1;
                    if seen == lines {
                        complete = index + 1;
                        break;
                    }
                }
            }
            let content = String::from_utf8_lossy(buf.get(..complete).unwrap_or(&[])).into_owned();
            Ok(LogChunk {
                content,
                start: offset,
                end: offset + complete as u64,
                size,
                has_before: offset > 0,
            })
        }
    }
}

fn empty_chunk(start: u64, end: u64, size: u64) -> LogChunk {
    LogChunk {
        content: String::new(),
        start,
        end,
        size,
        has_before: start > 0,
    }
}

/// Text logs only: a NUL byte means this is not a text log (binary/gzip).
fn ensure_text(buf: &[u8]) -> Result<(), ApiError> {
    if buf.contains(&0) {
        return Err(ApiError::invalid("not a text log (binary content)"));
    }
    Ok(())
}

/// `(offset, len)` spans of lines inside `buf` (excluding the `\n` bytes);
/// an unterminated final segment counts as the last line.
fn line_spans(buf: &[u8]) -> Vec<(usize, usize)> {
    let mut spans = Vec::new();
    let mut start = 0usize;
    for (index, byte) in buf.iter().enumerate() {
        if *byte == b'\n' {
            spans.push((start, index - start));
            start = index + 1;
        }
    }
    if start < buf.len() {
        spans.push((start, buf.len() - start));
    }
    spans
}

/// Substring search (case-insensitive) over a `/var/log` file.
fn search_file(rel: &str, query: &str, limit: u32) -> Result<LogSearchResult, ApiError> {
    let query = query.trim();
    if query.is_empty() {
        return Err(ApiError::invalid("search query is empty"));
    }
    let limit = limit.clamp(1, MAX_SEARCH_LIMIT) as usize;
    let path = resolve_under_logroot(rel)?;
    let file = std::fs::File::open(&path)
        .map_err(|e| ApiError::internal(format!("open {}: {e}", path.display())))?;
    let needle = query.to_lowercase();
    let mut reader = std::io::BufReader::new(file);
    let mut raw: Vec<u8> = Vec::new();
    let mut offset: u64 = 0;
    let mut line_no: u64 = 0;
    let mut scanned: u64 = 0;
    let mut truncated_scan = false;
    let mut hits = Vec::new();
    loop {
        raw.clear();
        // Per-line bound: a pathological single-line file must never be
        // buffered whole (fresh `take` each iteration).
        let read = (&mut reader)
            .take(SEARCH_SCAN_CAP)
            .read_until(b'\n', &mut raw)
            .map_err(|e| ApiError::internal(format!("read {}: {e}", path.display())))?;
        if read == 0 {
            break;
        }
        scanned += read as u64;
        let line_start = offset;
        offset += read as u64;
        line_no += 1;
        if scanned > SEARCH_SCAN_CAP {
            truncated_scan = true;
            break;
        }
        if raw.contains(&0) {
            // Same contract as chunk reads: this is not a text log.
            return Err(ApiError::invalid("not a text log (binary content)"));
        }
        if raw.len() as u64 >= SEARCH_SCAN_CAP && raw.last() != Some(&b'\n') {
            truncated_scan = true;
            break;
        }
        let text = String::from_utf8_lossy(&raw);
        if text.to_lowercase().contains(&needle) {
            let preview: String = text.chars().take(HIT_LINE_CHARS).collect();
            hits.push(LogHit {
                line_no,
                offset: line_start,
                line: preview.trim_end_matches('\n').to_string(),
            });
            if hits.len() >= limit {
                break;
            }
        }
    }
    Ok(LogSearchResult {
        hits,
        truncated_scan,
    })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::disallowed_methods)] // tests write under /var/log
    use super::*;

    fn write_log(name: &str, content: &[u8]) -> PathBuf {
        let path = Path::new(LOG_ROOT).join(name);
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        std::fs::write(&path, content).unwrap();
        path
    }

    #[test]
    fn list_discovers_new_files_and_skips_compressed() {
        let _ = std::fs::remove_file(Path::new(LOG_ROOT).join("ursine-test-a.log"));
        write_log("ursine-test-a.log", b"hello\n");
        write_log("ursine-test-b.log.gz", b"binary");
        let files = list_logs().unwrap();
        let names: Vec<&str> = files.iter().map(|f| f.path.as_str()).collect();
        assert!(names.contains(&"ursine-test-a.log"), "{names:?}");
        assert!(
            !names.iter().any(|n| n.to_lowercase().ends_with(".gz")),
            "{names:?}"
        );
        // Fresh scan: a file created later shows up without any config.
        write_log("ursine-test-c.log", b"custom drop-in\n");
        let files = list_logs().unwrap();
        assert!(files.iter().any(|f| f.path == "ursine-test-c.log"));
        let _ = std::fs::remove_file(Path::new(LOG_ROOT).join("ursine-test-a.log"));
        let _ = std::fs::remove_file(Path::new(LOG_ROOT).join("ursine-test-b.log.gz"));
        let _ = std::fs::remove_file(Path::new(LOG_ROOT).join("ursine-test-c.log"));
    }

    #[test]
    fn path_escapes_are_rejected() {
        assert!(resolve_under_logroot("/etc/passwd").is_err());
        assert!(resolve_under_logroot("../etc/passwd").is_err());
        assert!(resolve_under_logroot("a/../../etc").is_err());
        assert!(resolve_under_logroot("").is_err());
        assert!(resolve_under_logroot("..").is_err());
    }

    #[test]
    fn tail_before_and_follow_cursors() {
        let path = write_log(
            "ursine-test-cursor.log",
            b"l1\nl2\nl3\nl4\nl5\nl6\nl7\nl8\nl9\nl10\n",
        );
        let rel = "ursine-test-cursor.log";
        let end = read_chunk(rel, 3, LogCursor::End).unwrap();
        assert_eq!(end.content, "l8\nl9\nl10\n");
        assert_eq!(end.end, end.size);
        assert!(end.has_before);
        let prev = read_chunk(rel, 3, LogCursor::Before { offset: end.start }).unwrap();
        assert_eq!(prev.content, "l5\nl6\nl7\n");
        assert_eq!(prev.end, end.start);
        let from = read_chunk(rel, 3, LogCursor::From { offset: 0 }).unwrap();
        assert_eq!(from.content, "l1\nl2\nl3\n");
        assert_eq!(from.end, 9);
        assert!(!from.has_before);
        // Follow resumes from the previous end.
        let next = read_chunk(rel, 10, LogCursor::From { offset: from.end }).unwrap();
        assert_eq!(next.content, "l4\nl5\nl6\nl7\nl8\nl9\nl10\n");
        assert_eq!(next.end, next.size);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn line_count_is_clamped_and_empty_reads_are_safe() {
        let path = write_log("ursine-test-clamp.log", b"only one line no newline");
        let out = read_chunk("ursine-test-clamp.log", 0, LogCursor::End).unwrap();
        assert!(out.content.contains("only one line"));
        let out = read_chunk("ursine-test-clamp.log", 999_999, LogCursor::End).unwrap();
        assert!(out.content.contains("only one line"));
        assert!(!out.has_before);
        let missing = read_chunk("ursine-test-missing.log", 5, LogCursor::End);
        assert!(missing.is_err());
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn binary_content_is_rejected() {
        let path = write_log("ursine-test-binary.log", b"abc\0def\n");
        let err = read_chunk("ursine-test-binary.log", 5, LogCursor::End).unwrap_err();
        assert!(err.message.contains("binary"), "{}", err.message);
        let _ = std::fs::remove_file(path);
    }
}
