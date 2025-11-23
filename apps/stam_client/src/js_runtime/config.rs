use std::path::PathBuf;
use crate::AppPaths;

/// Configuration for script runtime (JavaScript, Lua, C#, etc.)
///
/// Contains all necessary paths and settings for mod execution.
/// Ensures proper segregation of game data by including game_id.
/// This configuration is runtime-agnostic and can be used by any scripting engine.
#[derive(Clone)]
pub struct ScriptRuntimeConfig {
    /// Application paths
    app_paths: AppPaths,
    /// Game identifier (for data segregation)
    game_id: String,
    /// Game-specific data directory
    game_data_dir: PathBuf,
    /// Game-specific config directory
    game_config_dir: PathBuf,
}

impl ScriptRuntimeConfig {
    /// Create a new runtime configuration
    ///
    /// # Arguments
    /// * `app_paths` - Application paths
    /// * `game_id` - Game identifier for data segregation
    pub fn new(app_paths: AppPaths, game_id: &str) -> Result<Self, Box<dyn std::error::Error>> {
        // Create game-specific directories
        let game_data_dir = app_paths.data_dir().join(game_id);
        let game_config_dir = app_paths.config_dir().join(game_id);

        // Ensure directories exist
        if !game_data_dir.exists() {
            std::fs::create_dir_all(&game_data_dir)?;
        }

        if !game_config_dir.exists() {
            std::fs::create_dir_all(&game_config_dir)?;
        }

        Ok(Self {
            app_paths,
            game_id: game_id.to_string(),
            game_data_dir,
            game_config_dir,
        })
    }

    /// Get the game-specific data directory
    pub fn game_data_dir(&self) -> &PathBuf {
        &self.game_data_dir
    }

    /// Get the game-specific config directory
    pub fn game_config_dir(&self) -> &PathBuf {
        &self.game_config_dir
    }

    /// Get the game ID
    pub fn game_id(&self) -> &str {
        &self.game_id
    }

    /// Get the application paths
    pub fn app_paths(&self) -> &AppPaths {
        &self.app_paths
    }
}
