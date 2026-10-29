//! Log viewer types (SPEC-logs): dynamic `/var/log` discovery plus
//! cursor-based chunk reads (tail / previous / follow).

use serde::{Deserialize, Serialize};

/// One discovered log file; `path` is relative to `/var/log`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LogFile {
    pub path: String,
    pub size: u64,
    /// Unix seconds.
    pub modified: u64,
}

/// Where in a log file to read from (`LogRead`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum LogCursor {
    /// Last `lines` lines (baseline, refresh, line-count input).
    End,
    /// The `lines` lines ending at `offset` ("load previous").
    Before { offset: u64 },
    /// Up to `lines` complete lines starting at `offset` (follow polls).
    From { offset: u64 },
}

/// One read result: plain-text content plus the offsets the GUI keeps for
/// previous-chunk reads, follow polling and rotation detection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LogChunk {
    pub content: String,
    /// First byte of `content` in the file.
    pub start: u64,
    /// Byte after the last returned line (polling cursor).
    pub end: u64,
    /// Current file size (size < end means rotation/truncation).
    pub size: u64,
    /// Earlier data exists (enables "load previous").
    pub has_before: bool,
}

/// Default line count for a fresh tail (GUI input seeds from this).
pub fn default_log_lines() -> u32 {
    200
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cursor_serializes_with_mode_tag() {
        let end = serde_json::to_string(&LogCursor::End).unwrap();
        assert_eq!(end, "{\"mode\":\"end\"}");
        let prev = serde_json::to_string(&LogCursor::Before { offset: 42 }).unwrap();
        assert_eq!(prev, "{\"mode\":\"before\",\"offset\":42}");
        let from = serde_json::to_string(&LogCursor::From { offset: 7 }).unwrap();
        assert_eq!(from, "{\"mode\":\"from\",\"offset\":7}");
        let back: LogCursor = serde_json::from_str(&from).unwrap();
        assert_eq!(back, LogCursor::From { offset: 7 });
    }

    #[test]
    fn chunk_roundtrips() {
        let chunk = LogChunk {
            content: "one\ntwo\n".into(),
            start: 0,
            end: 9,
            size: 4096,
            has_before: false,
        };
        let s = serde_json::to_string(&chunk).unwrap();
        assert_eq!(serde_json::from_str::<LogChunk>(&s).unwrap(), chunk);
    }

    #[test]
    fn default_lines_is_200() {
        assert_eq!(default_log_lines(), 200);
    }
}
