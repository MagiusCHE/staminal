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
//!
//! # @mod-id Path Resolution
//!
//! Paths starting with `@mod-id/` are resolved to the mod's root directory:
//! - `@other-mod/assets/image.png` → `mods/other-mod/assets/image.png`
//! - Regular paths are resolved relative to the current mod or data_dir
//!
//! Use `resolve_mod_path()` for path resolution with @mod-id support.

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

/// Validate that a path would be within a permitted directory, even if the path doesn't exist yet.
///
/// This is useful for validating paths before creating files or directories.
/// Unlike `validate_path`, this function does NOT require the path to exist.
///
/// # Security
/// This function:
/// - Normalizes the path by resolving `.` and `..` components
/// - Checks if the normalized path starts with the base directory
/// - Does NOT follow symlinks (the path may not exist)
/// - Rejects paths that would escape the base directory via `..`
///
/// # Arguments
/// * `relative_path` - The relative path to validate (must not start with `/`)
/// * `base_dir` - The base directory that the path must stay within
///
/// # Returns
/// * `Ok(PathBuf)` - The full absolute path if valid
/// * `Err(String)` - Error message if the path would escape the base directory
pub fn validate_path_for_creation(
    relative_path: impl AsRef<Path>,
    base_dir: impl AsRef<Path>,
) -> Result<PathBuf, String> {
    let relative_path = relative_path.as_ref();
    let base_dir = base_dir.as_ref();

    // Reject absolute paths
    if relative_path.is_absolute() {
        return Err(format!(
            "Absolute paths are not allowed: {}",
            relative_path.display()
        ));
    }

    // Canonicalize base_dir to get the real absolute path
    let canonical_base = base_dir.canonicalize().map_err(|e| {
        format!(
            "Failed to canonicalize base directory '{}': {}",
            base_dir.display(),
            e
        )
    })?;

    // Build the full path
    let full_path = canonical_base.join(relative_path);

    // Normalize the path by resolving `.` and `..` components without requiring existence
    let normalized = normalize_path_components(&full_path);

    // Check that the normalized path starts with the base directory
    if !normalized.starts_with(&canonical_base) {
        return Err(format!(
            "Access denied: path '{}' escapes the permitted directory '{}'. \
             Path traversal (../) is not allowed.",
            relative_path.display(),
            base_dir.display()
        ));
    }

    Ok(normalized)
}

/// Normalize a path by resolving `.` and `..` components without requiring the path to exist.
///
/// Unlike `std::fs::canonicalize`, this function:
/// - Does NOT require the path to exist
/// - Does NOT follow symlinks
/// - Only resolves `.` (current dir) and `..` (parent dir) components
fn normalize_path_components(path: &Path) -> PathBuf {
    use std::path::Component;

    let mut normalized = PathBuf::new();

    for component in path.components() {
        match component {
            Component::CurDir => {
                // Skip `.` components
            }
            Component::ParentDir => {
                // Go up one directory if possible
                normalized.pop();
            }
            _ => {
                normalized.push(component);
            }
        }
    }

    normalized
}

// ============================================================================
// @mod-id Path Resolution
// ============================================================================

/// Result of parsing a path with @mod-id syntax
#[derive(Debug, Clone, PartialEq)]
pub enum ParsedModPath {
    /// Path references another mod: @mod-id/path/to/file
    ModReference {
        /// The mod ID (without @)
        mod_id: String,
        /// The remaining path after mod-id/
        path: String,
    },
    /// Regular path (no @mod-id prefix)
    Regular(String),
}

/// Parse a path that may contain @mod-id syntax
///
/// # Arguments
/// * `path` - The path to parse
///
/// # Returns
/// * `ParsedModPath::ModReference` if path starts with @mod-id/
/// * `ParsedModPath::Regular` for all other paths
///
/// # Examples
/// ```ignore
/// parse_mod_path("@other-mod/assets/image.png") // ModReference { mod_id: "other-mod", path: "assets/image.png" }
/// parse_mod_path("assets/image.png")             // Regular("assets/image.png")
/// parse_mod_path("@invalid")                     // Regular("@invalid") - no slash, treated as regular
/// ```
pub fn parse_mod_path(path: &str) -> ParsedModPath {
    if !path.starts_with('@') {
        return ParsedModPath::Regular(path.to_string());
    }

    // Remove @ prefix
    let without_at = &path[1..];

    // Split on first /
    if let Some(slash_pos) = without_at.find('/') {
        let mod_id = &without_at[..slash_pos];
        let remaining = &without_at[slash_pos + 1..];

        // mod_id must not be empty
        if mod_id.is_empty() {
            return ParsedModPath::Regular(path.to_string());
        }

        ParsedModPath::ModReference {
            mod_id: mod_id.to_string(),
            path: remaining.to_string(),
        }
    } else {
        // No slash after @mod-id, treat as regular path
        ParsedModPath::Regular(path.to_string())
    }
}

/// Configuration for mod path resolution
pub struct ModPathConfig {
    /// The home/data directory (contains mods/ subdirectory)
    pub home_dir: PathBuf,
    /// The current mod ID (for resolving relative paths)
    pub current_mod_id: Option<String>,
    /// Optional config directory (for config files)
    pub config_dir: Option<PathBuf>,
    /// Callback to check if a mod exists (mod_id -> bool)
    /// If None, mod existence is not checked
    mod_exists_fn: Option<Box<dyn Fn(&str) -> bool + Send + Sync>>,
}

impl std::fmt::Debug for ModPathConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ModPathConfig")
            .field("home_dir", &self.home_dir)
            .field("current_mod_id", &self.current_mod_id)
            .field("config_dir", &self.config_dir)
            .field("mod_exists_fn", &self.mod_exists_fn.as_ref().map(|_| "<fn>"))
            .finish()
    }
}

impl ModPathConfig {
    /// Create a new ModPathConfig with just the home directory
    pub fn new(home_dir: impl Into<PathBuf>) -> Self {
        Self {
            home_dir: home_dir.into(),
            current_mod_id: None,
            config_dir: None,
            mod_exists_fn: None,
        }
    }

    /// Set the current mod ID
    pub fn with_current_mod(mut self, mod_id: impl Into<String>) -> Self {
        self.current_mod_id = Some(mod_id.into());
        self
    }

    /// Set the config directory
    pub fn with_config_dir(mut self, config_dir: impl Into<PathBuf>) -> Self {
        self.config_dir = Some(config_dir.into());
        self
    }

    /// Set a function to check if a mod exists
    pub fn with_mod_exists_fn<F>(mut self, f: F) -> Self
    where
        F: Fn(&str) -> bool + Send + Sync + 'static,
    {
        self.mod_exists_fn = Some(Box::new(f));
        self
    }

    /// Check if a mod exists (returns true if no check function is set)
    pub fn mod_exists(&self, mod_id: &str) -> bool {
        match &self.mod_exists_fn {
            Some(f) => f(mod_id),
            None => true, // No check function, assume exists
        }
    }
}

/// Result of resolving a mod path
#[derive(Debug)]
pub struct ResolvedModPath {
    /// The resolved absolute path
    pub absolute_path: PathBuf,
    /// The path relative to home_dir (for use with AssetServer etc.)
    pub relative_path: String,
    /// Whether a mod reference was resolved
    pub mod_id: Option<String>,
}

/// Resolve a path with @mod-id support
///
/// This function resolves paths that may contain @mod-id syntax and validates
/// that the resulting path is within permitted directories.
///
/// # Path Resolution Rules
///
/// 1. `@other-mod/path` → `mods/other-mod/path` (references another mod)
/// 2. `path/to/file` (with current_mod set) → `mods/current-mod/path/to/file`
/// 3. `path/to/file` (no current_mod) → `path/to/file` (relative to home_dir)
///
/// # Security
///
/// - The resolved path is validated to be within home_dir or config_dir
/// - Path traversal attempts (../) are blocked
/// - If a mod reference is used, the mod's existence is verified (if check function is set)
///
/// # Arguments
/// * `path` - The path to resolve (may contain @mod-id prefix)
/// * `config` - Configuration for path resolution
///
/// # Returns
/// * `Ok(ResolvedModPath)` with absolute and relative paths
/// * `Err(String)` if path is invalid or outside permitted directories
pub fn resolve_mod_path(path: &str, config: &ModPathConfig) -> Result<ResolvedModPath, String> {
    let parsed = parse_mod_path(path);

    let (mod_id, relative_to_mod) = match parsed {
        ParsedModPath::ModReference { mod_id, path: sub_path } => {
            // Check if mod exists
            if !config.mod_exists(&mod_id) {
                return Err(format!(
                    "Mod '{}' not found. Cannot resolve path '{}'",
                    mod_id, path
                ));
            }
            (Some(mod_id), sub_path)
        }
        ParsedModPath::Regular(regular_path) => {
            // Use current mod if set, otherwise treat as relative to home_dir
            if let Some(ref current) = config.current_mod_id {
                (Some(current.clone()), regular_path)
            } else {
                (None, regular_path)
            }
        }
    };

    // Build the relative path
    let relative_path = if let Some(ref mid) = mod_id {
        format!("mods/{}/{}", mid, relative_to_mod)
    } else {
        relative_to_mod
    };

    // Canonicalize home_dir first to get absolute path
    let canonical_home = config.home_dir.canonicalize().map_err(|e| {
        format!(
            "Failed to canonicalize home directory '{}': {}",
            config.home_dir.display(),
            e
        )
    })?;

    // Build the absolute path from canonicalized home_dir
    let absolute_path = canonical_home.join(&relative_path);

    // Normalize to resolve any .. or . components
    let normalized = normalize_path_components(&absolute_path);

    // Check if normalized path is within home_dir (security check for path traversal)
    if !normalized.starts_with(&canonical_home) {
        // Check config_dir if available
        if let Some(ref cfg_dir) = config.config_dir {
            if let Ok(canonical_cfg) = cfg_dir.canonicalize() {
                if normalized.starts_with(&canonical_cfg) {
                    return Ok(ResolvedModPath {
                        absolute_path: normalized,
                        relative_path,
                        mod_id,
                    });
                }
            }
        }

        return Err(format!(
            "Access denied: path '{}' resolves to '{}' which is outside permitted directories. \
             Path traversal is not allowed.",
            path,
            normalized.display()
        ));
    }

    Ok(ResolvedModPath {
        absolute_path: normalized,
        relative_path,
        mod_id,
    })
}

/// Resolve a mod path and validate that the file exists
///
/// This is like `resolve_mod_path` but also checks that the file exists
/// and canonicalizes the result.
///
/// # Arguments
/// * `path` - The path to resolve
/// * `config` - Configuration for path resolution
///
/// # Returns
/// * `Ok(ResolvedModPath)` with canonical absolute path
/// * `Err(String)` if path doesn't exist or is outside permitted directories
pub fn resolve_and_validate_mod_path(
    path: &str,
    config: &ModPathConfig,
) -> Result<ResolvedModPath, String> {
    let resolved = resolve_mod_path(path, config)?;

    // Now canonicalize to get the real path and verify existence
    let canonical = resolved.absolute_path.canonicalize().map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            format!("Path does not exist: {}", resolved.absolute_path.display())
        } else {
            format!(
                "Failed to resolve path '{}': {}",
                resolved.absolute_path.display(),
                e
            )
        }
    })?;

    // Re-validate canonical path is within permitted directories
    let canonical_home = config.home_dir.canonicalize().map_err(|e| {
        format!(
            "Failed to canonicalize home directory: {}",
            e
        )
    })?;

    if !canonical.starts_with(&canonical_home) {
        if let Some(ref cfg_dir) = config.config_dir {
            if let Ok(canonical_cfg) = cfg_dir.canonicalize() {
                if canonical.starts_with(&canonical_cfg) {
                    return Ok(ResolvedModPath {
                        absolute_path: canonical,
                        relative_path: resolved.relative_path,
                        mod_id: resolved.mod_id,
                    });
                }
            }
        }

        return Err(format!(
            "Access denied: resolved path '{}' is outside permitted directories (symlink escape attempt?)",
            canonical.display()
        ));
    }

    Ok(ResolvedModPath {
        absolute_path: canonical,
        relative_path: resolved.relative_path,
        mod_id: resolved.mod_id,
    })
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
