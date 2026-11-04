//! Structured extraction of `niri validate` (miette) failures.
//!
//! `niri validate -c <tmp>` prints miette diagnostics: an `Error: × <message>`
//! line, a `╭─[<file>:<line>:<col>]` header, snippet rows `N │ …` and `·`
//! underline rows. The underlined snippet row is the reliable location (the
//! header coordinate can point at the enclosing node), so we prefer it and
//! map the line back to the managed section and field of the proposed config
//! for field-level GUI errors.

use ursine_api::NiriValidateError;

/// `(message, optional header coordinate, optional underlined snippet row)`.
type ParsedBlock = (String, Option<(u32, u32)>, Option<u32>);

/// One diagnostic block being accumulated from stderr.
#[derive(Default)]
struct Block {
    message: Option<String>,
    causes: Vec<String>,
    header: Option<(u32, u32)>,
    last_row: Option<u32>,
    underline_seen: bool,
}

impl Block {
    /// Finish the block; returns `(message, header, row)`. Message-only
    /// blocks (no snippet header) are still returned so spanless
    /// diagnostics are not silently dropped.
    fn take(&mut self) -> Option<ParsedBlock> {
        let message = self.message.take()?;
        let header = self.header.take();
        let row = self.last_row.take();
        self.underline_seen = false;
        Some((message, header, row))
    }

    fn start_snippet(&mut self) {
        self.last_row = None;
        self.underline_seen = false;
    }
}

/// Parse `niri validate` stderr into field-level errors.
///
/// Returns an empty vector when nothing structured was reported; the caller
/// then falls back to the raw text.
pub(super) fn parse_validate_errors(stderr: &str, content: &str) -> Vec<NiriValidateError> {
    let mut errors = Vec::new();
    let mut block = Block::default();
    for line in stderr.lines() {
        if line.starts_with("Error:") {
            // Flush whatever is pending: a located block, or a spanless
            // diagnostic (message-only) that must not vanish.
            if let Some(built) = finish(&mut block, content) {
                errors.push(built);
            }
            block.start_snippet();
            let message = line.find('×').map_or_else(
                || line.trim_start_matches("Error:"),
                |idx| line.get(idx + '×'.len_utf8()..).unwrap_or(""),
            );
            block.message = Some(message.trim().to_string());
            continue;
        }
        let trimmed = line.trim_start();
        if let Some(rest) = trimmed
            .strip_prefix("├─▶")
            .or_else(|| trimmed.strip_prefix("╰─▶"))
        {
            block.causes.push(rest.trim().to_string());
            continue;
        }
        if let Some(header) = parse_header(line) {
            block.header = Some(header);
            block.start_snippet();
            continue;
        }
        if let Some(row) = parse_row(line) {
            if !block.underline_seen {
                block.last_row = Some(row);
            }
            continue;
        }
        if trimmed.starts_with('·') {
            block.underline_seen = true;
        }
    }
    if let Some(built) = finish(&mut block, content) {
        errors.push(built);
    }
    errors
}

fn finish(block: &mut Block, content: &str) -> Option<NiriValidateError> {
    let causes = std::mem::take(&mut block.causes);
    let (message, header, row) = block.take()?;
    let message = if causes.is_empty() {
        message
    } else {
        format!("{message}: {}", causes.join("; "))
    };
    // The underlined snippet row wins; the header coordinate only pairs with
    // it when both point at the same line (miette headers can name the
    // enclosing node instead of the span).
    let (line, column) = match (row, header) {
        (Some(row), Some((header_line, column))) if row == header_line => (Some(row), Some(column)),
        (Some(row), _) => (Some(row), None),
        (None, Some((line, column))) => (Some(line), Some(column)),
        (None, None) => (None, None),
    };
    let (section, field) = line.map_or((None, None), |line| map_location(content, line));
    Some(NiriValidateError {
        message,
        line,
        column,
        section,
        field,
    })
}

/// `╭─[<file>:<line>:<col>]` → `(line, column)`.
fn parse_header(line: &str) -> Option<(u32, u32)> {
    let start = line.find("╭─[")? + "╭─[".len();
    let rest = line.get(start..)?;
    let end = rest.find(']')?;
    let coords = rest.get(..end)?;
    let mut parts = coords.rsplitn(3, ':');
    let column: u32 = parts.next()?.parse().ok()?;
    let line: u32 = parts.next()?.parse().ok()?;
    Some((line, column))
}

/// ` 3 │     bogus-field 3` → `3`.
fn parse_row(line: &str) -> Option<u32> {
    let trimmed = line.trim_start();
    let digits_end = trimmed.find(|c: char| !c.is_ascii_digit())?;
    if digits_end == 0 {
        return None;
    }
    trimmed
        .get(digits_end..)?
        .trim_start()
        .strip_prefix('│')
        .map(|_| ())?;
    trimmed.get(..digits_end)?.parse().ok()
}

/// Map a 1-based content line to `(section, field)` of the proposed config.
fn map_location(content: &str, line: u32) -> (Option<String>, Option<String>) {
    let mut depth: i32 = 0;
    let mut section: Option<String> = None;
    let mut in_block_comment = false;
    for (idx, raw) in content.lines().enumerate() {
        let current = u32::try_from(idx).unwrap_or(u32::MAX) + 1;
        let code = strip_noise(raw, &mut in_block_comment);
        let trimmed = code.trim_start();
        let top_level = depth == 0 && !trimmed.is_empty();
        let token = first_identifier(trimmed);
        if current == line {
            return match (top_level, token) {
                // The offending line opens the section itself.
                (true, Some(node)) => (Some(section_key(&node).to_string()), None),
                (_, token) => (
                    section.as_ref().map(|name| section_key(name).to_string()),
                    token,
                ),
            };
        }
        if top_level {
            section = token;
        }
        depth += brace_delta(&code);
        if depth <= 0 {
            depth = 0;
            section = None;
        }
    }
    (None, None)
}

/// niri node name → managed section key (`set_section` vocabulary).
fn section_key(node: &str) -> &str {
    match node {
        "keyboard" => "input.keyboard",
        "touchpad" => "input.touchpad",
        "mouse" => "input.mouse",
        "binds" => "keybinds",
        "spawn-at-startup" => "spawn",
        "output" => "outputs",
        "screenshot-path" => "screenshot",
        other => other,
    }
}

/// Strip string bodies, `//` line comments and `/* … */` block comments
/// (state threaded across lines) so braces and tokens reflect code.
fn strip_noise(line: &str, in_block_comment: &mut bool) -> String {
    let mut out = String::with_capacity(line.len());
    let mut in_string = false;
    let mut escaped = false;
    let mut chars = line.chars().peekable();
    while let Some(ch) = chars.next() {
        if *in_block_comment {
            if ch == '*' && chars.peek() == Some(&'/') {
                chars.next();
                *in_block_comment = false;
            }
            continue;
        }
        if in_string {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_string = false;
            }
            continue;
        }
        if ch == '"' {
            in_string = true;
            continue;
        }
        if ch == '/' && chars.peek() == Some(&'/') {
            break;
        }
        if ch == '/' && chars.peek() == Some(&'*') {
            chars.next();
            *in_block_comment = true;
            continue;
        }
        out.push(ch);
    }
    out
}

fn brace_delta(code: &str) -> i32 {
    code.chars().fold(0, |acc, ch| match ch {
        '{' => acc + 1,
        '}' => acc - 1,
        _ => acc,
    })
}

/// First KDL identifier on the line, if it reads as a name (not a number).
fn first_identifier(trimmed: &str) -> Option<String> {
    let token: String = trimmed
        .chars()
        .take_while(|ch| ch.is_ascii_alphanumeric() || *ch == '_' || *ch == '-')
        .collect();
    if token.is_empty() {
        return None;
    }
    let named = token
        .chars()
        .any(|ch| ch.is_ascii_alphabetic() || ch == '_' || ch == '-');
    named.then_some(token)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Captured verbatim from `niri 25.11 validate -c` on an unknown node.
    const BAD_FIELD_STDERR: &str = "Error:   × error loading config\n  ├─▶ error parsing\n  ╰─▶ error parsing KDL\n\nError:   × unexpected node `bogus-field`\n   ╭─[.niri-validate.XXXXXX:2:1]\n 2 │     gaps 8\n 3 │     bogus-field 3\n   ·     ───────┬──────\n   ·            ╰── unexpected node\n 4 │ }\n   ╰────\n";

    const PROPOSED: &str = "layout {\n    gaps 8\n    bogus-field 3\n}\n";

    // Captured verbatim from `niri validate -c` on an unclosed brace.
    const SYNTAX_STDERR: &str = "Error:   × error loading config\n  ├─▶ error parsing\n  ╰─▶ error parsing KDL\n\nError:   × unclosed curly braces `{`\n   ╭─[.niri-validate.XXXXXX:1:1]\n 1 │ layout {\n   ·        ┬\n   ·        ╰── opened here\n 2 │     gaps 8\n   ╰────\n";

    #[test]
    fn parses_unknown_node_error() {
        let errors = parse_validate_errors(BAD_FIELD_STDERR, PROPOSED);
        // Spanless wrapper block first, then the located detail.
        assert_eq!(errors.len(), 2, "{errors:?}");
        assert_eq!(
            errors[0].message,
            "error loading config: error parsing; error parsing KDL"
        );
        assert_eq!(errors[0].line, None);
        let error = &errors[1];
        assert_eq!(error.message, "unexpected node `bogus-field`");
        assert_eq!(error.line, Some(3), "underline row wins over header 2:1");
        // Header names a different line than the underline row: no pair.
        assert_eq!(error.column, None);
        assert_eq!(error.section.as_deref(), Some("layout"));
        assert_eq!(error.field.as_deref(), Some("bogus-field"));
    }

    #[test]
    fn empty_stderr_yields_no_errors() {
        assert!(parse_validate_errors("", PROPOSED).is_empty());
        assert!(parse_validate_errors("config is valid\n", PROPOSED).is_empty());
    }

    #[test]
    fn parses_syntax_error_with_matching_header_and_row() {
        let errors = parse_validate_errors(SYNTAX_STDERR, "layout {\n    gaps 8\n");
        assert_eq!(errors.len(), 2, "{errors:?}");
        let error = &errors[1];
        assert_eq!(error.message, "unclosed curly braces `{`");
        // Row 1 and header line 1 agree, so the column is kept.
        assert_eq!(error.line, Some(1));
        assert_eq!(error.column, Some(1));
        assert_eq!(error.section.as_deref(), Some("layout"));
        assert_eq!(error.field, None, "top-level node has no field");
    }

    #[test]
    fn maps_nested_section_names() {
        let content = "output \"DP-1\" {\n    mode bogus 3\n}\n";
        let stderr = "Error:   × unexpected node `bogus`\n   ╭─[x.kdl:1:1]\n 1 │ output \"DP-1\" {\n 2 │     mode bogus 3\n   ·     ────┬────\n   ·         ╰── unexpected node\n   ╰────\n";
        let errors = parse_validate_errors(stderr, content);
        assert_eq!(errors.len(), 1, "{errors:?}");
        assert_eq!(errors[0].line, Some(2));
        assert_eq!(errors[0].section.as_deref(), Some("outputs"));
        assert_eq!(errors[0].field.as_deref(), Some("mode"));
    }

    #[test]
    fn braces_inside_strings_do_not_change_depth() {
        let content = "environment {\n    X \"a{b}c\"\n    bad 1\n}\n";
        let mut depth = 0i32;
        let mut in_block = false;
        for raw in content.lines() {
            let code = strip_noise(raw, &mut in_block);
            depth += brace_delta(&code);
        }
        assert_eq!(depth, 0);
        let (section, field) = map_location(content, 3);
        assert_eq!(section.as_deref(), Some("environment"));
        assert_eq!(field.as_deref(), Some("bad"));
    }

    #[test]
    fn block_comments_do_not_change_depth() {
        let content = "layout {\n    /* { unbalanced */\n    gaps 8\n}\n";
        let (section, field) = map_location(content, 3);
        assert_eq!(section.as_deref(), Some("layout"));
        assert_eq!(field.as_deref(), Some("gaps"));
    }

    #[test]
    fn section_key_translates_nodes() {
        assert_eq!(section_key("binds"), "keybinds");
        assert_eq!(section_key("spawn-at-startup"), "spawn");
        assert_eq!(section_key("layout"), "layout");
    }

    #[test]
    fn header_only_block_still_maps() {
        let stderr = "Error:   × unexpected node `x`\n   ╭─[c.kdl:3:5]\n 3 │     x 1\n   ╰────\n";
        let errors = parse_validate_errors(stderr, "layout {\n    gaps 1\n    x 1\n}\n");
        assert_eq!(errors.len(), 1, "{errors:?}");
        assert_eq!(errors[0].line, Some(3));
        assert_eq!(errors[0].column, Some(5));
        assert_eq!(errors[0].field.as_deref(), Some("x"));
    }
}
