//! Result types for the `packages.nix` module (nix profile, new CLI only).

use serde::{Deserialize, Serialize};

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
            pname: None,
            version: None,
            description: None,
        };
        let v = serde_json::to_value(&r).unwrap();
        assert!(!v.as_object().unwrap().contains_key("pname"));
        assert_eq!(v["attr"], "hello");
    }
}
