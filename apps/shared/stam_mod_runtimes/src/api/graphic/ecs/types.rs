//! ECS Types and Structures
//!
//! Defines the core types used by the ECS API.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Schema for a custom component type defined by mods
///
/// Components must be registered with a schema before they can be used.
/// The schema defines the fields and their types for validation.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ComponentSchema {
    /// Component type name (e.g., "Player", "Velocity")
    pub name: String,
    /// Field definitions
    pub fields: HashMap<String, FieldType>,
}

impl ComponentSchema {
    /// Create a new component schema
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            fields: HashMap::new(),
        }
    }

    /// Add a field to the schema
    pub fn with_field(mut self, name: impl Into<String>, field_type: FieldType) -> Self {
        self.fields.insert(name.into(), field_type);
        self
    }

    /// Validate component data against this schema
    pub fn validate(&self, data: &serde_json::Value) -> Result<(), String> {
        let obj = data
            .as_object()
            .ok_or_else(|| format!("Component '{}' data must be an object", self.name))?;

        for (field_name, field_type) in &self.fields {
            if let Some(value) = obj.get(field_name) {
                field_type.validate(value).map_err(|e| {
                    format!("Component '{}' field '{}': {}", self.name, field_name, e)
                })?;
            }
            // Note: We don't require all fields to be present - missing fields are allowed
        }

        Ok(())
    }
}

/// Field type for component schema validation
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum FieldType {
    /// A number (f64)
    Number,
    /// A string
    String,
    /// A boolean
    Bool,
    /// A 2D vector { x, y }
    Vec2,
    /// A 3D vector { x, y, z }
    Vec3,
    /// A color { r, g, b, a } (values 0-1) or hex string
    Color,
    /// An entity reference (u64 ID)
    Entity,
    /// An array of values
    Array {
        /// Element type
        element: Box<FieldType>,
    },
    /// A nested object with defined fields
    Object {
        /// Field definitions
        fields: HashMap<String, FieldType>,
    },
    /// Any JSON value (no validation)
    Any,
}

impl FieldType {
    /// Validate a JSON value against this field type
    pub fn validate(&self, value: &serde_json::Value) -> Result<(), String> {
        match self {
            FieldType::Number => {
                if !value.is_number() {
                    return Err("expected a number".to_string());
                }
            }
            FieldType::String => {
                if !value.is_string() {
                    return Err("expected a string".to_string());
                }
            }
            FieldType::Bool => {
                if !value.is_boolean() {
                    return Err("expected a boolean".to_string());
                }
            }
            FieldType::Vec2 => {
                let obj = value
                    .as_object()
                    .ok_or_else(|| "expected an object with x, y fields".to_string())?;
                if !obj.contains_key("x") || !obj.contains_key("y") {
                    return Err("expected an object with x, y fields".to_string());
                }
                if !obj["x"].is_number() || !obj["y"].is_number() {
                    return Err("x and y must be numbers".to_string());
                }
            }
            FieldType::Vec3 => {
                let obj = value
                    .as_object()
                    .ok_or_else(|| "expected an object with x, y, z fields".to_string())?;
                if !obj.contains_key("x") || !obj.contains_key("y") || !obj.contains_key("z") {
                    return Err("expected an object with x, y, z fields".to_string());
                }
                if !obj["x"].is_number() || !obj["y"].is_number() || !obj["z"].is_number() {
                    return Err("x, y, and z must be numbers".to_string());
                }
            }
            FieldType::Color => {
                // Accept either a hex string or an object with r, g, b, a
                if value.is_string() {
                    // Hex color validation
                    let s = value.as_str().unwrap();
                    if !s.starts_with('#') || (s.len() != 7 && s.len() != 9) {
                        return Err(
                            "expected a hex color string (#RRGGBB or #RRGGBBAA)".to_string()
                        );
                    }
                } else if let Some(obj) = value.as_object() {
                    if !obj.contains_key("r") || !obj.contains_key("g") || !obj.contains_key("b") {
                        return Err(
                            "expected an object with r, g, b fields (a is optional)".to_string()
                        );
                    }
                } else {
                    return Err("expected a hex string or { r, g, b, a } object".to_string());
                }
            }
            FieldType::Entity => {
                if !value.is_u64() {
                    return Err("expected an entity ID (number)".to_string());
                }
            }
            FieldType::Array { element } => {
                let arr = value
                    .as_array()
                    .ok_or_else(|| "expected an array".to_string())?;
                for (i, item) in arr.iter().enumerate() {
                    element
                        .validate(item)
                        .map_err(|e| format!("element [{}]: {}", i, e))?;
                }
            }
            FieldType::Object { fields } => {
                let obj = value
                    .as_object()
                    .ok_or_else(|| "expected an object".to_string())?;
                for (field_name, field_type) in fields {
                    if let Some(field_value) = obj.get(field_name) {
                        field_type
                            .validate(field_value)
                            .map_err(|e| format!("field '{}': {}", field_name, e))?;
                    }
                }
            }
            FieldType::Any => {
                // Any value is valid
            }
        }
        Ok(())
    }
}

/// Result of a query operation
///
/// Contains the entity ID and all requested component data.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct QueryResult {
    /// The entity's script-facing ID
    pub entity_id: u64,
    /// Component data (component_name -> JSON data)
    pub components: HashMap<String, serde_json::Value>,
}

impl QueryResult {
    /// Create a new query result
    pub fn new(entity_id: u64) -> Self {
        Self {
            entity_id,
            components: HashMap::new(),
        }
    }

    /// Add component data to the result
    pub fn with_component(mut self, name: impl Into<String>, data: serde_json::Value) -> Self {
        self.components.insert(name.into(), data);
        self
    }

    /// Get component data by name
    pub fn get(&self, name: &str) -> Option<&serde_json::Value> {
        self.components.get(name)
    }
}

/// Options for entity queries
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct QueryOptions {
    /// Components that entities must have
    pub with_components: Vec<String>,
    /// Components that entities must NOT have
    pub without_components: Vec<String>,
    /// Limit the number of results
    pub limit: Option<usize>,
}

impl QueryOptions {
    /// Create new empty query options
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a required component
    pub fn with(mut self, component: impl Into<String>) -> Self {
        self.with_components.push(component.into());
        self
    }

    /// Add an excluded component
    pub fn without(mut self, component: impl Into<String>) -> Self {
        self.without_components.push(component.into());
        self
    }

    /// Set the result limit
    pub fn limit(mut self, limit: usize) -> Self {
        self.limit = Some(limit);
        self
    }
}

/// Information about a spawned entity
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EntityInfo {
    /// The entity's script-facing ID
    pub entity_id: u64,
    /// The mod that owns this entity
    pub owner_mod: String,
}

/// Predefined system behaviors that can be used in declared systems
///
/// These behaviors are implemented in Rust and execute efficiently
/// without crossing the JS/Rust boundary every frame.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum SystemBehavior {
    /// Add Velocity to Transform every frame
    /// Required components: Transform, Velocity
    ApplyVelocity,

    /// Apply gravity to Velocity
    /// Required components: Velocity
    /// Config: strength (f32), direction ("down" | "up" | {x, y})
    ApplyGravity,

    /// Reduce Velocity over time (friction/drag)
    /// Required components: Velocity
    /// Config: factor (f32, 0-1, how much to retain per second)
    ApplyFriction,

    /// Increment a numeric field over time
    /// Config: field (string), rate (f32, per second), max_field (optional string)
    RegenerateOverTime,

    /// Decrement a numeric field over time
    /// Config: field (string), rate (f32, per second), min_field (optional string)
    DecayOverTime,

    /// Move entity towards another entity
    /// Config: speed_field (string), target_field (string, entity ID field)
    FollowEntity,

    /// Orbit around a point or entity
    /// Config: center (Vec2 or entity field), radius (f32), speed (f32)
    OrbitAround,

    /// Bounce when hitting bounds
    /// Config: bounds (Rect or window), damping (f32, optional)
    BounceOnBounds,

    /// Despawn entity when a field reaches zero
    /// Config: field (string)
    DespawnWhenZero,

    /// Cycle through sprite animation frames
    /// Config: frames (array of paths), frame_time (f32)
    AnimateSprite,
}

/// Configuration for a declared system
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DeclaredSystem {
    /// Unique name for this system
    pub name: String,
    /// Query to select entities
    pub query: QueryOptions,
    /// Behavior to apply (mutually exclusive with formulas)
    pub behavior: Option<SystemBehavior>,
    /// Configuration for the behavior
    pub config: Option<serde_json::Value>,
    /// Mathematical formulas to evaluate (mutually exclusive with behavior)
    pub formulas: Option<Vec<String>>,
    /// Whether this system is enabled
    pub enabled: bool,
    /// Execution order (lower runs first)
    pub order: i32,
}

impl DeclaredSystem {
    /// Create a new declared system with a behavior
    pub fn with_behavior(
        name: impl Into<String>,
        query: QueryOptions,
        behavior: SystemBehavior,
    ) -> Self {
        Self {
            name: name.into(),
            query,
            behavior: Some(behavior),
            config: None,
            formulas: None,
            enabled: true,
            order: 0,
        }
    }

    /// Create a new declared system with formulas
    pub fn with_formulas(
        name: impl Into<String>,
        query: QueryOptions,
        formulas: Vec<String>,
    ) -> Self {
        Self {
            name: name.into(),
            query,
            behavior: None,
            config: None,
            formulas: Some(formulas),
            enabled: true,
            order: 0,
        }
    }

    /// Add configuration
    pub fn with_config(mut self, config: serde_json::Value) -> Self {
        self.config = Some(config);
        self
    }

    /// Set execution order
    pub fn with_order(mut self, order: i32) -> Self {
        self.order = order;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_field_type_number_validation() {
        assert!(FieldType::Number.validate(&json!(42)).is_ok());
        assert!(FieldType::Number.validate(&json!(3.14)).is_ok());
        assert!(FieldType::Number.validate(&json!("hello")).is_err());
    }

    #[test]
    fn test_field_type_vec2_validation() {
        assert!(FieldType::Vec2.validate(&json!({"x": 1, "y": 2})).is_ok());
        assert!(FieldType::Vec2.validate(&json!({"x": 1})).is_err());
        assert!(FieldType::Vec2
            .validate(&json!({"x": "a", "y": "b"}))
            .is_err());
    }

    #[test]
    fn test_field_type_color_validation() {
        assert!(FieldType::Color.validate(&json!("#FF0000")).is_ok());
        assert!(FieldType::Color.validate(&json!("#FF0000FF")).is_ok());
        assert!(FieldType::Color
            .validate(&json!({"r": 1, "g": 0, "b": 0}))
            .is_ok());
        assert!(FieldType::Color.validate(&json!("red")).is_err());
    }

    #[test]
    fn test_component_schema_validation() {
        let schema = ComponentSchema::new("Player")
            .with_field("health", FieldType::Number)
            .with_field("name", FieldType::String)
            .with_field("position", FieldType::Vec2);

        let valid_data = json!({
            "health": 100,
            "name": "Hero",
            "position": {"x": 0, "y": 0}
        });
        assert!(schema.validate(&valid_data).is_ok());

        // Missing fields are OK
        let partial_data = json!({"health": 50});
        assert!(schema.validate(&partial_data).is_ok());

        // Wrong type should fail
        let invalid_data = json!({"health": "not a number"});
        assert!(schema.validate(&invalid_data).is_err());
    }

    #[test]
    fn test_query_options_builder() {
        let opts = QueryOptions::new()
            .with("Transform")
            .with("Velocity")
            .without("Frozen")
            .limit(10);

        assert_eq!(opts.with_components, vec!["Transform", "Velocity"]);
        assert_eq!(opts.without_components, vec!["Frozen"]);
        assert_eq!(opts.limit, Some(10));
    }
}
