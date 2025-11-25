use serde::{Deserialize, Serialize};
use schemars::JsonSchema;
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
}

impl Validatable for ModManifest {}

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
