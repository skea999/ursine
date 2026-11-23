//! scanners — SANE discovery and managed airscan devices.

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::json;
use ursine_api::{Action, AirscanDevice, ApiError, ErrorCode, Scanner, ScannerConfigView};

use super::ModuleCtx;
use crate::modules::packages_nix::exec;

const SCANIMAGE: [&str; 3] = [
    "/usr/bin/scanimage",
    "/usr/local/bin/scanimage",
    "/bin/scanimage",
];
const DEFAULT_CONFIG: &str = "/etc/sane.d/airscan.conf";
const TIMEOUT: Duration = Duration::from_secs(120);

pub async fn handle(action: &Action, ctx: &ModuleCtx<'_>) -> Result<serde_json::Value, ApiError> {
    match action {
        Action::ListScanners => Ok(json!({"scanners": detect(ctx.as_user).await?})),
        Action::GetScannerConfig => Ok(json!(read_view(ctx).await?)),
        Action::SetScannerDevice {
            name,
            address,
            protocol,
        } => {
            validate_name(name)?;
            validate_address(address)?;
            validate_protocol(protocol)?;
            let text = read_config_text()?;
            let device = AirscanDevice {
                name: name.clone(),
                address: address.clone(),
                protocol: protocol.clone(),
            };
            let edited = upsert_device(&text, &device)?;
            store(
                &edited,
                &text,
                crate::backup_configs::copies_from(&ctx.config),
            )?;
            Ok(json!(read_view(ctx).await?))
        }
        Action::RemoveScannerDevice { name } => {
            validate_name(name)?;
            let text = read_config_text()?;
            let edited = remove_device(&text, name)?;
            store(
                &edited,
                &text,
                crate::backup_configs::copies_from(&ctx.config),
            )?;
            Ok(json!(read_view(ctx).await?))
        }
        Action::SetScannerDiscovery { enabled } => {
            let text = read_config_text()?;
            let edited = set_discovery(&text, !enabled)?;
            store(
                &edited,
                &text,
                crate::backup_configs::copies_from(&ctx.config),
            )?;
            Ok(json!(read_view(ctx).await?))
        }
        _ => Err(ApiError::invalid("action is not valid for module scanners")),
    }
}

#[derive(Debug, Default, PartialEq, Eq)]
struct AirscanConfig {
    devices: Vec<AirscanDevice>,
    discovery_disabled: bool,
}

async fn read_view(ctx: &ModuleCtx<'_>) -> Result<ScannerConfigView, ApiError> {
    let config = read_config()?;
    let path = config_path();
    Ok(ScannerConfigView {
        path: path.display().to_string(),
        exists: path.exists(),
        discovery_disabled: config.discovery_disabled,
        devices: config.devices,
        detected: detect(ctx.as_user).await?,
    })
}

fn config_path() -> PathBuf {
    std::env::var_os("URSINE_AIRSCAN_CONFIG")
        .map_or_else(|| PathBuf::from(DEFAULT_CONFIG), PathBuf::from)
}

async fn detect(user: &str) -> Result<Vec<Scanner>, ApiError> {
    let program = tool(&SCANIMAGE, "scanimage")?;
    let output = exec::run_as_user_argv(user, program, &["-L"], TIMEOUT).await?;
    Ok(parse_devices(&output.stdout))
}

fn parse_devices(output: &str) -> Vec<Scanner> {
    output
        .lines()
        .filter_map(|line| {
            let rest = line.strip_prefix("device `")?;
            let (id, rest) = rest.split_once('\'')?;
            Some(Scanner {
                id: id.to_string(),
                description: rest
                    .strip_prefix(" is a ")
                    .unwrap_or(rest)
                    .trim()
                    .to_string(),
            })
        })
        .collect()
}

/// The airscan config must be a regular file: never follow a planted symlink.
fn config_meta(path: &Path) -> Result<std::fs::Metadata, ApiError> {
    let meta = std::fs::symlink_metadata(path)
        .map_err(|e| ApiError::internal(format!("inspect airscan config: {e}")))?;
    if meta.file_type().is_symlink() || !meta.is_file() {
        return Err(ApiError::invalid("airscan config is not a regular file"));
    }
    Ok(meta)
}

/// Write the edit only when it changes the file: a call that changes nothing
/// must not rotate the backup slots or create a file that was never there.
fn store(edited: &str, original: &str, copies: usize) -> Result<(), ApiError> {
    if edited == original {
        return Ok(());
    }
    write_config(edited, copies)
}

/// The config file exactly as it is on disk; a missing file is an empty base,
/// so the first edit creates it from nothing.
fn read_config_text() -> Result<String, ApiError> {
    let path = config_path();
    if !path.exists() {
        return Ok(String::new());
    }
    config_meta(&path)?;
    let bytes = std::fs::read(&path)
        .map_err(|e| ApiError::internal(format!("read airscan config: {e}")))?;
    // The editor works on lines of text; bytes it cannot read are refused with
    // a fix instead of an unexplained internal error.
    String::from_utf8(bytes).map_err(|_| {
        uneditable(
            "it is not valid UTF-8",
            "save airscan.conf as UTF-8 and try again",
        )
    })
}

fn read_config() -> Result<AirscanConfig, ApiError> {
    Ok(parse_config(&read_config_text()?))
}

fn parse_config(text: &str) -> AirscanConfig {
    let mut config = AirscanConfig::default();
    let mut section = "";
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line.starts_with('[') {
            section = line;
            continue;
        }
        if section == "[devices]" {
            if let Some((name, value)) = line.split_once('=') {
                let name = name.trim().trim_matches('"').to_string();
                let mut parts = value.splitn(2, ',');
                let address = parts.next().unwrap_or_default().trim().to_string();
                let protocol = parts.next().unwrap_or("WSD").trim().to_string();
                if !name.is_empty() && !address.is_empty() {
                    config.devices.push(AirscanDevice {
                        name,
                        address,
                        protocol,
                    });
                }
            }
        } else if section == "[options]"
            && line
                .replace(' ', "")
                .eq_ignore_ascii_case("discovery=disable")
        {
            config.discovery_disabled = true;
        }
    }
    config
}

/// Write the edited text back atomically, keeping the shared backup rotation.
fn write_config(text: &str, copies: usize) -> Result<(), ApiError> {
    let path = config_path();
    let parent = path
        .parent()
        .ok_or_else(|| ApiError::invalid("airscan config has no parent"))?;
    std::fs::create_dir_all(parent)
        .map_err(|e| ApiError::internal(format!("create sane config dir: {e}")))?;
    if path.exists() {
        config_meta(&path)?;
        // Uniform rotation (replaces the old unbounded timestamped copies;
        // legacy ones are pruned inside rotate).
        crate::backup_configs::rotate(&path, copies, None)?;
    }
    let temp = path.with_file_name(format!(
        ".airscan.conf.ursine-{}-{}",
        std::process::id(),
        nanos()
    ));
    std::fs::write(&temp, text)
        .map_err(|e| ApiError::internal(format!("write airscan temp: {e}")))?;
    std::fs::rename(&temp, &path)
        .map_err(|e| ApiError::internal(format!("replace airscan config: {e}")))?;
    Ok(())
}

/// A file shape the editor refuses to touch: rewriting it would either lose
/// content the caller does not own or guess which of two targets was meant.
fn uneditable(reason: &str, fix: &str) -> ApiError {
    ApiError::new(
        ErrorCode::Conflict,
        format!("airscan config cannot be edited safely: {reason}"),
    )
    .with_fix(fix)
}

/// One line of the config, kept with the exact terminator it had on disk.
struct Line {
    text: String,
    ending: &'static str,
}

/// The config file as a list of lines that remember their own terminator, so an
/// edit rewrites only the lines it owns and copies every other byte unchanged.
struct Document {
    lines: Vec<Line>,
}

impl Document {
    fn parse(text: &str) -> Self {
        let lines = text
            .split_inclusive('\n')
            .map(|chunk| {
                chunk.strip_suffix('\n').map_or_else(
                    || Line {
                        text: chunk.to_string(),
                        ending: "",
                    },
                    |body| {
                        let crlf = body.ends_with('\r');
                        Line {
                            text: body.strip_suffix('\r').unwrap_or(body).to_string(),
                            ending: if crlf { "\r\n" } else { "\n" },
                        }
                    },
                )
            })
            .collect();
        Self { lines }
    }

    fn render(&self) -> String {
        let mut text = String::new();
        for line in &self.lines {
            text.push_str(&line.text);
            text.push_str(line.ending);
        }
        text
    }

    /// Insert a line at `index`, inheriting the terminator of the line it
    /// follows so a CRLF file stays CRLF.
    fn insert(&mut self, index: usize, text: String) {
        let previous = index.checked_sub(1);
        let mut ending = previous
            .and_then(|index| self.lines.get(index))
            .map_or("\n", |line| line.ending);
        if ending.is_empty() {
            // The line we append to had no terminator; it needs one now that a
            // line follows it.
            ending = "\n";
            if let Some(line) = previous.and_then(|index| self.lines.get_mut(index)) {
                line.ending = "\n";
            }
        }
        self.lines.insert(index, Line { text, ending });
    }

    fn remove(&mut self, index: usize) {
        let removed = self.lines.remove(index);
        // Removing the last line removes the file's final newline too: the
        // bytes before it stay as they were.
        if removed.ending.is_empty() {
            if let Some(last) = self.lines.last_mut() {
                last.ending = "";
            }
        }
    }

    /// Replace the text of an existing line; the index comes from a scan of
    /// this same document, so it always points at a line.
    fn set_text(&mut self, index: usize, text: String) {
        if let Some(line) = self.lines.get_mut(index) {
            line.text = text;
        }
    }

    /// Append a whole section at the end of the file, separated from existing
    /// content by one blank line.
    fn append_section(&mut self, header: &str, body: &[&str]) {
        if let Some(last) = self.lines.last_mut() {
            if last.ending.is_empty() {
                last.ending = "\n";
            }
        }
        if self
            .lines
            .last()
            .is_some_and(|line| !line.text.trim().is_empty())
        {
            self.lines.push(Line {
                text: String::new(),
                ending: "\n",
            });
        }
        self.lines.push(Line {
            text: header.to_string(),
            ending: "\n",
        });
        for line in body {
            self.lines.push(Line {
                text: (*line).to_string(),
                ending: "\n",
            });
        }
    }
}

/// The name of a `[section]` header line, tolerating a trailing comment.
fn header_name(line: &str) -> Option<&str> {
    let rest = line.trim().strip_prefix('[')?;
    let (name, _) = rest.split_once(']')?;
    Some(name.trim())
}

fn is_comment_or_blank(line: &str) -> bool {
    let trimmed = line.trim();
    trimmed.is_empty() || trimmed.starts_with('#')
}

/// The one header of `name`; a second one is an ambiguous edit target and is
/// refused rather than guessed at.
fn section_header(document: &Document, name: &str) -> Result<Option<usize>, ApiError> {
    let mut found = None;
    for (index, line) in document.lines.iter().enumerate() {
        if header_name(&line.text).is_some_and(|header| header.eq_ignore_ascii_case(name)) {
            if found.is_some() {
                return Err(uneditable(
                    &format!("it has more than one [{name}] section"),
                    &format!("keep one [{name}] section in airscan.conf and try again"),
                ));
            }
            found = Some(index);
        }
    }
    Ok(found)
}

/// First line after the section body: the next header, or the end of the file.
fn section_body_end(document: &Document, header: usize) -> usize {
    document
        .lines
        .iter()
        .enumerate()
        .skip(header + 1)
        .find(|(_, line)| header_name(&line.text).is_some())
        .map_or(document.lines.len(), |(index, _)| index)
}

/// Where a new key belongs: after the last non-blank line of the section, so
/// the blank lines that separate sections stay where they are.
fn insert_position(document: &Document, start: usize, end: usize) -> usize {
    document
        .lines
        .iter()
        .enumerate()
        .take(end)
        .skip(start)
        .filter(|(_, line)| !line.text.trim().is_empty())
        .map(|(index, _)| index + 1)
        .next_back()
        .unwrap_or(start)
}

/// The device name of a `"name" = address, protocol` row; a comment is never a
/// row, so a commented-out device stays untouched.
fn device_row_name(line: &str) -> Option<&str> {
    if is_comment_or_blank(line) {
        return None;
    }
    let (name, _) = line.split_once('=')?;
    let name = name.trim().trim_matches('"');
    (!name.is_empty()).then_some(name)
}

/// Indices of the `[devices]` rows that name `name`; more than one row is an
/// ambiguous edit target and is refused.
fn device_rows(
    document: &Document,
    header: usize,
    end: usize,
    name: &str,
) -> Result<Vec<usize>, ApiError> {
    let rows: Vec<usize> = document
        .lines
        .iter()
        .enumerate()
        .take(end)
        .skip(header + 1)
        .filter(|(_, line)| device_row_name(&line.text) == Some(name))
        .map(|(index, _)| index)
        .collect();
    if rows.len() > 1 {
        return Err(uneditable(
            &format!("it has {} rows for the device named {name:?}", rows.len()),
            "keep one row per device in [devices] and try again",
        ));
    }
    Ok(rows)
}

/// The key of a `key = value` line, or `None` for a comment, a blank line or a
/// line without `=`.
fn line_key(line: &str) -> Option<&str> {
    if is_comment_or_blank(line) {
        return None;
    }
    let (key, _) = line.split_once('=')?;
    Some(key.trim().trim_matches('"'))
}

/// Indices of the `discovery` keys inside `[options]`; more than one is an
/// ambiguous edit target and is refused.
fn discovery_rows(document: &Document, header: usize, end: usize) -> Result<Vec<usize>, ApiError> {
    let rows: Vec<usize> = document
        .lines
        .iter()
        .enumerate()
        .take(end)
        .skip(header + 1)
        .filter(|(_, line)| {
            line_key(&line.text).is_some_and(|key| key.eq_ignore_ascii_case("discovery"))
        })
        .map(|(index, _)| index)
        .collect();
    if rows.len() > 1 {
        return Err(uneditable(
            &format!("it has {} discovery keys in [options]", rows.len()),
            "keep one discovery line in [options] and try again",
        ));
    }
    Ok(rows)
}

/// Replace the `[devices]` row for `device.name`, or add one at the end of that
/// section; every other line of `text` comes back unchanged.
fn upsert_device(text: &str, device: &AirscanDevice) -> Result<String, ApiError> {
    let mut document = Document::parse(text);
    let row = format!(
        "\"{}\" = {}, {}",
        device.name, device.address, device.protocol
    );
    let Some(header) = section_header(&document, "devices")? else {
        document.append_section("[devices]", &[row.as_str()]);
        return Ok(document.render());
    };
    let end = section_body_end(&document, header);
    let rows = device_rows(&document, header, end, &device.name)?;
    if let [index] = rows.as_slice() {
        document.set_text(*index, row);
    } else {
        let position = insert_position(&document, header + 1, end);
        document.insert(position, row);
    }
    Ok(document.render())
}

/// Drop the `[devices]` row for `name`, leaving every other line as it was.
fn remove_device(text: &str, name: &str) -> Result<String, ApiError> {
    let mut document = Document::parse(text);
    let Some(header) = section_header(&document, "devices")? else {
        return Ok(text.to_string());
    };
    let end = section_body_end(&document, header);
    let rows = device_rows(&document, header, end, name)?;
    let Some(index) = rows.first() else {
        return Ok(text.to_string());
    };
    document.remove(*index);
    Ok(document.render())
}

/// Rewrite the `discovery` key inside `[options]`, adding the section only when
/// the file has none; every other line of `text` comes back unchanged.
fn set_discovery(text: &str, disabled: bool) -> Result<String, ApiError> {
    let mut document = Document::parse(text);
    let value = if disabled {
        "discovery = disable"
    } else {
        "discovery = enable"
    };
    let Some(header) = section_header(&document, "options")? else {
        document.append_section("[options]", &[value]);
        return Ok(document.render());
    };
    let end = section_body_end(&document, header);
    let rows = discovery_rows(&document, header, end)?;
    if let [index] = rows.as_slice() {
        document.set_text(*index, value.to_string());
    } else {
        let position = insert_position(&document, header + 1, end);
        document.insert(position, value.to_string());
    }
    Ok(document.render())
}

fn validate_name(name: &str) -> Result<(), ApiError> {
    if name.is_empty()
        || name.len() > 128
        || name.chars().any(|c| matches!(c, '"' | '\n' | '\r' | '\0'))
    {
        return Err(ApiError::invalid("invalid scanner name"));
    }
    Ok(())
}

fn validate_address(address: &str) -> Result<(), ApiError> {
    let valid = address.starts_with("http://") || address.starts_with("https://");
    if !valid || address.len() > 2048 || address.chars().any(char::is_control) {
        return Err(ApiError::invalid("scanner address must be an http(s) URL"));
    }
    Ok(())
}

fn validate_protocol(protocol: &str) -> Result<(), ApiError> {
    if !matches!(protocol, "WSD" | "eSCL") {
        return Err(ApiError::invalid("scanner protocol must be WSD or eSCL"));
    }
    Ok(())
}

fn tool(candidates: &[&'static str], label: &str) -> Result<&'static str, ApiError> {
    crate::tooling::resolve(candidates, label, "apk add sane-utils")
}

fn nanos() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_airscan_config() {
        let config = parse_config(
            "[devices]\n\"Brother MFC-1910W\" = http://192.168.1.2:80/WebServices/ScannerService, WSD\n\n[options]\ndiscovery = disable\n",
        );
        assert_eq!(config.devices.len(), 1);
        assert_eq!(config.devices[0].protocol, "WSD");
        assert!(config.discovery_disabled);
    }

    /// The sample from ISSUES.md entry 34: header comments, a second `[options]`
    /// key, a blacklist and a commented-out device.
    const SAMPLE: &str = "\
# airscan.conf - office\n\
# Brother MFC-1910W on the LAN\n\
[devices]\n\
\"Brother MFC-1910W\" = http://192.168.1.2:80/WebServices/ScannerService, WSD\n\
#\"Retired\" = http://192.168.1.9/eSCL, eSCL\n\
\n\
[options]\n\
model-dns-sd = disable\n\
discovery = disable\n\
\n\
[blacklist]\n\
ip = 10.0.0.0/8\n";

    fn device(name: &str, address: &str, protocol: &str) -> AirscanDevice {
        AirscanDevice {
            name: name.to_string(),
            address: address.to_string(),
            protocol: protocol.to_string(),
        }
    }

    #[test]
    fn adds_a_device_and_keeps_every_other_line() {
        let edited = upsert_device(
            SAMPLE,
            &device("Office", "https://scanner.local/eSCL", "eSCL"),
        )
        .expect("the sample is editable");
        let expected = SAMPLE.replace(
            "#\"Retired\" = http://192.168.1.9/eSCL, eSCL\n",
            "#\"Retired\" = http://192.168.1.9/eSCL, eSCL\n\"Office\" = https://scanner.local/eSCL, eSCL\n",
        );
        assert_eq!(edited, expected);
    }

    #[test]
    fn replaces_only_the_row_of_an_existing_device() {
        let edited = upsert_device(
            SAMPLE,
            &device("Brother MFC-1910W", "https://brother.local/eSCL", "eSCL"),
        )
        .expect("the sample is editable");
        let expected = SAMPLE.replace(
            "http://192.168.1.2:80/WebServices/ScannerService, WSD",
            "https://brother.local/eSCL, eSCL",
        );
        assert_eq!(edited, expected);
    }

    #[test]
    fn removes_only_the_named_device_row() {
        let edited = remove_device(SAMPLE, "Brother MFC-1910W").expect("the sample is editable");
        let expected = SAMPLE.replace(
            "\"Brother MFC-1910W\" = http://192.168.1.2:80/WebServices/ScannerService, WSD\n",
            "",
        );
        assert_eq!(edited, expected);
        assert!(edited.contains("#\"Retired\" = http://192.168.1.9/eSCL, eSCL"));
        // A name the file does not have leaves the text exactly as it was, so
        // the caller writes nothing at all.
        assert_eq!(remove_device(SAMPLE, "Absent").unwrap(), SAMPLE);
    }

    #[test]
    fn toggles_discovery_and_keeps_the_other_option() {
        let edited = set_discovery(SAMPLE, false).expect("the sample is editable");
        assert_eq!(
            edited,
            SAMPLE.replace("discovery = disable", "discovery = enable")
        );
        assert!(edited.contains("model-dns-sd = disable"));
        assert!(edited.contains("[blacklist]\nip = 10.0.0.0/8\n"));
        assert!(edited.starts_with("# airscan.conf - office\n"));
    }

    #[test]
    fn adds_the_options_section_when_the_file_has_none() {
        let text = "[devices]\n\"Office\" = https://scanner.local/eSCL, eSCL\n";
        let edited = set_discovery(text, true).expect("a file without [options] is editable");
        assert_eq!(
            edited,
            "[devices]\n\"Office\" = https://scanner.local/eSCL, eSCL\n\n[options]\ndiscovery = disable\n"
        );
    }

    #[test]
    fn adds_the_devices_section_and_a_final_newline_when_missing() {
        let edited = upsert_device(
            "[blacklist]\nip = 10.0.0.0/8\n",
            &device("Office", "https://scanner.local/eSCL", "eSCL"),
        )
        .expect("a file without [devices] is editable");
        assert_eq!(
            edited,
            "[blacklist]\nip = 10.0.0.0/8\n\n[devices]\n\"Office\" = https://scanner.local/eSCL, eSCL\n"
        );

        let appended = upsert_device(
            "[devices]",
            &device("Office", "https://scanner.local/eSCL", "eSCL"),
        )
        .expect("a file without a final newline is editable");
        assert_eq!(
            appended,
            "[devices]\n\"Office\" = https://scanner.local/eSCL, eSCL\n"
        );
    }

    #[test]
    fn keeps_crlf_line_endings() {
        let edited = set_discovery("[options]\r\ndiscovery = disable\r\n", false)
            .expect("a CRLF file is editable");
        assert_eq!(edited, "[options]\r\ndiscovery = enable\r\n");
    }

    #[test]
    fn refuses_duplicate_sections_and_duplicate_keys() {
        let twice = "[devices]\n[devices]\n";
        let err = upsert_device(
            twice,
            &device("Office", "https://scanner.local/eSCL", "eSCL"),
        )
        .expect_err("two [devices] sections are ambiguous");
        assert_eq!(err.code, ErrorCode::Conflict);
        assert!(err.fix.is_some());

        let twice = "[options]\ndiscovery = disable\ndiscovery = enable\n";
        let err = set_discovery(twice, true).expect_err("two discovery keys are ambiguous");
        assert_eq!(err.code, ErrorCode::Conflict);
        assert!(err.fix.is_some());
    }

    #[test]
    fn validates_manual_fields() {
        assert!(validate_name("Brother MFC").is_ok());
        assert!(validate_name("bad\nname").is_err());
        assert!(validate_address("http://scanner.local/eSCL").is_ok());
        assert!(validate_address("file:///etc/passwd").is_err());
        assert!(validate_protocol("WSD").is_ok());
        assert!(validate_protocol("raw").is_err());
    }
}
