/// JavaScript Runtime Adapter
///
/// Re-exports the shared JavaScript runtime adapter and provides client-specific configuration helpers.

// Re-export from shared stam_mod_runtimes
pub use stam_mod_runtimes::adapters::{
    JsRuntimeAdapter, JsRuntimeConfig,
    run_js_event_loop,
};

/// Helper function to create JsRuntimeConfig using the game_root directory
///
/// # Arguments
/// * `game_root` - The game-specific root directory (already includes server/game path)
pub fn create_js_runtime_config(game_root: &std::path::Path) -> Result<JsRuntimeConfig, Box<dyn std::error::Error>> {
    // Use game_root as both data and config directory
    // The game_root already contains the proper path: <data_dir>/<host> - <server_name>/<game_id>/
    let game_data_dir = game_root.to_path_buf();
    let game_config_dir = game_root.join("config");

    // Ensure config directory exists (game_root and mods dir are already created by app_paths.game_root())
    if !game_config_dir.exists() {
        std::fs::create_dir_all(&game_config_dir)?;
    }

    Ok(JsRuntimeConfig::new(game_data_dir, game_config_dir))
}
