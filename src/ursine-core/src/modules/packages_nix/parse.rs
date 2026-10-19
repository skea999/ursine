//! Parsers for `nix profile` / `nix search` JSON output (new CLI only).

use serde::Deserialize;
use ursine_api::{ApiError, InstalledPackage, SearchResult};

/// `nix profile list --json` → sorted package list.
pub fn parse_profile_list(json: &str) -> Result<Vec<InstalledPackage>, ApiError> {
    #[derive(Deserialize)]
    struct Element {
        #[serde(default)]
        active: bool,
        #[serde(default, rename = "attrPath")]
        attr_path: Option<String>,
        #[serde(default, rename = "originalUrl")]
        original_url: Option<String>,
        #[serde(default)]
        url: Option<String>,
        #[serde(default, rename = "storePaths")]
        store_paths: Vec<String>,
    }
    #[derive(Deserialize)]
    struct ListJson {
        #[serde(default)]
        elements: std::collections::BTreeMap<String, Element>,
    }

    let parsed: ListJson = serde_json::from_str(json)
        .map_err(|e| ApiError::internal(format!("parse profile list: {e}")))?;

    let mut out: Vec<InstalledPackage> = parsed
        .elements
        .into_iter()
        .map(|(name, e)| InstalledPackage {
            name,
            attr_path: e.attr_path,
            original_url: e.original_url,
            url: e.url,
            store_paths: e.store_paths,
            active: e.active,
        })
        .collect();
    out.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(out)
}

/// `nix search nixpkgs <q> --json` → sorted results.
pub fn parse_search(json: &str) -> Result<Vec<SearchResult>, ApiError> {
    #[derive(Deserialize)]
    struct Hit {
        #[serde(default)]
        pname: Option<String>,
        #[serde(default)]
        version: Option<String>,
        #[serde(default)]
        description: Option<String>,
    }

    let parsed: std::collections::BTreeMap<String, Hit> = serde_json::from_str(json)
        .map_err(|e| ApiError::internal(format!("parse search results: {e}")))?;

    Ok(parsed
        .into_iter()
        .map(|(attr, h)| SearchResult {
            attr,
            pname: h.pname,
            version: h.version,
            description: h.description,
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    // Realistic sample of `nix profile list --json` (nix >= 2.19).
    const LIST_JSON: &str = r#"{
      "elements": {
        "hello": {
          "active": true,
          "attrPath": "packages.x86_64-linux.hello",
          "originalUrl": "flake:nixpkgs",
          "storePaths": ["/nix/store/abc-hello-2.12"],
          "url": "flake:nixpkgs#hello"
        },
        "jq-1.7.1": {
          "active": true,
          "attrPath": "packages.x86_64-linux.jq",
          "originalUrl": "flake:nixpkgs",
          "storePaths": ["/nix/store/def-jq-1.7.1"],
          "url": "flake:nixpkgs#jq"
        }
      }
    }"#;

    const SEARCH_JSON: &str = r#"{
      "legacyPackages.x86_64-linux.hello": {
        "pname": "hello",
        "version": "2.12.1",
        "description": "A program that produces a familiar, friendly greeting"
      },
      "legacyPackages.x86_64-linux.hello-wayland": {
        "pname": "hello-wayland",
        "version": "0.1.0",
        "description": "Hello world Wayland client"
      }
    }"#;

    #[test]
    fn parses_profile_list_sorted() {
        let pkgs = parse_profile_list(LIST_JSON).unwrap();
        assert_eq!(pkgs.len(), 2);
        assert_eq!(pkgs[0].name, "hello");
        assert_eq!(pkgs[1].name, "jq-1.7.1");
        assert_eq!(
            pkgs[0].attr_path.as_deref(),
            Some("packages.x86_64-linux.hello")
        );
        assert_eq!(pkgs[0].url.as_deref(), Some("flake:nixpkgs#hello"));
        assert!(pkgs[0].active);
    }

    #[test]
    fn parses_empty_profile() {
        let pkgs = parse_profile_list(r#"{"elements": {}}"#).unwrap();
        assert!(pkgs.is_empty());
    }

    #[test]
    fn parses_search_results() {
        let hits = parse_search(SEARCH_JSON).unwrap();
        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].attr, "legacyPackages.x86_64-linux.hello");
        assert_eq!(hits[0].version.as_deref(), Some("2.12.1"));
        assert!(hits[0].description.as_deref().unwrap().contains("greeting"));
    }

    #[test]
    fn malformed_json_is_internal_error() {
        let err = parse_profile_list("not json").unwrap_err();
        assert_eq!(err.code, ursine_api::ErrorCode::Internal);
        let err = parse_search("{").unwrap_err();
        assert_eq!(err.code, ursine_api::ErrorCode::Internal);
    }
}
