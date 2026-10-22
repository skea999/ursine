//! Result types for the `packages.nix` module (nix profile, new CLI only).
//!
//! Also owns the pure installable policy shared by daemon and frontends, so a
//! prompt or scope decision cannot drift from what the daemon executes.

use serde::{Deserialize, Serialize};

use crate::ApiError;

/// One element of `nix profile list --json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstalledPackage {
    /// Profile element name (e.g. `hello` or `hello-2.12`).
    pub name: String,
    /// Attribute path when installable came from a flake (e.g. `packages.x86_64-linux.hello`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attr_path: Option<String>,
    /// Original flake reference as written at install time (e.g. `flake:nixpkgs`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub original_url: Option<String>,
    /// Resolved installable URL (e.g. `flake:nixpkgs#hello`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// Store paths owned by this element.
    #[serde(default)]
    pub store_paths: Vec<String>,
    /// Whether the element is currently active.
    #[serde(default)]
    pub active: bool,
}

/// One result row of `nix search nixpkgs <query> --json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchResult {
    /// Attribute path (e.g. `legacyPackages.x86_64-linux.hello`).
    pub attr: String,
    /// Installable accepted by `nix profile add` (e.g. `hello`), normalized by
    /// the daemon so the frontend never has to understand flake attr layouts.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub installable: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pname: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// Outcome of a mutating profile command (install/remove/upgrade/...).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommandOutcome {
    pub stdout: String,
    pub stderr: String,
}

/// Charset accepted in flake refs / attr names: no shell metacharacters.
pub fn is_safe_installable_atom(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 512
        && value.chars().all(|c| {
            c.is_ascii_alphanumeric()
                || matches!(c, '.' | '_' | '+' | '-' | ':' | '/' | '#' | '?' | '=')
        })
}

/// Validate one installable exactly as the daemon does.
pub fn validate_installable(arg: &str) -> Result<(), ApiError> {
    if arg.starts_with('-') || !is_safe_installable_atom(arg) {
        return Err(ApiError::invalid("invalid package reference"));
    }
    Ok(())
}

/// Normalize an installable: bare attr → `nixpkgs#attr`; flake refs pass
/// through (same accepted charset). Path installs are rejected in v1.
pub fn normalize_installable(arg: &str) -> Result<String, ApiError> {
    validate_installable(arg)?;
    if arg.starts_with('.') || arg.starts_with('/') {
        return Err(ApiError::invalid(
            "path installs are not supported; use nixpkgs#name",
        ));
    }
    Ok(if arg.contains('#') {
        arg.to_string()
    } else {
        format!("nixpkgs#{arg}")
    })
}

/// Convert a flake search attr into the ref `nix profile add` accepts.
///
/// Search returns `legacyPackages.<system>.<attr>` (or `packages.<system>.<attr>`);
/// both drop the `legacyPackages`/`packages` and system prefixes. Other shapes
/// pass through unchanged.
pub fn installable_from_attr(attr: &str) -> String {
    let mut parts = attr.splitn(3, '.');
    match (parts.next(), parts.next(), parts.next()) {
        (Some("legacyPackages" | "packages"), Some(_system), Some(rest)) if !rest.is_empty() => {
            rest.to_string()
        }
        _ => attr.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn installed_package_roundtrip() {
        let p = InstalledPackage {
            name: "hello".into(),
            attr_path: Some("packages.x86_64-linux.hello".into()),
            original_url: Some("flake:nixpkgs".into()),
            url: Some("flake:nixpkgs#hello".into()),
            store_paths: vec!["/nix/store/abc-hello-2.12".into()],
            active: true,
        };
        let s = serde_json::to_string(&p).unwrap();
        assert_eq!(serde_json::from_str::<InstalledPackage>(&s).unwrap(), p);
    }

    #[test]
    fn search_result_skips_none() {
        let r = SearchResult {
            attr: "hello".into(),
            installable: None,
            pname: None,
            version: None,
            description: None,
        };
        let v = serde_json::to_value(&r).unwrap();
        assert!(!v.as_object().unwrap().contains_key("pname"));
        assert!(!v.as_object().unwrap().contains_key("installable"));
        assert_eq!(v["attr"], "hello");
    }
}
