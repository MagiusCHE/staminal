use thiserror::Error;

pub type Result<T> = std::result::Result<T, SchemaError>;

#[derive(Error, Debug)]
pub enum SchemaError {
    #[error("Failed to read file '{0}': {1}")]
    IoError(String, #[source] std::io::Error),

    #[error("JSON parse error: {0}")]
    ParseError(#[from] serde_json::Error),

    #[error("Schema validation failed: {0}")]
    ValidationError(String),
}
