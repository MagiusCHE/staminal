use std::path::PathBuf;

/// Configuration for JavaScript runtime
///
/// Contains game-specific directories for data and config.
#[derive(Clone)]
pub struct JsRuntimeConfig {
    /// Game identifier (used for logging on server, None on client)
    game_id: Option<String>,
    /// Game-specific data directory
    game_data_dir: PathBuf,
    /// Game-specific config directory
    game_config_dir: PathBuf,
}

impl JsRuntimeConfig {
    /// Create a new JavaScript runtime configuration
    ///
    /// # Arguments
    /// * `game_data_dir` - Path to game-specific data directory
    /// * `game_config_dir` - Path to game-specific config directory
    pub fn new(game_data_dir: PathBuf, game_config_dir: PathBuf) -> Self {
        Self {
            game_id: None,
            game_data_dir,
            game_config_dir,
        }
    }

    /// Set the game identifier (for server-side logging)
    pub fn with_game_id(mut self, game_id: impl Into<String>) -> Self {
        self.game_id = Some(game_id.into());
        self
    }

    /// Get the game identifier (if set)
    pub fn game_id(&self) -> Option<&str> {
        self.game_id.as_deref()
    }

    /// Get the game-specific data directory
    pub fn game_data_dir(&self) -> &PathBuf {
        &self.game_data_dir
    }

    /// Get the game-specific config directory
    pub fn game_config_dir(&self) -> &PathBuf {
        &self.game_config_dir
    }
}
