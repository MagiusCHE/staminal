//! Bevy application setup and main loop
//!
//! This module configures and runs the Bevy application with:
//! - Window configuration
//! - bevy_egui plugin for UI rendering
//! - Custom systems for mod runtime communication

use bevy::prelude::*;
use bevy_egui::EguiPlugin;

use super::ui_bridge::UiBridge;
use super::systems::{
    UiLayouts,
    WindowRegistry,
    process_ui_commands,
    process_window_commands,
    sync_window_size,
    render_ui_layouts,
    check_shutdown,
};

/// Staminal application state
#[derive(Clone)]
pub struct StaminalApp {
    /// Window title
    pub title: String,
    /// Initial window width
    pub width: u32,
    /// Initial window height
    pub height: u32,
    /// Whether the window is resizable
    pub resizable: bool,
}

impl Default for StaminalApp {
    fn default() -> Self {
        Self {
            title: "Staminal".to_string(),
            width: 1280,
            height: 720,
            resizable: true,
        }
    }
}

/// Plugin that sets up the Staminal UI systems
pub struct StaminalUiPlugin {
    bridge: UiBridge,
}

impl StaminalUiPlugin {
    pub fn new(bridge: UiBridge) -> Self {
        Self { bridge }
    }
}

impl Plugin for StaminalUiPlugin {
    fn build(&self, app: &mut App) {
        app
            // Insert the bridge as a resource
            .insert_resource(self.bridge.clone())
            // Insert UI layouts storage
            .init_resource::<UiLayouts>()
            // Window registry for tracking script window IDs
            .init_resource::<WindowRegistry>()
            // Add systems - process commands first, then render
            .add_systems(Update, (
                check_shutdown,
                process_ui_commands,
                process_window_commands,
                sync_window_size,
            ))
            .add_systems(Update, render_ui_layouts.after(process_ui_commands));
    }
}

/// Run the Bevy application
///
/// This function blocks until the window is closed.
/// It should be called from a separate thread if you need
/// to run other async tasks concurrently.
///
/// The application starts with a hidden window.
/// Scripts call `window.create()` to configure and show the window.
///
/// # Arguments
/// * `_config` - Application configuration (currently unused, window is created by script)
/// * `bridge` - Communication bridge for mod runtimes
pub fn run_bevy_app(_config: StaminalApp, bridge: UiBridge) {
    tracing::info!("Starting Bevy application with hidden window (waiting for window.show() from script)");

    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "Staminal".to_string(),
                resolution: bevy::window::WindowResolution::new(1280, 720),
                visible: false, // Start hidden, script will show it
                ..default()
            }),
            ..default()
        }))
        .add_plugins(EguiPlugin::default())
        .add_plugins(StaminalUiPlugin::new(bridge))
        .run();
}

/// Create Bevy app configuration from command line args or defaults
pub fn create_app_config(title: Option<&str>, width: Option<u32>, height: Option<u32>) -> StaminalApp {
    StaminalApp {
        title: title.unwrap_or("Staminal").to_string(),
        width: width.unwrap_or(1280),
        height: height.unwrap_or(720),
        resizable: true,
    }
}
