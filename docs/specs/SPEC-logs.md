# SPEC — Log viewer (`logs`)

Status: **planned → implementing** (user decision 2026-10-29).

## Requirements (user)

- Dynamically discover log files under `/var/log/` — works on every
  distro, no per-file configuration: a custom file dropped into `/var/log`
  is picked up by the next refresh.
- Render files as plain text.
- Show the **last 200 lines** by default (default configurable).
- Button to **load earlier lines** (previous chunks).
- Line count editable in an **input field** (re-tails with the new count).
- **Follow button** = `tail -f` (live updates).

## Design

- Protocol: `ModuleId::Logs` with two actions (one module keeps the wire
  small; follows the `job_output` polling precedent — no daemon streaming):
  - `Action::LogList` → `Vec<LogFile>`: path relative to `/var/log`, size,
    mtime; scanned fresh on every call (dynamic discovery); compressed
    (`.gz/.xz/.bz2/.zip`) and non-regular entries skipped.
  - `Action::LogRead { file, lines, cursor }` with
    `LogCursor::End | Before { offset } | From { offset }` →
    `LogChunk { content, start, end, size, has_before }`:
    - `End`: the last `lines` lines (baseline, refresh, line-count input);
    - `Before { offset }`: the `lines` lines ending at `offset`
      ("load previous"; partial first line dropped);
    - `From { offset }`: forward read (follow polls every ~700 ms from the
      last `end`; `size < end` in a response = rotation/truncate → GUI
      resets to `End`).
- Limits: `lines` clamped 1..=10 000; reads capped at 512 KiB per call
  (many lines are cheap; a giant line is truncated at the cap); NUL bytes
  in the chunk ⇒ `binary` error (text only, per requirement).
- Security: `file` is a relative path validated under `/var/log`
  (absolute paths, `..`, and symlinks resolving outside are rejected);
  the daemon is the only reader, sessions are already wheel-gated.
- Audit: `LogList` audited; `LogRead` not (high-frequency follow polls —
  same precedent as `job_output`, commented in dispatch).
- `as_user` not valid (host logs, not per-user).

## GUI

- Sidebar entry **Logs** (always available — no capability needed).
- File dropdown (path + size) with Refresh; selecting re-tails.
- Controls: line-count input (default `200`) + apply, **Load previous**
  (disabled until `has_before`), **Follow** toggler (snap-to-bottom on
  append, auto-refresh ~700 ms, stops on toggle/screen change/sign-out).
- **Search**: query box + Search → case-insensitive substring hits
  (`LogSearch`: 4 MiB scan cap with `truncated_scan`, default 50 hits,
  500 max, 400-char previews with line numbers); clicking a hit replaces
  the body with the chunk around it (offset-anchored `From` read) and
  keeps previous/follow working from the new cursor.
- Body: monospace text box with a side scrollbar (same widget style as
  the package job log).
