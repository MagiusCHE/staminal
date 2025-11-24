/// JavaScript Runtime Adapter
///
/// Re-exports the shared JavaScript runtime adapter and provides client-specific configuration helpers.

use crate::AppPaths;

// Re-export from shared stam_mod_runtimes
pub use stam_mod_runtimes::adapters::{JsRuntimeAdapter, JsRuntimeConfig};

/// Helper function to create JsRuntimeConfig from AppPaths
///
/// # Arguments
/// * `app_paths` - Application paths
/// * `game_id` - Game identifier
pub fn create_js_runtime_config(app_paths: &AppPaths, game_id: &str) -> Result<JsRuntimeConfig, Box<dyn std::error::Error>> {
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

    Ok(JsRuntimeConfig::new(game_data_dir, game_config_dir))
}
