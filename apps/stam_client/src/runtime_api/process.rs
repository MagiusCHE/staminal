/// Process API abstraction
///
/// Provides access to process and application information.
/// This module is runtime-agnostic and can be used by JavaScript, Lua, C#, etc.

use std::path::PathBuf;
use std::fs;

/// Process API implementation
pub struct ProcessApi {
    data_dir: PathBuf,
    config_dir: PathBuf,
}

impl ProcessApi {
    /// Create a new ProcessApi with the specified directories
    pub fn new(data_dir: PathBuf, config_dir: PathBuf) -> Self {
        Self { data_dir, config_dir }
    }

    /// Get the application data directory path as an absolute path
    pub fn app_data_path(&self) -> String {
        Self::to_absolute_path(&self.data_dir)
    }

    /// Get the application config directory path as an absolute path
    pub fn app_config_path(&self) -> String {
        Self::to_absolute_path(&self.config_dir)
    }

    /// Convert a path to absolute and normalized form
    fn to_absolute_path(path: &PathBuf) -> String {
        // Convert to absolute path using canonicalize if path exists
        let absolute_path = fs::canonicalize(path)
            .unwrap_or_else(|_| {
                // If canonicalize fails (e.g., path doesn't exist yet),
                // manually resolve to absolute path
                let base = if path.is_absolute() {
                    path.clone()
                } else {
                    std::env::current_dir()
                        .unwrap_or_else(|_| PathBuf::from("."))
                        .join(path)
                };

                // Normalize the path by resolving . and ..
                Self::normalize_path(&base)
            });

        absolute_path.to_string_lossy().to_string()
    }

    /// Normalize a path by resolving . and .. components
    fn normalize_path(path: &PathBuf) -> PathBuf {
        let mut normalized = PathBuf::new();

        for component in path.components() {
            match component {
                std::path::Component::CurDir => {
                    // Skip "." components
                }
                std::path::Component::ParentDir => {
                    // Go up one level for ".."
                    normalized.pop();
                }
                _ => {
                    // Add normal components
                    normalized.push(component);
                }
            }
        }

        normalized
    }
}

/// Application-specific API (part of process.app)
pub struct AppApi {
    process_api: ProcessApi,
}

impl AppApi {
    /// Create a new AppApi
    pub fn new(data_dir: PathBuf, config_dir: PathBuf) -> Self {
        Self {
            process_api: ProcessApi::new(data_dir, config_dir),
        }
    }

    /// Get the data directory path as an absolute path
    pub fn data_path(&self) -> String {
        self.process_api.app_data_path()
    }

    /// Get the config directory path as an absolute path
    pub fn config_path(&self) -> String {
        self.process_api.app_config_path()
    }
}
