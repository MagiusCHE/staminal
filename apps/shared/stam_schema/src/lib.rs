use serde::Deserialize;
use schemars::JsonSchema;
use std::fs;

pub mod error;
pub mod mod_manifest;

pub use error::{SchemaError, Result};
pub use mod_manifest::{ModManifest, parse_version_requirement};

/// Trait for types that can be validated against JSON Schema
pub trait Validatable: JsonSchema + for<'de> Deserialize<'de> {
    /// Load and validate from JSON file
    fn from_json_file(path: &str) -> Result<Self> {
        let content = fs::read_to_string(path)
            .map_err(|e| SchemaError::IoError(path.to_string(), e))?;

        Self::from_json_str(&content)
    }

    /// Load and validate from JSON string
    fn from_json_str(json: &str) -> Result<Self> {
        // First deserialize
        let value: serde_json::Value = serde_json::from_str(json)
            .map_err(SchemaError::ParseError)?;

        // Then validate against schema
        let schema = schemars::schema_for!(Self);
        let schema_json = serde_json::to_value(&schema)
            .map_err(SchemaError::ParseError)?;

        let compiled = jsonschema::validator_for(&schema_json)
            .map_err(|e| SchemaError::ValidationError(e.to_string()))?;

        // Validate the JSON against the schema
        compiled.validate(&value)
            .map_err(|e| SchemaError::ValidationError(format!("{}", e)))?;

        // Finally deserialize to target type
        serde_json::from_value(value)
            .map_err(SchemaError::ParseError)
    }

    /// Generate JSON Schema for this type
    fn generate_schema() -> schemars::schema::RootSchema {
        schemars::schema_for!(Self)
    }

    /// Generate JSON Schema as JSON string
    fn schema_json() -> Result<String> {
        let schema = Self::generate_schema();
        serde_json::to_string_pretty(&schema)
            .map_err(SchemaError::ParseError)
    }
}
