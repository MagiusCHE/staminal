//! Path Security Module
//!
//! This module provides centralized path validation for all mod operations.
//! All file access from mods MUST go through these validation functions to ensure
//! scripts cannot escape their sandbox.
//!
//! # Permitted Directories
//!
//! Mods are only allowed to access files within:
//! - `data_dir` - The game data directory (contains mods, assets, saves, etc.)
//! - `config_dir` - The configuration directory (optional, for config files)
//!
//! # Security Rules
//!
//! 1. All paths are canonicalized before comparison to prevent directory traversal attacks
//! 2. Relative paths are resolved against `data_dir`
//! 3. Absolute paths must be within permitted directories
//! 4. Symlinks are followed and the real path is checked

use std::path::{Path, PathBuf};

/// Configuration for path security validation
#[derive(Clone, Debug)]
pub struct PathSecurityConfig {
    /// The game data directory (required)
    pub data_dir: PathBuf,
    /// The configuration directory (optional)
    pub config_dir: Option<PathBuf>,
}

impl PathSecurityConfig {
    /// Create a new PathSecurityConfig with only data_dir
    pub fn new(data_dir: impl Into<PathBuf>) -> Self {
        Self {
            data_dir: data_dir.into(),
            config_dir: None,
        }
    }

    /// Create a new PathSecurityConfig with both data_dir and config_dir
    pub fn with_config_dir(
        data_dir: impl Into<PathBuf>,
        config_dir: impl Into<PathBuf>,
    ) -> Self {
        Self {
            data_dir: data_dir.into(),
            config_dir: Some(config_dir.into()),
        }
    }
}

/// Result of path validation
#[derive(Debug)]
pub enum PathValidationResult {
    /// Path is valid and permitted
    Permitted(PathBuf),
    /// Path is not permitted (outside allowed directories)
    NotPermitted(String),
    /// Path does not exist
    NotFound(String),
    /// Path resolution failed
    ResolutionError(String),
}

impl PathValidationResult {
    /// Convert to Result, returning the canonical path on success
    pub fn into_result(self) -> Result<PathBuf, String> {
        match self {
            PathValidationResult::Permitted(path) => Ok(path),
            PathValidationResult::NotPermitted(msg) => Err(msg),
            PathValidationResult::NotFound(msg) => Err(msg),
            PathValidationResult::ResolutionError(msg) => Err(msg),
        }
    }

    /// Check if the path is permitted
    pub fn is_permitted(&self) -> bool {
        matches!(self, PathValidationResult::Permitted(_))
    }
}

/// Validate that a path is within permitted directories
///
/// This function handles both relative and absolute paths:
/// - Relative paths are resolved against `data_dir`
/// - Absolute paths are checked to be within `data_dir` or `config_dir`
///
/// # Arguments
/// * `path` - The path to validate (relative or absolute)
/// * `config` - The security configuration
///
/// # Returns
/// * `PathValidationResult` indicating if the path is permitted
///
/// # Security
/// This function:
/// - Canonicalizes all paths to prevent directory traversal (../)
/// - Follows symlinks and validates the real path
/// - Rejects paths outside permitted directories
pub fn validate_path(path: impl AsRef<Path>, config: &PathSecurityConfig) -> PathValidationResult {
    let path = path.as_ref();

    // Canonicalize data_dir first
    let canonical_data_dir = match config.data_dir.canonicalize() {
        Ok(p) => p,
        Err(e) => {
            return PathValidationResult::ResolutionError(format!(
                "Failed to canonicalize data_dir '{}': {}",
                config.data_dir.display(),
                e
            ));
        }
    };

    // Canonicalize config_dir if present
    let canonical_config_dir = config.config_dir.as_ref().and_then(|p| p.canonicalize().ok());

    // Resolve the path
    let resolved_path = if path.is_absolute() {
        path.to_path_buf()
    } else {
        // Relative paths are resolved against data_dir
        canonical_data_dir.join(path)
    };

    // Canonicalize the resolved path to get the real path
    let canonical_path = match resolved_path.canonicalize() {
        Ok(p) => p,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return PathValidationResult::NotFound(format!(
                "Path does not exist: {}",
                resolved_path.display()
            ));
        }
        Err(e) => {
            return PathValidationResult::ResolutionError(format!(
                "Failed to resolve path '{}': {}",
                resolved_path.display(),
                e
            ));
        }
    };

    // Check if the canonical path is within permitted directories
    if canonical_path.starts_with(&canonical_data_dir) {
        return PathValidationResult::Permitted(canonical_path);
    }

    if let Some(ref config_dir) = canonical_config_dir {
        if canonical_path.starts_with(config_dir) {
            return PathValidationResult::Permitted(canonical_path);
        }
    }

    // Path is not within permitted directories
    PathValidationResult::NotPermitted(format!(
        "Access denied: path '{}' is outside permitted directories. \
         Mods can only access files within the game data directory.",
        path.display()
    ))
}

/// Validate and resolve a path, returning the canonical path on success
///
/// This is a convenience wrapper around `validate_path` that returns a `Result`.
///
/// # Arguments
/// * `path` - The path to validate (relative or absolute)
/// * `config` - The security configuration
///
/// # Returns
/// * `Ok(PathBuf)` - The canonical path if permitted
/// * `Err(String)` - Error message if not permitted
pub fn validate_and_resolve_path(
    path: impl AsRef<Path>,
    config: &PathSecurityConfig,
) -> Result<PathBuf, String> {
    validate_path(path, config).into_result()
}

/// Check if a path is within permitted directories without returning the resolved path
///
/// This is useful for quick permission checks when you don't need the resolved path.
///
/// # Arguments
/// * `path` - The path to check
/// * `config` - The security configuration
///
/// # Returns
/// * `true` if the path is permitted
/// * `false` if the path is not permitted, doesn't exist, or couldn't be resolved
pub fn is_path_permitted(path: impl AsRef<Path>, config: &PathSecurityConfig) -> bool {
    validate_path(path, config).is_permitted()
}

/// Make a relative path absolute by joining it with data_dir
///
/// This function does NOT validate the path - use `validate_path` for that.
///
/// # Arguments
/// * `path` - The path (relative or absolute)
/// * `data_dir` - The data directory to use as base for relative paths
///
/// # Returns
/// * The absolute path
pub fn make_absolute(path: impl AsRef<Path>, data_dir: impl AsRef<Path>) -> PathBuf {
    let path = path.as_ref();
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        data_dir.as_ref().join(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn test_relative_path_within_data_dir() {
        let temp = tempdir().unwrap();
        let data_dir = temp.path().join("data");
        fs::create_dir_all(&data_dir).unwrap();
        let test_file = data_dir.join("test.txt");
        fs::write(&test_file, "test").unwrap();

        let config = PathSecurityConfig::new(&data_dir);
        let result = validate_path("test.txt", &config);

        assert!(result.is_permitted());
    }

    #[test]
    fn test_absolute_path_within_data_dir() {
        let temp = tempdir().unwrap();
        let data_dir = temp.path().join("data");
        fs::create_dir_all(&data_dir).unwrap();
        let test_file = data_dir.join("test.txt");
        fs::write(&test_file, "test").unwrap();

        let config = PathSecurityConfig::new(&data_dir);
        let result = validate_path(&test_file, &config);

        assert!(result.is_permitted());
    }

    #[test]
    fn test_path_outside_data_dir() {
        let temp = tempdir().unwrap();
        let data_dir = temp.path().join("data");
        let outside_dir = temp.path().join("outside");
        fs::create_dir_all(&data_dir).unwrap();
        fs::create_dir_all(&outside_dir).unwrap();
        let outside_file = outside_dir.join("secret.txt");
        fs::write(&outside_file, "secret").unwrap();

        let config = PathSecurityConfig::new(&data_dir);
        let result = validate_path(&outside_file, &config);

        assert!(!result.is_permitted());
    }

    #[test]
    fn test_directory_traversal_blocked() {
        let temp = tempdir().unwrap();
        let data_dir = temp.path().join("data");
        let outside_dir = temp.path().join("outside");
        fs::create_dir_all(&data_dir).unwrap();
        fs::create_dir_all(&outside_dir).unwrap();
        let outside_file = outside_dir.join("secret.txt");
        fs::write(&outside_file, "secret").unwrap();

        let config = PathSecurityConfig::new(&data_dir);
        // Try to escape with ../
        let result = validate_path("../outside/secret.txt", &config);

        assert!(!result.is_permitted());
    }

    #[test]
    fn test_path_in_config_dir() {
        let temp = tempdir().unwrap();
        let data_dir = temp.path().join("data");
        let config_dir = temp.path().join("config");
        fs::create_dir_all(&data_dir).unwrap();
        fs::create_dir_all(&config_dir).unwrap();
        let config_file = config_dir.join("settings.json");
        fs::write(&config_file, "{}").unwrap();

        let config = PathSecurityConfig::with_config_dir(&data_dir, &config_dir);
        let result = validate_path(&config_file, &config);

        assert!(result.is_permitted());
    }

    #[test]
    fn test_nonexistent_path() {
        let temp = tempdir().unwrap();
        let data_dir = temp.path().join("data");
        fs::create_dir_all(&data_dir).unwrap();

        let config = PathSecurityConfig::new(&data_dir);
        let result = validate_path("nonexistent.txt", &config);

        matches!(result, PathValidationResult::NotFound(_));
    }
}
