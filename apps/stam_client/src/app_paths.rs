/// Application paths configuration module
///
/// Centralizes all path management for the application.
/// Initialized once at startup with custom_home parameter.

use std::path::PathBuf;
use std::fs;
use tracing::debug;

/// Sanitize a string to be used as a directory name
/// Replaces invalid characters and normalizes whitespace
pub fn sanitize_dirname(name: &str) -> String {
    let mut result = String::new();
    let mut last_was_space = false;

    for c in name.chars() {
        // Replace invalid filesystem characters with underscore
        let safe_char = match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' | '\0' => '_',
            // Normalize whitespace
            ' ' | '\t' | '\n' | '\r' => ' ',
            _ => c,
        };

        // Avoid duplicate spaces
        if safe_char == ' ' {
            if !last_was_space && !result.is_empty() {
                result.push(' ');
                last_was_space = true;
            }
        } else {
            result.push(safe_char);
            last_was_space = false;
        }
    }

    // Trim trailing spaces
    result.trim_end().to_string()
}

#[derive(Clone)]
pub struct AppPaths {
    config_dir: PathBuf,
    data_dir: PathBuf,
}

impl AppPaths {
    /// Initialize application paths
    ///
    /// # Arguments
    /// * `custom_home` - Optional custom home directory from STAM_HOME env variable
    pub fn new(custom_home: Option<&str>) -> Result<Self, Box<dyn std::error::Error>> {
        let (config_dir, data_dir) = if let Some(home) = custom_home {
            // Custom home: use it directly for both config and data
            let home_path = PathBuf::from(home);
            (home_path.clone(), home_path)
        } else {
            // Standard paths
            let config = dirs::config_dir()
                .ok_or("Could not determine config directory")?
                .join("staminal");

            let data = dirs::data_dir()
                .ok_or("Could not determine data directory")?
                .join("staminal");

            (config, data)
        };

        // Create directories if they don't exist
        if !config_dir.exists() {
            fs::create_dir_all(&config_dir)?;
            debug!("Created Staminal config directory: {}", config_dir.display());
        }

        if !data_dir.exists() {
            fs::create_dir_all(&data_dir)?;
            debug!("Created Staminal data directory: {}", data_dir.display());
        }

        Ok(Self {
            config_dir,
            data_dir,
        })
    }

    /// Get the Staminal config directory
    pub fn config_dir(&self) -> &PathBuf {
        &self.config_dir
    }

    /// Get the Staminal data directory
    pub fn data_dir(&self) -> &PathBuf {
        &self.data_dir
    }

    /// Get the temporary directory for downloads
    pub fn tmp_dir(&self) -> Result<PathBuf, Box<dyn std::error::Error>> {
        let tmp_dir = self.data_dir.join("tmp");

        // Create directory if it doesn't exist
        if !tmp_dir.exists() {
            fs::create_dir_all(&tmp_dir)?;
            debug!("Created tmp directory: {}", tmp_dir.display());
        }

        Ok(tmp_dir)
    }

    /// Get the game-specific data directory
    ///
    /// Directory structure: data_dir/<server_dir>/<game_id>/
    /// Where server_dir is: "<host> - <server_name>" (sanitized)
    ///
    /// # Arguments
    /// * `host` - Server host (e.g., "127.0.0.1", "magius.it")
    /// * `server_name` - Human-readable server name (e.g., "Develop Realm")
    /// * `game_id` - Game identifier
    pub fn game_root(&self, host: &str, server_name: &str, game_id: &str) -> Result<PathBuf, Box<dyn std::error::Error>> {
        // Build server directory name: "<host> - <server_name>" (sanitized)
        let server_dir_name = sanitize_dirname(&format!("{} - {}", host, server_name));
        let server_dir = self.data_dir.join(&server_dir_name);

        // Create server directory if it doesn't exist
        if !server_dir.exists() {
            fs::create_dir_all(&server_dir)?;
            debug!("Created server directory: {}", server_dir.display());
        }

        let game_root = server_dir.join(game_id);

        // Create game directory if it doesn't exist
        if !game_root.exists() {
            fs::create_dir_all(&game_root)?;
            debug!("Created game root directory: {}", game_root.display());
        }

        // Create mods directory for this game
        let mods_dir = game_root.join("mods");
        if !mods_dir.exists() {
            fs::create_dir_all(&mods_dir)?;
            debug!("Created mods directory: {}", mods_dir.display());
        }

        Ok(game_root)
    }
}
