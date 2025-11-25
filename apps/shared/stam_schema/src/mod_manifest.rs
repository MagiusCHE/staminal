use serde::{Deserialize, Serialize};
use schemars::JsonSchema;
use std::collections::HashMap;
use crate::Validatable;

/// Mod manifest structure (manifest.json)
/// This defines the metadata for a mod package
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[schemars(title = "Mod Manifest")]
#[schemars(description = "Manifest file for a Staminal mod package")]
pub struct ModManifest {
    /// Human-readable name of the mod
    #[schemars(description = "Display name of the mod")]
    pub name: String,

    /// Semantic version of the mod (e.g., "1.0.0")
    #[schemars(description = "Mod version in semver format (major.minor.patch)")]
    pub version: String,

    /// Description of what the mod does
    #[schemars(description = "Brief description of the mod's purpose")]
    pub description: String,

    /// Entry point file for the mod (e.g., "index.js")
    #[schemars(description = "Main entry point file for the mod runtime")]
    pub entry_point: String,

    /// Load priority (lower numbers load first)
    #[schemars(description = "Loading priority - lower values load earlier")]
    #[serde(default)]
    pub priority: i32,

    /// Mod type: "bootstrap" or "library"
    #[schemars(description = "Mod type: 'bootstrap' (entry point) or 'library' (helper)")]
    #[serde(rename = "type", default)]
    pub mod_type: Option<String>,

    /// Dependencies on other mods, client, server, or game
    /// Key is mod-id (or "@client"/"@server"/"@game" for engine/game version requirements)
    /// Value is version constraint: "1.0.0" for exact, "1.0.0,2.0.0" for range (min,max)
    #[schemars(description = "Dependencies: mod-id -> version constraint. Use 'client' or 'server' for engine requirements.")]
    #[serde(default)]
    pub requires: HashMap<String, String>,
}

impl Validatable for ModManifest {}

/// Parse a version requirement string
/// Returns (min_version, max_version) tuple
/// If no comma, min == max (exact version)
pub fn parse_version_requirement(requirement: &str) -> (String, String) {
    if let Some((min, max)) = requirement.split_once(',') {
        (min.trim().to_string(), max.trim().to_string())
    } else {
        let exact = requirement.trim().to_string();
        (exact.clone(), exact)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_valid_manifest() {
        let json = r#"{
            "name": "test-mod",
            "version": "1.0.0",
            "description": "A test mod",
            "entry_point": "index.js",
            "priority": 0
        }"#;

        let manifest = ModManifest::from_json_str(json).unwrap();
        assert_eq!(manifest.name, "test-mod");
        assert_eq!(manifest.version, "1.0.0");
        assert_eq!(manifest.entry_point, "index.js");
    }

    #[test]
    fn test_manifest_default_priority() {
        let json = r#"{
            "name": "test-mod",
            "version": "1.0.0",
            "description": "A test mod",
            "entry_point": "index.js"
        }"#;

        let manifest = ModManifest::from_json_str(json).unwrap();
        assert_eq!(manifest.priority, 0);
    }

    #[test]
    fn test_manifest_with_requires() {
        let json = r#"{
            "name": "test-mod",
            "version": "1.0.0",
            "description": "A test mod",
            "entry_point": "index.js",
            "type": "bootstrap",
            "requires": {
                "@client": "1.0.0",
                "@server": "1.0.0,2.0.0",
                "js-helper": "1.0.0"
            }
        }"#;

        let manifest = ModManifest::from_json_str(json).unwrap();
        assert_eq!(manifest.mod_type, Some("bootstrap".to_string()));
        assert_eq!(manifest.requires.get("@client"), Some(&"1.0.0".to_string()));
        assert_eq!(manifest.requires.get("@server"), Some(&"1.0.0,2.0.0".to_string()));
        assert_eq!(manifest.requires.get("js-helper"), Some(&"1.0.0".to_string()));
    }

    #[test]
    fn test_parse_version_requirement_exact() {
        let (min, max) = parse_version_requirement("1.0.0");
        assert_eq!(min, "1.0.0");
        assert_eq!(max, "1.0.0");
    }

    #[test]
    fn test_parse_version_requirement_range() {
        let (min, max) = parse_version_requirement("1.0.0,2.0.0");
        assert_eq!(min, "1.0.0");
        assert_eq!(max, "2.0.0");
    }

    #[test]
    fn test_parse_version_requirement_range_with_spaces() {
        let (min, max) = parse_version_requirement("1.0.0, 2.0.0");
        assert_eq!(min, "1.0.0");
        assert_eq!(max, "2.0.0");
    }

    #[test]
    fn test_invalid_manifest_missing_name() {
        let json = r#"{
            "version": "1.0.0",
            "description": "A test mod",
            "entry_point": "index.js"
        }"#;

        let result = ModManifest::from_json_str(json);
        assert!(result.is_err());
    }
}
