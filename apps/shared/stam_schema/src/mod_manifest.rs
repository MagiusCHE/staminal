use serde::{Deserialize, Serialize};
use schemars::JsonSchema;
use schemars::r#gen::SchemaGenerator;
use schemars::schema::{Schema, SchemaObject, SingleOrVec, InstanceType};
use std::collections::HashMap;
use crate::Validatable;

/// Wrapper type for execute_on that can be either a string or array of strings
/// This type handles both JSON Schema generation and serde deserialization
#[derive(Debug, Clone, Default)]
pub struct StringOrArray(pub Vec<String>);

impl StringOrArray {
    /// Check if the array contains a specific value
    pub fn contains(&self, value: &str) -> bool {
        self.0.iter().any(|s| s == value)
    }

    /// Get an iterator over the strings
    pub fn iter(&self) -> impl Iterator<Item = &String> {
        self.0.iter()
    }
}

impl JsonSchema for StringOrArray {
    fn schema_name() -> String {
        "StringOrArray".to_string()
    }

    fn json_schema(_gen: &mut SchemaGenerator) -> Schema {
        // Create a schema that accepts either a string or array of strings
        let string_schema = SchemaObject {
            instance_type: Some(SingleOrVec::Single(Box::new(InstanceType::String))),
            ..Default::default()
        };

        let array_schema = SchemaObject {
            instance_type: Some(SingleOrVec::Single(Box::new(InstanceType::Array))),
            array: Some(Box::new(schemars::schema::ArrayValidation {
                items: Some(SingleOrVec::Single(Box::new(Schema::Object(SchemaObject {
                    instance_type: Some(SingleOrVec::Single(Box::new(InstanceType::String))),
                    ..Default::default()
                })))),
                ..Default::default()
            })),
            ..Default::default()
        };

        Schema::Object(SchemaObject {
            subschemas: Some(Box::new(schemars::schema::SubschemaValidation {
                any_of: Some(vec![
                    Schema::Object(string_schema),
                    Schema::Object(array_schema),
                ]),
                ..Default::default()
            })),
            ..Default::default()
        })
    }
}

impl Serialize for StringOrArray {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        self.0.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for StringOrArray {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        use serde::de::{self, Visitor};

        struct StringOrArrayVisitor;

        impl<'de> Visitor<'de> for StringOrArrayVisitor {
            type Value = StringOrArray;

            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("a string or array of strings")
            }

            fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                Ok(StringOrArray(vec![value.to_string()]))
            }

            fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
            where
                A: de::SeqAccess<'de>,
            {
                let mut values = Vec::new();
                while let Some(value) = seq.next_element::<String>()? {
                    values.push(value);
                }
                Ok(StringOrArray(values))
            }
        }

        deserializer.deserialize_any(StringOrArrayVisitor)
    }
}

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
    /// Optional - mods without entry_point are automatically considered "attached"
    /// (they provide assets/resources only, no executable code)
    #[schemars(description = "Main entry point file for the mod runtime. Optional - mods without entry_point are asset-only.")]
    #[serde(default)]
    pub entry_point: Option<String>,

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
    #[schemars(description = "Dependencies: mod-id -> version constraint. Use '@client', '@server' or '@game' for engine/game requirements.")]
    #[serde(default)]
    pub requires: HashMap<String, String>,

    /// Where this mod should execute: "server", "client", or both
    /// Can be a single string or an array of strings
    #[schemars(description = "Where this mod executes: 'server', 'client', or ['server', 'client'] for both")]
    #[serde(default)]
    pub execute_on: StringOrArray,
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
        assert_eq!(manifest.entry_point, Some("index.js".to_string()));
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

    #[test]
    fn test_manifest_without_entry_point() {
        // Asset-only mods don't need an entry_point
        let json = r#"{
            "name": "asset-pack",
            "version": "1.0.0",
            "description": "An asset-only mod"
        }"#;

        let manifest = ModManifest::from_json_str(json).unwrap();
        assert_eq!(manifest.name, "asset-pack");
        assert_eq!(manifest.version, "1.0.0");
        assert_eq!(manifest.entry_point, None);
    }
}
