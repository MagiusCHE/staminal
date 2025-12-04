/// File API abstraction
///
/// Provides file system operations with path security validation.
/// All file operations validate paths against permitted directories (data_dir, config_dir).
/// This module is runtime-agnostic and can be used by JavaScript, Lua, C#, etc.

use std::path::PathBuf;
use std::fs;
use std::io::Read;

use super::path_security::validate_path_for_creation;

/// File API implementation
///
/// Provides secure file operations that validate all paths against permitted directories.
#[derive(Clone)]
pub struct FileApi {
    /// Game data directory (contains mods, assets, saves, etc.)
    data_dir: PathBuf,
    /// Game config directory (contains configuration files)
    config_dir: PathBuf,
}

/// Result of reading a JSON file
#[derive(Debug)]
pub enum ReadJsonResult {
    /// File was read and parsed successfully
    Success(String),
    /// File does not exist or is empty, default value should be used
    UseDefault,
    /// Error occurred (path security violation, invalid JSON, etc.)
    Error(String),
}

impl FileApi {
    /// Create a new FileApi with the specified directories
    ///
    /// # Arguments
    /// * `data_dir` - Game data directory (mods, assets, saves)
    /// * `config_dir` - Game config directory (configuration files)
    pub fn new(data_dir: PathBuf, config_dir: PathBuf) -> Self {
        Self { data_dir, config_dir }
    }

    /// Get the data directory
    pub fn data_dir(&self) -> &PathBuf {
        &self.data_dir
    }

    /// Get the config directory
    pub fn config_dir(&self) -> &PathBuf {
        &self.config_dir
    }

    /// Validate that a path is within one of the permitted directories
    ///
    /// # Arguments
    /// * `path` - The path to validate (can be relative or absolute)
    ///
    /// # Returns
    /// * `Ok(PathBuf)` - The validated absolute path
    /// * `Err(String)` - Error message if validation failed
    pub fn validate_path(&self, path: &str) -> Result<PathBuf, String> {
        let path = PathBuf::from(path);

        // If path is absolute, check if it's within one of the permitted directories
        if path.is_absolute() {
            // Try to validate against data_dir
            if !self.data_dir.as_os_str().is_empty() {
                if let Ok(canonical_data) = self.data_dir.canonicalize() {
                    if let Ok(canonical_path) = path.canonicalize() {
                        if canonical_path.starts_with(&canonical_data) {
                            return Ok(canonical_path);
                        }
                    }
                    // Path doesn't exist yet, check if parent is within data_dir
                    if let Some(parent) = path.parent() {
                        if let Ok(canonical_parent) = parent.canonicalize() {
                            if canonical_parent.starts_with(&canonical_data) {
                                return Ok(path);
                            }
                        }
                    }
                }
            }

            // Try to validate against config_dir
            if !self.config_dir.as_os_str().is_empty() {
                if let Ok(canonical_config) = self.config_dir.canonicalize() {
                    if let Ok(canonical_path) = path.canonicalize() {
                        if canonical_path.starts_with(&canonical_config) {
                            return Ok(canonical_path);
                        }
                    }
                    // Path doesn't exist yet, check if parent is within config_dir
                    if let Some(parent) = path.parent() {
                        if let Ok(canonical_parent) = parent.canonicalize() {
                            if canonical_parent.starts_with(&canonical_config) {
                                return Ok(path);
                            }
                        }
                    }
                }
            }

            return Err(format!(
                "Access denied: absolute path '{}' is not within permitted directories (data_dir or config_dir)",
                path.display()
            ));
        }

        // For relative paths, try data_dir first, then config_dir
        // First try data_dir
        if !self.data_dir.as_os_str().is_empty() {
            if let Ok(validated) = validate_path_for_creation(&path, &self.data_dir) {
                return Ok(validated);
            }
        }

        // Then try config_dir
        if !self.config_dir.as_os_str().is_empty() {
            if let Ok(validated) = validate_path_for_creation(&path, &self.config_dir) {
                return Ok(validated);
            }
        }

        Err(format!(
            "Access denied: path '{}' could not be validated against permitted directories. \
             Path traversal (../) that escapes the permitted directories is not allowed.",
            path.display()
        ))
    }

    /// Read a JSON file and return its contents as a JSON string
    ///
    /// # Arguments
    /// * `path` - Path to the JSON file (relative or absolute)
    /// * `encoding` - File encoding (currently only "utf-8" is supported)
    ///
    /// # Returns
    /// * `ReadJsonResult::Success(json_string)` - File was read and contains valid JSON
    /// * `ReadJsonResult::UseDefault` - File doesn't exist or is empty
    /// * `ReadJsonResult::Error(message)` - Error occurred
    ///
    /// # Security
    /// The path is validated to ensure it's within permitted directories (data_dir or config_dir).
    /// Path traversal attacks (../) are blocked.
    pub fn read_json(&self, path: &str, encoding: &str) -> ReadJsonResult {
        // Validate encoding
        let encoding_lower = encoding.to_lowercase();
        if encoding_lower != "utf-8" && encoding_lower != "utf8" {
            return ReadJsonResult::Error(format!(
                "Unsupported encoding '{}'. Only 'utf-8' is currently supported.",
                encoding
            ));
        }

        // Validate path
        let validated_path = match self.validate_path(path) {
            Ok(p) => p,
            Err(e) => return ReadJsonResult::Error(e),
        };

        // Check if file exists
        if !validated_path.exists() {
            return ReadJsonResult::UseDefault;
        }

        // Check if it's a file (not a directory)
        if !validated_path.is_file() {
            return ReadJsonResult::Error(format!(
                "Path '{}' is not a file",
                path
            ));
        }

        // Read file contents
        let mut file = match fs::File::open(&validated_path) {
            Ok(f) => f,
            Err(e) => return ReadJsonResult::Error(format!(
                "Failed to open file '{}': {}",
                path, e
            )),
        };

        let mut contents = String::new();
        if let Err(e) = file.read_to_string(&mut contents) {
            return ReadJsonResult::Error(format!(
                "Failed to read file '{}': {}",
                path, e
            ));
        }

        // Check if file is empty or contains only whitespace
        let trimmed = contents.trim();
        if trimmed.is_empty() {
            return ReadJsonResult::UseDefault;
        }

        // Validate JSON syntax
        // We use serde_json to parse and validate, then return the original string
        // This ensures the JSON is valid before returning
        match serde_json::from_str::<serde_json::Value>(trimmed) {
            Ok(_) => ReadJsonResult::Success(trimmed.to_string()),
            Err(e) => ReadJsonResult::Error(format!(
                "Invalid JSON in file '{}': {}",
                path, e
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;
    use std::io::Write;

    #[test]
    fn test_read_json_valid_file() {
        let temp = tempdir().unwrap();
        let data_dir = temp.path().to_path_buf();
        let config_dir = temp.path().join("config");
        fs::create_dir_all(&config_dir).unwrap();

        // Create a valid JSON file
        let json_file = data_dir.join("test.json");
        let mut file = fs::File::create(&json_file).unwrap();
        writeln!(file, r#"{{"name": "test", "value": 42}}"#).unwrap();

        let api = FileApi::new(data_dir.clone(), config_dir);

        // Read using relative path
        match api.read_json("test.json", "utf-8") {
            ReadJsonResult::Success(json) => {
                assert!(json.contains("\"name\""));
                assert!(json.contains("\"test\""));
            }
            other => panic!("Expected Success, got {:?}", other),
        }
    }

    #[test]
    fn test_read_json_nonexistent_file() {
        let temp = tempdir().unwrap();
        let api = FileApi::new(temp.path().to_path_buf(), temp.path().join("config"));

        match api.read_json("nonexistent.json", "utf-8") {
            ReadJsonResult::UseDefault => {}
            other => panic!("Expected UseDefault, got {:?}", other),
        }
    }

    #[test]
    fn test_read_json_empty_file() {
        let temp = tempdir().unwrap();
        let data_dir = temp.path().to_path_buf();
        let config_dir = temp.path().join("config");
        fs::create_dir_all(&config_dir).unwrap();

        // Create an empty file
        let json_file = data_dir.join("empty.json");
        fs::File::create(&json_file).unwrap();

        let api = FileApi::new(data_dir.clone(), config_dir);

        match api.read_json("empty.json", "utf-8") {
            ReadJsonResult::UseDefault => {}
            other => panic!("Expected UseDefault, got {:?}", other),
        }
    }

    #[test]
    fn test_read_json_invalid_json() {
        let temp = tempdir().unwrap();
        let data_dir = temp.path().to_path_buf();
        let config_dir = temp.path().join("config");
        fs::create_dir_all(&config_dir).unwrap();

        // Create a file with invalid JSON
        let json_file = data_dir.join("invalid.json");
        let mut file = fs::File::create(&json_file).unwrap();
        writeln!(file, "not valid json {{").unwrap();

        let api = FileApi::new(data_dir.clone(), config_dir);

        match api.read_json("invalid.json", "utf-8") {
            ReadJsonResult::Error(msg) => {
                assert!(msg.contains("Invalid JSON"));
            }
            other => panic!("Expected Error, got {:?}", other),
        }
    }

    #[test]
    fn test_path_traversal_blocked() {
        let temp = tempdir().unwrap();
        let data_dir = temp.path().join("data");
        fs::create_dir_all(&data_dir).unwrap();
        let config_dir = temp.path().join("config");
        fs::create_dir_all(&config_dir).unwrap();

        let api = FileApi::new(data_dir, config_dir);

        // Attempt path traversal
        match api.read_json("../../../etc/passwd", "utf-8") {
            ReadJsonResult::Error(msg) => {
                assert!(msg.contains("Access denied") || msg.contains("escapes"));
            }
            other => panic!("Expected Error for path traversal, got {:?}", other),
        }
    }
}
