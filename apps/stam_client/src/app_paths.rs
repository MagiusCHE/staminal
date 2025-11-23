/// Application paths configuration module
///
/// Centralizes all path management for the application.
/// Initialized once at startup with custom_home parameter.

use std::path::PathBuf;
use std::fs;
use tracing::debug;

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

    /// Get the game-specific data directory
    ///
    /// # Arguments
    /// * `game_id` - Game identifier
    pub fn game_root(&self, game_id: &str) -> Result<PathBuf, Box<dyn std::error::Error>> {
        let game_root = self.data_dir.join(game_id);

        // Create directory if it doesn't exist
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
