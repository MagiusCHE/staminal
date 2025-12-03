//! Bevy Engine Implementation
//!
//! This module implements the `GraphicEngine` trait for the Bevy game engine.
//! Bevy runs on the main thread and communicates with the worker thread via channels.

use bevy::prelude::*;
use bevy::window::{PrimaryWindow, WindowMode, WindowResolution};
use bevy::winit::{UpdateMode, WinitSettings};
use std::collections::HashMap;
use std::sync::mpsc::Receiver;
use std::sync::Mutex;
use tokio::sync::mpsc::Sender;

use stam_mod_runtimes::api::{
    ColorValue, EdgeInsets, FlexDirection, GraphicCommand, GraphicEngine, GraphicEngineInfo,
    GraphicEngines, GraphicEvent, InitialWindowConfig, JustifyContent, KeyModifiers, MouseButton,
    PropertyValue, SizeValue, WidgetConfig, WidgetEventType, WidgetType, WindowPositionMode,
    AlignItems,
};

// Note: WinitWindows was removed as SetWindowPosition is no longer supported
// (Wayland doesn't allow setting window position after creation)

/// Bevy engine implementation
///
/// This struct wraps the Bevy App and provides the GraphicEngine interface.
/// It receives commands from the GraphicProxy and sends events back.
pub struct BevyEngine {
    /// Channel to send events to the worker thread
    event_tx: Sender<GraphicEvent>,
}

impl BevyEngine {
    /// Create a new BevyEngine instance
    ///
    /// # Arguments
    /// * `event_tx` - Channel to send events to the worker thread
    pub fn new(event_tx: Sender<GraphicEvent>) -> Self {
        Self { event_tx }
    }
}

impl GraphicEngine for BevyEngine {
    fn run(
        &mut self,
        command_rx: Receiver<GraphicCommand>,
        initial_window_config: Option<InitialWindowConfig>,
        asset_root: Option<std::path::PathBuf>,
    ) {
        let event_tx = self.event_tx.clone();

        let mut app = App::new();

        // Get the initial window configuration, using defaults if not provided
        let win_config = initial_window_config.unwrap_or_default();

        // Determine initial window position
        let position = match win_config.position_mode {
            WindowPositionMode::Default => bevy::window::WindowPosition::Automatic,
            WindowPositionMode::Centered => {
                bevy::window::WindowPosition::Centered(MonitorSelection::Primary)
            }
            WindowPositionMode::At(x, y) => bevy::window::WindowPosition::At(IVec2::new(x, y)),
        };

        // Determine window mode
        let mode = if win_config.fullscreen {
            WindowMode::BorderlessFullscreen(MonitorSelection::Current)
        } else {
            WindowMode::Windowed
        };

        // Build window with initial configuration
        // The window is created with the proper settings from the start
        // This is important because some settings (like position) cannot be changed after creation on Wayland
        // Configure AssetPlugin to use asset_root as the base path for loading assets
        let asset_file_path = asset_root
            .as_ref()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|| "assets".to_string());

        tracing::debug!("Bevy AssetPlugin file_path: {}", asset_file_path);

        app.add_plugins(
            DefaultPlugins
                .build()
                .set(WindowPlugin {
                    primary_window: Some(Window {
                        title: win_config.title.clone(),
                        resolution: WindowResolution::new(win_config.width as u32, win_config.height as u32),
                        resizable: win_config.resizable,
                        mode,
                        position,
                        visible: true,
                        ..default()
                    }),
                    ..default()
                })
                .set(bevy::asset::AssetPlugin {
                    file_path: asset_file_path,
                    ..default()
                }),
        );

        // Insert command receiver as non-send resource (uses Mutex for thread safety)
        app.insert_non_send_resource(CommandReceiverRes(Mutex::new(command_rx)));
        app.insert_resource(EventSenderRes(event_tx.clone()));
        app.insert_resource(WindowRegistry::default());
        app.insert_resource(WidgetRegistry::default());
        app.insert_resource(FontRegistry::default());
        app.insert_resource(EngineReadySent::default());

        // Force continuous updates even without windows or when unfocused
        // This ensures the Update schedule runs continuously to process commands
        app.insert_resource(WinitSettings {
            focused_mode: UpdateMode::Continuous,
            unfocused_mode: UpdateMode::Continuous,
        });

        // Add startup system to register the primary window in our registry
        app.add_systems(Startup, register_primary_window);

        // Add our systems - send_engine_ready_event runs AFTER process_commands
        // to ensure the command channel is being processed before JS handlers
        // try to call createWindow() in response to EngineReady
        app.add_systems(
            Update,
            (
                process_commands,
                send_engine_ready_event.after(process_commands),
                send_frame_events,
                handle_keyboard_input,
                handle_mouse_input,
                handle_window_events,
                handle_widget_interactions,
            ),
        );

        tracing::info!("Bevy engine starting main loop");

        // Run Bevy main loop (blocks until exit)
        app.run();
    }

    fn engine_type(&self) -> GraphicEngines {
        GraphicEngines::Bevy
    }

    fn get_engine_info(&self) -> GraphicEngineInfo {
        // Get the Bevy version from Cargo.toml (it's in our dependencies)
        // The version is determined at compile time
        const BEVY_VERSION: &str = env!("CARGO_PKG_VERSION");

        GraphicEngineInfo {
            engine_type: "Bevy".to_string(),
            engine_type_id: GraphicEngines::Bevy.to_u32(),
            name: "Bevy Game Engine".to_string(),
            version: "0.17.3".to_string(), // Match our Cargo.toml version
            description: "A refreshingly simple data-driven game engine built in Rust".to_string(),
            features: vec![
                "ECS".to_string(),
                "2D Rendering".to_string(),
                "3D Rendering".to_string(),
                "UI System".to_string(),
                "Sprite Rendering".to_string(),
                "Text Rendering".to_string(),
                "Input Handling".to_string(),
                "Window Management".to_string(),
                "Asset Loading".to_string(),
            ],
            // Backend is determined at runtime by wgpu, but we can provide a general description
            // In a more sophisticated implementation, we could query wgpu for the actual backend
            backend: detect_rendering_backend(),
            supports_2d: true,
            supports_3d: true,
            supports_ui: true,
            supports_audio: false, // We didn't enable audio feature
        }
    }
}

/// Detect the rendering backend being used
///
/// This is a best-effort detection based on the platform.
/// In reality, wgpu selects the backend at runtime.
fn detect_rendering_backend() -> String {
    #[cfg(target_os = "windows")]
    {
        "DirectX 12 / Vulkan".to_string()
    }
    #[cfg(target_os = "macos")]
    {
        "Metal".to_string()
    }
    #[cfg(target_os = "linux")]
    {
        "Vulkan".to_string()
    }
    #[cfg(target_os = "ios")]
    {
        "Metal".to_string()
    }
    #[cfg(target_os = "android")]
    {
        "Vulkan / OpenGL ES".to_string()
    }
    #[cfg(target_arch = "wasm32")]
    {
        "WebGPU / WebGL".to_string()
    }
    #[cfg(not(any(
        target_os = "windows",
        target_os = "macos",
        target_os = "linux",
        target_os = "ios",
        target_os = "android",
        target_arch = "wasm32"
    )))]
    {
        "Unknown".to_string()
    }
}

/// Resource holding the command receiver channel (wrapped in Mutex for thread safety)
struct CommandReceiverRes(Mutex<Receiver<GraphicCommand>>);

/// Resource holding the event sender channel
#[derive(Resource)]
struct EventSenderRes(Sender<GraphicEvent>);

/// Registry mapping our window IDs to Bevy Entity IDs
#[derive(Resource, Default)]
struct WindowRegistry {
    /// Map from our window ID to Bevy Entity
    id_to_entity: HashMap<u64, Entity>,
    /// Map from Bevy Entity to our window ID
    entity_to_id: HashMap<Entity, u64>,
}

impl WindowRegistry {
    fn register(&mut self, id: u64, entity: Entity) {
        self.id_to_entity.insert(id, entity);
        self.entity_to_id.insert(entity, id);
    }

    fn unregister(&mut self, id: u64) -> Option<Entity> {
        if let Some(entity) = self.id_to_entity.remove(&id) {
            self.entity_to_id.remove(&entity);
            Some(entity)
        } else {
            None
        }
    }

    fn get_entity(&self, id: u64) -> Option<Entity> {
        self.id_to_entity.get(&id).copied()
    }

    fn get_id(&self, entity: Entity) -> Option<u64> {
        self.entity_to_id.get(&entity).copied()
    }
}

/// Resource to hold the default UI camera entity
#[derive(Resource)]
struct UiCamera(Entity);

// ============================================================================
// Widget System Resources and Components
// ============================================================================

/// Registry mapping widget IDs to Bevy Entities
#[derive(Resource, Default)]
struct WidgetRegistry {
    /// Map from widget ID to Bevy Entity
    id_to_entity: HashMap<u64, Entity>,
    /// Map from Bevy Entity to widget ID
    entity_to_id: HashMap<Entity, u64>,
    /// Root UI nodes for each window (window_id -> root node entity)
    window_roots: HashMap<u64, Entity>,
}

impl WidgetRegistry {
    fn register(&mut self, id: u64, entity: Entity) {
        self.id_to_entity.insert(id, entity);
        self.entity_to_id.insert(entity, id);
    }

    fn unregister(&mut self, id: u64) -> Option<Entity> {
        if let Some(entity) = self.id_to_entity.remove(&id) {
            self.entity_to_id.remove(&entity);
            Some(entity)
        } else {
            None
        }
    }

    fn get_entity(&self, id: u64) -> Option<Entity> {
        self.id_to_entity.get(&id).copied()
    }

    fn get_id(&self, entity: Entity) -> Option<u64> {
        self.entity_to_id.get(&entity).copied()
    }

    fn set_window_root(&mut self, window_id: u64, root_entity: Entity) {
        self.window_roots.insert(window_id, root_entity);
    }

    fn get_window_root(&self, window_id: u64) -> Option<Entity> {
        self.window_roots.get(&window_id).copied()
    }
}

/// Marker component for Staminal widgets
#[derive(Component)]
struct StamWidget {
    /// Unique widget ID
    id: u64,
    /// Parent window ID
    window_id: u64,
    /// Widget type
    widget_type: WidgetType,
}

/// Component tracking which events this widget is subscribed to
#[derive(Component, Default)]
struct WidgetEventSubscriptions {
    on_click: bool,
    on_hover: bool,
    on_focus: bool,
}

/// Component for button color states
#[derive(Component)]
struct ButtonColors {
    normal: Color,
    hovered: Color,
    pressed: Color,
    disabled: Color,
}

/// Marker component for disabled buttons
#[derive(Component)]
struct ButtonDisabled;

impl Default for ButtonColors {
    fn default() -> Self {
        Self {
            normal: Color::srgb(0.3, 0.3, 0.3),
            hovered: Color::srgb(0.4, 0.4, 0.4),
            pressed: Color::srgb(0.2, 0.2, 0.2),
            disabled: Color::srgb(0.5, 0.5, 0.5),
        }
    }
}

/// Font configuration for inheritance
#[derive(Clone, Debug)]
struct InheritedFontConfig {
    /// Font family alias
    family: String,
    /// Font size
    size: f32,
}

impl Default for InheritedFontConfig {
    fn default() -> Self {
        Self {
            family: "default".to_string(),
            size: 16.0,
        }
    }
}

/// Registry for loaded fonts and window default fonts
#[derive(Resource, Default)]
struct FontRegistry {
    /// Loaded font handles by alias
    loaded_fonts: HashMap<String, Handle<Font>>,
    /// Default font for each window (window_id -> font config)
    window_fonts: HashMap<u64, InheritedFontConfig>,
}

impl FontRegistry {
    /// Get the font handle for an alias, or None if not loaded
    fn get_font(&self, alias: &str) -> Option<Handle<Font>> {
        self.loaded_fonts.get(alias).cloned()
    }

    /// Get the default font config for a window
    fn get_window_font(&self, window_id: u64) -> InheritedFontConfig {
        self.window_fonts
            .get(&window_id)
            .cloned()
            .unwrap_or_default()
    }

    /// Set the default font for a window
    fn set_window_font(&mut self, window_id: u64, family: String, size: f32) {
        self.window_fonts.insert(window_id, InheritedFontConfig { family, size });
    }

    /// Register a loaded font
    fn register_font(&mut self, alias: String, handle: Handle<Font>) {
        self.loaded_fonts.insert(alias, handle);
    }

    /// Unregister a font
    fn unregister_font(&mut self, alias: &str) {
        self.loaded_fonts.remove(alias);
    }
}

/// Component to store inherited font config on widgets
#[derive(Component, Clone, Default)]
struct WidgetFontConfig {
    /// Font family alias (if set by this widget, overrides parent)
    family: Option<String>,
    /// Font size (if set by this widget, overrides parent)
    size: Option<f32>,
}

impl WidgetFontConfig {
    /// Merge with parent font config to get effective font
    fn resolve(&self, parent: &InheritedFontConfig) -> InheritedFontConfig {
        InheritedFontConfig {
            family: self.family.clone().unwrap_or_else(|| parent.family.clone()),
            size: self.size.unwrap_or(parent.size),
        }
    }
}

/// Component to track previous interaction state for detecting changes
#[derive(Component, Default)]
struct PreviousInteraction(Interaction);

/// Convert ColorValue to Bevy Color
fn color_value_to_bevy(color: &ColorValue) -> Color {
    Color::srgba(color.r, color.g, color.b, color.a)
}

/// Convert SizeValue to Bevy Val
fn size_value_to_val(size: &SizeValue) -> Val {
    match size {
        SizeValue::Px(px) => Val::Px(*px),
        SizeValue::Percent(pct) => Val::Percent(*pct),
        SizeValue::Auto => Val::Auto,
    }
}

/// Convert EdgeInsets to Bevy UiRect
fn edge_insets_to_ui_rect(insets: &EdgeInsets) -> UiRect {
    UiRect {
        top: Val::Px(insets.top),
        right: Val::Px(insets.right),
        bottom: Val::Px(insets.bottom),
        left: Val::Px(insets.left),
    }
}

/// System to process commands from the worker thread
fn process_commands(
    cmd_rx: NonSend<CommandReceiverRes>,
    event_tx: Res<EventSenderRes>,
    mut commands: Commands,
    mut registry: ResMut<WindowRegistry>,
    mut widget_registry: ResMut<WidgetRegistry>,
    mut font_registry: ResMut<FontRegistry>,
    asset_server: Res<AssetServer>,
    mut windows: Query<&mut Window>,
    mut app_exit: EventWriter<bevy::app::AppExit>,
    primary_window_query: Query<Entity, With<PrimaryWindow>>,
    mut text_query: Query<&mut Text>,
    mut bg_color_query: Query<&mut BackgroundColor>,
    mut node_query: Query<&mut Node>,
    mut text_color_query: Query<&mut TextColor>,
    mut button_query: Query<(&mut ButtonColors, Option<&Interaction>, Option<&Children>)>,
    ui_camera: Option<Res<UiCamera>>,
) {
    // Lock the receiver and process all available commands (non-blocking)
    let receiver = match cmd_rx.0.lock() {
        Ok(r) => r,
        Err(e) => {
            tracing::error!("Failed to lock command receiver: {:?}", e);
            return;
        }
    };
    while let Ok(cmd) = receiver.try_recv() {
        match cmd {
            GraphicCommand::CreateWindow {
                id,
                config,
                response_tx,
            } => {
                tracing::debug!("Creating window {} with config: {:?}", id, config);

                // Convert position mode to Bevy WindowPosition
                let position = match config.position_mode {
                    WindowPositionMode::Default => bevy::window::WindowPosition::Automatic,
                    WindowPositionMode::Centered => {
                        bevy::window::WindowPosition::Centered(MonitorSelection::Primary)
                    }
                    WindowPositionMode::At(x, y) => {
                        bevy::window::WindowPosition::At(IVec2::new(x, y))
                    }
                };

                // Spawn the window entity
                let window = Window {
                    title: config.title,
                    resolution: WindowResolution::new(config.width as u32, config.height as u32),
                    resizable: config.resizable,
                    visible: config.visible,
                    position,
                    mode: if config.fullscreen {
                        WindowMode::BorderlessFullscreen(MonitorSelection::Current)
                    } else {
                        WindowMode::Windowed
                    },
                    ..default()
                };

                tracing::debug!("Bevy Window struct: {:?}", window);

                let entity = commands.spawn(window).id();
                registry.register(id, entity);

                let _ = response_tx.send(Ok(()));

                // Send window created event
                let _ = event_tx.0.try_send(GraphicEvent::WindowCreated { window_id: id });
            }

            GraphicCommand::CloseWindow { id, response_tx } => {
                tracing::debug!("Closing window {}", id);

                if let Some(entity) = registry.unregister(id) {
                    commands.entity(entity).despawn();
                    let _ = response_tx.send(Ok(()));

                    // Send window closed event
                    let _ = event_tx.0.try_send(GraphicEvent::WindowClosed { window_id: id });
                } else {
                    let _ = response_tx.send(Err(format!("Window {} not found", id)));
                }
            }

            GraphicCommand::SetWindowSize {
                id,
                width,
                height,
                response_tx,
            } => {
                if let Some(entity) = registry.get_entity(id) {
                    if let Ok(mut window) = windows.get_mut(entity) {
                        window.resolution.set(width as f32, height as f32);
                        let _ = response_tx.send(Ok(()));
                    } else {
                        let _ = response_tx.send(Err(format!("Window {} entity not found", id)));
                    }
                } else {
                    let _ = response_tx.send(Err(format!("Window {} not found", id)));
                }
            }

            GraphicCommand::SetWindowTitle {
                id,
                title,
                response_tx,
            } => {
                if let Some(entity) = registry.get_entity(id) {
                    if let Ok(mut window) = windows.get_mut(entity) {
                        window.title = title;
                        let _ = response_tx.send(Ok(()));
                    } else {
                        let _ = response_tx.send(Err(format!("Window {} entity not found", id)));
                    }
                } else {
                    let _ = response_tx.send(Err(format!("Window {} not found", id)));
                }
            }

            GraphicCommand::SetWindowFullscreen {
                id,
                fullscreen,
                response_tx,
            } => {
                if let Some(entity) = registry.get_entity(id) {
                    if let Ok(mut window) = windows.get_mut(entity) {
                        window.mode = if fullscreen {
                            WindowMode::BorderlessFullscreen(MonitorSelection::Current)
                        } else {
                            WindowMode::Windowed
                        };
                        let _ = response_tx.send(Ok(()));
                    } else {
                        let _ = response_tx.send(Err(format!("Window {} entity not found", id)));
                    }
                } else {
                    let _ = response_tx.send(Err(format!("Window {} not found", id)));
                }
            }

            GraphicCommand::SetWindowVisible {
                id,
                visible,
                response_tx,
            } => {
                if let Some(entity) = registry.get_entity(id) {
                    if let Ok(mut window) = windows.get_mut(entity) {
                        window.visible = visible;
                        let _ = response_tx.send(Ok(()));
                    } else {
                        let _ = response_tx.send(Err(format!("Window {} entity not found", id)));
                    }
                } else {
                    let _ = response_tx.send(Err(format!("Window {} not found", id)));
                }
            }

            // Note: SetWindowResizable was removed - resizable is set at window creation time

            GraphicCommand::Shutdown { response_tx } => {
                tracing::info!("Bevy engine received shutdown command");

                // Send shutting down event
                let _ = event_tx.0.try_send(GraphicEvent::EngineShuttingDown);

                let _ = response_tx.send(Ok(()));

                // Request app exit
                app_exit.write(bevy::app::AppExit::Success);
            }

            GraphicCommand::GetEngineInfo { response_tx } => {
                // Create and send engine info
                let info = GraphicEngineInfo {
                    engine_type: "Bevy".to_string(),
                    engine_type_id: GraphicEngines::Bevy.to_u32(),
                    name: "Bevy Game Engine".to_string(),
                    version: "0.17.3".to_string(),
                    description: "A refreshingly simple data-driven game engine built in Rust"
                        .to_string(),
                    features: vec![
                        "ECS".to_string(),
                        "2D Rendering".to_string(),
                        "3D Rendering".to_string(),
                        "UI System".to_string(),
                        "Sprite Rendering".to_string(),
                        "Text Rendering".to_string(),
                        "Input Handling".to_string(),
                        "Window Management".to_string(),
                        "Asset Loading".to_string(),
                    ],
                    backend: detect_rendering_backend(),
                    supports_2d: true,
                    supports_3d: true,
                    supports_ui: true,
                    supports_audio: false,
                };
                let _ = response_tx.send(info);
            }

            // ================================================================
            // Widget Commands
            // ================================================================
            GraphicCommand::CreateWidget {
                window_id,
                widget_id,
                parent_id,
                widget_type,
                config,
                response_tx,
            } => {
                // tracing::debug!(
                //     "Creating widget {} (type: {:?}) in window {}",
                //     widget_id,
                //     widget_type,
                //     window_id
                // );

                // Ensure window has a root UI node
                let root_entity = if let Some(root) = widget_registry.get_window_root(window_id) {
                    root
                } else {
                    // Create root UI node for this window
                    // This root node fills the entire window and acts as the base for all UI
                    let mut root_cmd = commands.spawn((
                        Node {
                            width: Val::Percent(100.0),
                            height: Val::Percent(100.0),
                            flex_direction: bevy::ui::FlexDirection::Column,
                            justify_content: bevy::ui::JustifyContent::FlexStart,
                            align_items: bevy::ui::AlignItems::Stretch,
                            ..default()
                        },
                    ));

                    // Associate root UI node with the camera
                    if let Some(ref camera) = ui_camera {
                        root_cmd.insert(UiTargetCamera(camera.0));
                        tracing::debug!(
                            "Root UI node associated with camera {:?}",
                            camera.0
                        );
                    }

                    let root = root_cmd.id();
                    widget_registry.set_window_root(window_id, root);
                    tracing::debug!("Created root UI node for window {}", window_id);
                    root
                };

                // Determine parent entity
                let parent_entity = match parent_id {
                    Some(pid) => widget_registry.get_entity(pid).unwrap_or(root_entity),
                    None => root_entity,
                };

                // Create widget entity based on type
                // For now, we pass None for parent_font since we don't track parent widget's font
                // A full implementation would query the parent entity's WidgetFontConfig
                let widget_entity = create_widget_entity(
                    &mut commands,
                    widget_id,
                    window_id,
                    widget_type,
                    &config,
                    parent_entity,
                    &font_registry,
                    None, // TODO: Get parent widget's effective font config
                );

                widget_registry.register(widget_id, widget_entity);
                let _ = response_tx.send(Ok(()));

                // Send widget created event
                let _ = event_tx.0.try_send(GraphicEvent::WidgetCreated {
                    window_id,
                    widget_id,
                    widget_type,
                });
            }

            GraphicCommand::UpdateWidgetProperty {
                widget_id,
                property,
                value,
                response_tx,
            } => {
                if let Some(entity) = widget_registry.get_entity(widget_id) {
                    let result = update_widget_property(
                        entity,
                        &property,
                        &value,
                        &mut text_query,
                        &mut bg_color_query,
                        &mut node_query,
                        &mut text_color_query,
                        &mut button_query,
                        &mut commands,
                    );
                    let _ = response_tx.send(result);
                } else {
                    let _ = response_tx.send(Err(format!("Widget {} not found", widget_id)));
                }
            }

            GraphicCommand::UpdateWidgetConfig {
                widget_id,
                config,
                response_tx,
            } => {
                if let Some(entity) = widget_registry.get_entity(widget_id) {
                    // Update multiple properties from config
                    let mut errors = Vec::new();

                    if let Some(ref content) = config.content {
                        if let Err(e) = update_widget_property(
                            entity,
                            "content",
                            &PropertyValue::String(content.clone()),
                            &mut text_query,
                            &mut bg_color_query,
                            &mut node_query,
                            &mut text_color_query,
                            &mut button_query,
                            &mut commands,
                        ) {
                            errors.push(e);
                        }
                    }

                    if let Some(ref color) = config.background_color {
                        if let Err(e) = update_widget_property(
                            entity,
                            "backgroundColor",
                            &PropertyValue::Color(color.clone()),
                            &mut text_query,
                            &mut bg_color_query,
                            &mut node_query,
                            &mut text_color_query,
                            &mut button_query,
                            &mut commands,
                        ) {
                            errors.push(e);
                        }
                    }

                    if errors.is_empty() {
                        let _ = response_tx.send(Ok(()));
                    } else {
                        let _ = response_tx.send(Err(errors.join("; ")));
                    }
                } else {
                    let _ = response_tx.send(Err(format!("Widget {} not found", widget_id)));
                }
            }

            GraphicCommand::DestroyWidget {
                widget_id,
                response_tx,
            } => {
                if let Some(entity) = widget_registry.unregister(widget_id) {
                    commands.entity(entity).despawn();
                    let _ = response_tx.send(Ok(()));

                    // Note: We'd need to track window_id to send proper event
                    // For now, use window_id 0
                    let _ = event_tx.0.try_send(GraphicEvent::WidgetDestroyed {
                        window_id: 0,
                        widget_id,
                    });
                } else {
                    let _ = response_tx.send(Err(format!("Widget {} not found", widget_id)));
                }
            }

            GraphicCommand::ReparentWidget {
                widget_id,
                new_parent_id,
                response_tx,
            } => {
                if let Some(widget_entity) = widget_registry.get_entity(widget_id) {
                    let new_parent = match new_parent_id {
                        Some(pid) => widget_registry.get_entity(pid),
                        None => {
                            // Get window root - we'd need to track which window this widget belongs to
                            // For simplicity, just use the first window root
                            widget_registry.window_roots.values().next().copied()
                        }
                    };

                    if let Some(parent_entity) = new_parent {
                        commands.entity(widget_entity).insert(ChildOf(parent_entity));
                        let _ = response_tx.send(Ok(()));
                    } else {
                        let _ = response_tx.send(Err("Parent widget not found".to_string()));
                    }
                } else {
                    let _ = response_tx.send(Err(format!("Widget {} not found", widget_id)));
                }
            }

            GraphicCommand::ClearWindowWidgets {
                window_id,
                response_tx,
            } => {
                // Remove root node for this window (despawn will also remove all children)
                if let Some(root_entity) = widget_registry.window_roots.remove(&window_id) {
                    commands.entity(root_entity).despawn();
                }

                // Remove all widgets that belong to this window from registry
                // Note: This requires tracking window_id in widget registry
                // For now, just clear the root
                let _ = response_tx.send(Ok(()));
            }

            GraphicCommand::SubscribeWidgetEvents {
                widget_id,
                event_types,
                response_tx,
            } => {
                // Widget event subscriptions are tracked on the GraphicProxy side
                // We just acknowledge the command here
                // In a more sophisticated implementation, we'd update the WidgetEventSubscriptions component
                let _ = response_tx.send(Ok(()));
            }

            GraphicCommand::UnsubscribeWidgetEvents {
                widget_id,
                event_types,
                response_tx,
            } => {
                let _ = response_tx.send(Ok(()));
            }

            // ================================================================
            // Window Font Command
            // ================================================================
            GraphicCommand::SetWindowFont {
                id,
                family,
                size,
                response_tx,
            } => {
                tracing::debug!("Setting window {} font: {} size {}", id, family, size);
                font_registry.set_window_font(id, family, size);
                let _ = response_tx.send(Ok(()));
            }

            // ================================================================
            // Asset Commands
            // ================================================================
            GraphicCommand::LoadFont {
                path,
                alias,
                response_tx,
            } => {
                // Generate alias from path if not provided
                let assigned_alias = alias.unwrap_or_else(|| {
                    std::path::Path::new(&path)
                        .file_stem()
                        .and_then(|s| s.to_str())
                        .unwrap_or("font")
                        .to_string()
                });

                // Load font via AssetServer
                let font_handle: Handle<Font> = asset_server.load(&path);
                font_registry.register_font(assigned_alias.clone(), font_handle);

                // tracing::debug!("Font registered: {} as \"{}\"", path, assigned_alias);
                let _ = response_tx.send(Ok(assigned_alias));
            }

            GraphicCommand::UnloadFont { alias, response_tx } => {
                font_registry.unregister_font(&alias);
                // tracing::debug!("Font unloaded: \"{}\"", alias);
                let _ = response_tx.send(Ok(()));
            }

            GraphicCommand::PreloadImage { path, response_tx } => {
                // Preload image via AssetServer
                let _: Handle<Image> = asset_server.load(&path);
                // tracing::debug!("Image preloaded: {}", path);
                let _ = response_tx.send(Ok(()));
            }
        }
    }
}

/// Create a widget entity based on type
fn create_widget_entity(
    commands: &mut Commands,
    widget_id: u64,
    window_id: u64,
    widget_type: WidgetType,
    config: &WidgetConfig,
    parent_entity: Entity,
    font_registry: &FontRegistry,
    parent_font: Option<&InheritedFontConfig>,
) -> Entity {
    // Resolve font configuration with inheritance
    // Priority: widget config > parent widget > window default > global default
    let window_font = font_registry.get_window_font(window_id);
    let base_font = parent_font.unwrap_or(&window_font);

    // Build widget's font config from config.font
    let widget_font_config = WidgetFontConfig {
        family: config.font.as_ref().and_then(|f| {
            if f.family.is_empty() || f.family == "default" {
                None
            } else {
                Some(f.family.clone())
            }
        }),
        size: config.font.as_ref().map(|f| f.size),
    };

    // Resolve the effective font for this widget
    let effective_font = widget_font_config.resolve(base_font);

    // Get the font handle (if loaded), or None for default
    let font_handle = font_registry.get_font(&effective_font.family);

    // Build base Node component from config
    let mut node = Node::default();

    // Apply dimensions
    if let Some(ref width) = config.width {
        node.width = size_value_to_val(width);
    }
    if let Some(ref height) = config.height {
        node.height = size_value_to_val(height);
    }

    // Apply layout
    if let Some(ref direction) = config.direction {
        node.flex_direction = match direction {
            FlexDirection::Row => bevy::ui::FlexDirection::Row,
            FlexDirection::Column => bevy::ui::FlexDirection::Column,
            FlexDirection::RowReverse => bevy::ui::FlexDirection::RowReverse,
            FlexDirection::ColumnReverse => bevy::ui::FlexDirection::ColumnReverse,
        };
    }

    if let Some(ref justify) = config.justify_content {
        node.justify_content = match justify {
            JustifyContent::Default => bevy::ui::JustifyContent::Default,
            JustifyContent::FlexStart => bevy::ui::JustifyContent::FlexStart,
            JustifyContent::FlexEnd => bevy::ui::JustifyContent::FlexEnd,
            JustifyContent::Center => bevy::ui::JustifyContent::Center,
            JustifyContent::SpaceBetween => bevy::ui::JustifyContent::SpaceBetween,
            JustifyContent::SpaceAround => bevy::ui::JustifyContent::SpaceAround,
            JustifyContent::SpaceEvenly => bevy::ui::JustifyContent::SpaceEvenly,
            JustifyContent::Stretch => bevy::ui::JustifyContent::Stretch,
            JustifyContent::Start => bevy::ui::JustifyContent::Start,
            JustifyContent::End => bevy::ui::JustifyContent::End,
        };
    }

    if let Some(ref align) = config.align_items {
        node.align_items = match align {
            AlignItems::Default => bevy::ui::AlignItems::Default,
            AlignItems::Stretch => bevy::ui::AlignItems::Stretch,
            AlignItems::FlexStart => bevy::ui::AlignItems::FlexStart,
            AlignItems::FlexEnd => bevy::ui::AlignItems::FlexEnd,
            AlignItems::Center => bevy::ui::AlignItems::Center,
            AlignItems::Baseline => bevy::ui::AlignItems::Baseline,
            AlignItems::Start => bevy::ui::AlignItems::Start,
            AlignItems::End => bevy::ui::AlignItems::End,
        };
    }

    // Apply spacing
    if let Some(ref margin) = config.margin {
        node.margin = edge_insets_to_ui_rect(margin);
    }
    if let Some(ref padding) = config.padding {
        node.padding = edge_insets_to_ui_rect(padding);
    }
    if let Some(gap) = config.gap {
        node.row_gap = Val::Px(gap);
        node.column_gap = Val::Px(gap);
    }

    // Create widget based on type
    match widget_type {
        WidgetType::Container => {
            // For containers, ensure we have a default flex direction if not specified
            let mut container_node = node;
            if config.direction.is_none() {
                container_node.flex_direction = bevy::ui::FlexDirection::Column;
            }
            if config.align_items.is_none() {
                container_node.align_items = bevy::ui::AlignItems::Stretch;
            }
            // Ensure containers without explicit dimensions can still contain children
            // by using Auto sizing which allows growth based on content
            if config.width.is_none() {
                container_node.width = Val::Auto;
            }
            if config.height.is_none() {
                container_node.height = Val::Auto;
            }

            let mut entity_cmd = commands.spawn((
                container_node,
                StamWidget {
                    id: widget_id,
                    window_id,
                    widget_type,
                },
                WidgetEventSubscriptions::default(),
            ));

            if let Some(ref color) = config.background_color {
                entity_cmd.insert(BackgroundColor(color_value_to_bevy(color)));
            }

            // Apply border radius if specified
            if let Some(radius) = config.border_radius {
                entity_cmd.insert(bevy::ui::BorderRadius::all(Val::Px(radius)));
            }

            entity_cmd.insert(ChildOf(parent_entity));
            entity_cmd.id()
        }

        WidgetType::Text => {
            let content = config.content.clone().unwrap_or_default();
            // Use effective_font which has inheritance applied
            let font_size = effective_font.size;
            let color = config
                .font_color
                .as_ref()
                .map(color_value_to_bevy)
                .unwrap_or(Color::WHITE);

            // tracing::debug!(
            //     "Creating Text widget: content='{}', font='{}', font_size={}, color={:?}, parent={:?}",
            //     content,
            //     effective_font.family,
            //     font_size,
            //     color,
            //     parent_entity
            // );

            // In Bevy 0.15, Text UI needs a properly configured Node for layout
            // Use the node from config (which may have width/height) or create one with auto sizing
            let mut text_node = node;
            // Ensure text can grow to fit content
            if config.width.is_none() {
                text_node.width = Val::Auto;
            }
            if config.height.is_none() {
                text_node.height = Val::Auto;
            }

            // Build TextFont with the appropriate font handle
            let text_font = if let Some(ref handle) = font_handle {
                TextFont {
                    font: handle.clone(),
                    font_size,
                    ..default()
                }
            } else {
                TextFont {
                    font_size,
                    ..default()
                }
            };

            let mut entity_cmd = commands.spawn((
                text_node,
                Text::new(content),
                TextColor(color),
                text_font,
                StamWidget {
                    id: widget_id,
                    window_id,
                    widget_type,
                },
                WidgetEventSubscriptions::default(),
                widget_font_config.clone(), // Store for child inheritance
            ));

            // Add background color if specified (transparent by default in Bevy 0.17)
            if let Some(ref bg_color) = config.background_color {
                entity_cmd.insert(BackgroundColor(color_value_to_bevy(bg_color)));
            }

            // Apply border radius if specified
            if let Some(radius) = config.border_radius {
                entity_cmd.insert(bevy::ui::BorderRadius::all(Val::Px(radius)));
            }

            let entity = entity_cmd.id();
            commands.entity(entity).insert(ChildOf(parent_entity));

            //tracing::debug!("Text widget entity {:?} created and parented", entity);
            entity
        }

        WidgetType::Button => {
            let label = config.label.clone().unwrap_or_default();

            let normal = config
                .background_color
                .as_ref()
                .map(color_value_to_bevy)
                .unwrap_or(Color::srgb(0.3, 0.3, 0.3));
            let hovered = config
                .hover_color
                .as_ref()
                .map(color_value_to_bevy)
                .unwrap_or(Color::srgb(0.4, 0.4, 0.4));
            let pressed = config
                .pressed_color
                .as_ref()
                .map(color_value_to_bevy)
                .unwrap_or(Color::srgb(0.2, 0.2, 0.2));
            let disabled = config
                .disabled_color
                .as_ref()
                .map(color_value_to_bevy)
                .unwrap_or(Color::srgb(0.5, 0.5, 0.5));

            // Set default button styling
            let mut button_node = node.clone();
            button_node.padding = UiRect::all(Val::Px(10.0));
            button_node.justify_content = bevy::ui::JustifyContent::Center;
            button_node.align_items = bevy::ui::AlignItems::Center;

            // Build TextFont with inherited font configuration
            let text_font = if let Some(handle) = font_handle.clone() {
                TextFont {
                    font: handle,
                    font_size: effective_font.size,
                    ..default()
                }
            } else {
                TextFont {
                    font_size: effective_font.size,
                    ..default()
                }
            };

            let mut button_cmd = commands.spawn((
                button_node,
                Button,
                BackgroundColor(normal),
                ButtonColors {
                    normal,
                    hovered,
                    pressed,
                    disabled,
                },
                StamWidget {
                    id: widget_id,
                    window_id,
                    widget_type,
                },
                WidgetEventSubscriptions {
                    on_click: true,
                    on_hover: true,
                    ..default()
                },
                PreviousInteraction::default(),
                widget_font_config.clone(), // Store for child inheritance
            ));

            // Apply border radius if specified
            if let Some(radius) = config.border_radius {
                button_cmd.insert(bevy::ui::BorderRadius::all(Val::Px(radius)));
            }

            let button_entity = button_cmd
                .with_children(|parent| {
                    parent.spawn((
                        Text::new(label),
                        TextColor(Color::WHITE),
                        text_font,
                    ));
                })
                .insert(ChildOf(parent_entity))
                .id();

            button_entity
        }

        WidgetType::Panel => {
            let bg_color = config
                .background_color
                .as_ref()
                .map(color_value_to_bevy)
                .unwrap_or(Color::srgba(0.2, 0.2, 0.2, 0.8));

            let mut entity_cmd = commands.spawn((
                node,
                BackgroundColor(bg_color),
                StamWidget {
                    id: widget_id,
                    window_id,
                    widget_type,
                },
                WidgetEventSubscriptions::default(),
                ChildOf(parent_entity),
            ));

            // Apply border radius if specified
            if let Some(radius) = config.border_radius {
                entity_cmd.insert(bevy::ui::BorderRadius::all(Val::Px(radius)));
            }

            entity_cmd.id()
        }

        WidgetType::Image => {
            // For now, just create a placeholder node
            // Full image support would require AssetServer integration
            let bg_color = config
                .background_color
                .as_ref()
                .map(color_value_to_bevy)
                .unwrap_or(Color::srgba(0.5, 0.5, 0.5, 1.0));

            let mut entity_cmd = commands.spawn((
                node,
                BackgroundColor(bg_color),
                StamWidget {
                    id: widget_id,
                    window_id,
                    widget_type,
                },
                WidgetEventSubscriptions::default(),
                ChildOf(parent_entity),
            ));

            // Apply border radius if specified
            if let Some(radius) = config.border_radius {
                entity_cmd.insert(bevy::ui::BorderRadius::all(Val::Px(radius)));
            }

            entity_cmd.id()
        }
    }
}

/// Update a widget property
fn update_widget_property(
    entity: Entity,
    property: &str,
    value: &PropertyValue,
    text_query: &mut Query<&mut Text>,
    bg_color_query: &mut Query<&mut BackgroundColor>,
    node_query: &mut Query<&mut Node>,
    text_color_query: &mut Query<&mut TextColor>,
    button_query: &mut Query<(&mut ButtonColors, Option<&Interaction>, Option<&Children>)>,
    commands: &mut Commands,
) -> Result<(), String> {
    match property {
        "content" | "label" => {
            if let PropertyValue::String(text) = value {
                // First try direct text component
                if let Ok(mut text_component) = text_query.get_mut(entity) {
                    *text_component = Text::new(text.clone());
                    return Ok(());
                }
                // For buttons, the text is on a child entity
                if let Ok((_, _, Some(children))) = button_query.get(entity) {
                    for child in children.iter() {
                        if let Ok(mut text_component) = text_query.get_mut(child) {
                            *text_component = Text::new(text.clone());
                            return Ok(());
                        }
                    }
                }
            }
            Err(format!("Cannot update {} on this widget", property))
        }
        "backgroundColor" => {
            if let PropertyValue::Color(color) = value {
                let bevy_color = color_value_to_bevy(color);

                // Update the background color
                if let Ok(mut bg) = bg_color_query.get_mut(entity) {
                    *bg = BackgroundColor(bevy_color);

                    // For buttons, also update the 'normal' state color
                    // so that when interaction changes back to None, it uses the new color
                    if let Ok((mut button_colors, _, _)) = button_query.get_mut(entity) {
                        button_colors.normal = bevy_color;
                    }

                    return Ok(());
                }
            }
            Err(format!("Cannot update {} on this widget", property))
        }
        "fontColor" => {
            if let PropertyValue::Color(color) = value {
                // First try direct text color component
                if let Ok(mut text_color) = text_color_query.get_mut(entity) {
                    *text_color = TextColor(color_value_to_bevy(color));
                    return Ok(());
                }
                // For buttons, the text color is on a child entity
                if let Ok((_, _, Some(children))) = button_query.get(entity) {
                    for child in children.iter() {
                        if let Ok(mut text_color) = text_color_query.get_mut(child) {
                            *text_color = TextColor(color_value_to_bevy(color));
                            return Ok(());
                        }
                    }
                }
            }
            Err(format!("Cannot update {} on this widget", property))
        }
        "width" => {
            if let PropertyValue::Size(size) = value {
                if let Ok(mut node) = node_query.get_mut(entity) {
                    node.width = size_value_to_val(size);
                    return Ok(());
                }
            }
            Err(format!("Cannot update {} on this widget", property))
        }
        "height" => {
            if let PropertyValue::Size(size) = value {
                if let Ok(mut node) = node_query.get_mut(entity) {
                    node.height = size_value_to_val(size);
                    return Ok(());
                }
            }
            Err(format!("Cannot update {} on this widget", property))
        }
        "disabled" => {
            if let PropertyValue::Bool(disabled) = value {
                // For buttons, update the background color based on disabled state
                if let Ok((button_colors, _, _)) = button_query.get(entity) {
                    let new_color = if *disabled {
                        button_colors.disabled
                    } else {
                        button_colors.normal
                    };
                    if let Ok(mut bg) = bg_color_query.get_mut(entity) {
                        *bg = BackgroundColor(new_color);

                        // Add or remove the ButtonDisabled marker component
                        if *disabled {
                            commands.entity(entity).insert(ButtonDisabled);
                        } else {
                            commands.entity(entity).remove::<ButtonDisabled>();
                        }

                        return Ok(());
                    }
                }
            }
            Err(format!("Cannot update {} on this widget", property))
        }
        "hoverColor" => {
            if let PropertyValue::Color(color) = value {
                if let Ok((mut button_colors, interaction_opt, _)) = button_query.get_mut(entity) {
                    let new_color = color_value_to_bevy(color);
                    button_colors.hovered = new_color;

                    // If the button is currently hovered, update the background color immediately
                    if let Some(interaction) = interaction_opt {
                        if *interaction == Interaction::Hovered {
                            if let Ok(mut bg) = bg_color_query.get_mut(entity) {
                                *bg = BackgroundColor(new_color);
                            }
                        }
                    }

                    return Ok(());
                }
            }
            Err(format!("Cannot update {} on this widget", property))
        }
        "pressedColor" => {
            if let PropertyValue::Color(color) = value {
                if let Ok((mut button_colors, interaction_opt, _)) = button_query.get_mut(entity) {
                    let new_color = color_value_to_bevy(color);
                    button_colors.pressed = new_color;

                    // If the button is currently pressed, update the background color immediately
                    if let Some(interaction) = interaction_opt {
                        if *interaction == Interaction::Pressed {
                            if let Ok(mut bg) = bg_color_query.get_mut(entity) {
                                *bg = BackgroundColor(new_color);
                            }
                        }
                    }

                    return Ok(());
                }
            }
            Err(format!("Cannot update {} on this widget", property))
        }
        "disabledColor" => {
            if let PropertyValue::Color(color) = value {
                if let Ok((mut button_colors, _, _)) = button_query.get_mut(entity) {
                    button_colors.disabled = color_value_to_bevy(color);
                    return Ok(());
                }
            }
            Err(format!("Cannot update {} on this widget", property))
        }
        "borderRadius" => {
            if let PropertyValue::Number(radius) = value {
                // Insert or update the BorderRadius component
                commands.entity(entity).insert(bevy::ui::BorderRadius::all(Val::Px(*radius as f32)));
                return Ok(());
            }
            Err(format!("Cannot update {} on this widget", property))
        }
        _ => Err(format!("Unknown property: {}", property)),
    }
}

/// Resource to track if EngineReady has been sent
#[derive(Resource, Default)]
struct EngineReadySent(bool);

/// Constant for the primary/main window ID
const PRIMARY_WINDOW_ID: u64 = 1;

/// Startup system to register the primary window in our registry
///
/// Bevy creates a primary window when using `primary_window: Some(...)`.
/// This system finds that window and registers it with ID 1 (PRIMARY_WINDOW_ID)
/// so that JS code can modify it via `mainWindow` from `getEngineInfo()`.
///
/// Also spawns a default 2D camera for UI rendering. Bevy UI requires a camera
/// to render properly. This camera will be used until a mod decides to replace it.
fn register_primary_window(
    mut commands: Commands,
    mut registry: ResMut<WindowRegistry>,
    primary_window_query: Query<Entity, With<PrimaryWindow>>,
) {
    if let Ok(entity) = primary_window_query.single() {
        registry.register(PRIMARY_WINDOW_ID, entity);
        tracing::debug!("Registered primary window as ID {}", PRIMARY_WINDOW_ID);

        // Spawn a default 2D camera for UI rendering
        // Bevy UI requires at least one camera in the scene
        let camera_entity = commands.spawn(Camera2d).id();
        commands.insert_resource(UiCamera(camera_entity));
        tracing::debug!(
            "Spawned default 2D camera {:?} for UI rendering",
            camera_entity
        );
    } else {
        tracing::warn!("No primary window found to register");
    }
}

/// System to send EngineReady event after Bevy is fully running
///
/// This runs in the Update schedule after the first frame, ensuring that
/// the command processor is running before any JS handlers try to call
/// graphic API methods like createWindow().
///
/// We use a flag resource to ensure it only fires once.
/// We use try_send to avoid blocking the main thread.
fn send_engine_ready_event(event_tx: Res<EventSenderRes>, mut sent: ResMut<EngineReadySent>) {
    if !sent.0 {
        sent.0 = true;
        // Use try_send to avoid blocking - the worker thread should have the receiver ready
        let _ = event_tx.0.try_send(GraphicEvent::EngineReady);
    }
}

/// System to send frame events
fn send_frame_events(event_tx: Res<EventSenderRes>, time: Res<Time>) {
    // Send frame start event (window_id 0 = all/primary)
    let _ = event_tx.0.try_send(GraphicEvent::FrameStart {
        window_id: 0,
        delta_time: time.delta_secs(),
    });
}

/// System to handle keyboard input
fn handle_keyboard_input(
    event_tx: Res<EventSenderRes>,
    keyboard: Res<ButtonInput<KeyCode>>,
) {
    // Get the primary window ID (or default to 1)
    let window_id = 1u64; // TODO: proper multi-window support

    // Get current modifiers
    let modifiers = KeyModifiers {
        shift: keyboard.pressed(KeyCode::ShiftLeft) || keyboard.pressed(KeyCode::ShiftRight),
        ctrl: keyboard.pressed(KeyCode::ControlLeft) || keyboard.pressed(KeyCode::ControlRight),
        alt: keyboard.pressed(KeyCode::AltLeft) || keyboard.pressed(KeyCode::AltRight),
        meta: keyboard.pressed(KeyCode::SuperLeft) || keyboard.pressed(KeyCode::SuperRight),
    };

    // Send key pressed events
    for key in keyboard.get_just_pressed() {
        let key_name = format!("{:?}", key);
        let _ = event_tx.0.try_send(GraphicEvent::KeyPressed {
            window_id,
            key: key_name,
            modifiers: modifiers.clone(),
        });
    }

    // Send key released events
    for key in keyboard.get_just_released() {
        let key_name = format!("{:?}", key);
        let _ = event_tx.0.try_send(GraphicEvent::KeyReleased {
            window_id,
            key: key_name,
            modifiers: modifiers.clone(),
        });
    }
}

/// System to handle mouse input
fn handle_mouse_input(
    event_tx: Res<EventSenderRes>,
    mouse_button: Res<ButtonInput<bevy::input::mouse::MouseButton>>,
    windows: Query<&Window, With<PrimaryWindow>>,
) {
    let window_id = 1u64; // TODO: proper multi-window support

    // Get cursor position
    let (x, y) = if let Ok(window) = windows.single() {
        window
            .cursor_position()
            .map(|pos| (pos.x, pos.y))
            .unwrap_or((0.0, 0.0))
    } else {
        (0.0, 0.0)
    };

    // Send mouse button pressed events
    for button in mouse_button.get_just_pressed() {
        let btn = match button {
            bevy::input::mouse::MouseButton::Left => MouseButton::Left,
            bevy::input::mouse::MouseButton::Right => MouseButton::Right,
            bevy::input::mouse::MouseButton::Middle => MouseButton::Middle,
            bevy::input::mouse::MouseButton::Back => MouseButton::Other(3),
            bevy::input::mouse::MouseButton::Forward => MouseButton::Other(4),
            bevy::input::mouse::MouseButton::Other(n) => MouseButton::Other(*n as u8),
        };
        let _ = event_tx.0.try_send(GraphicEvent::MouseButtonPressed {
            window_id,
            button: btn,
            x,
            y,
        });
    }

    // Send mouse button released events
    for button in mouse_button.get_just_released() {
        let btn = match button {
            bevy::input::mouse::MouseButton::Left => MouseButton::Left,
            bevy::input::mouse::MouseButton::Right => MouseButton::Right,
            bevy::input::mouse::MouseButton::Middle => MouseButton::Middle,
            bevy::input::mouse::MouseButton::Back => MouseButton::Other(3),
            bevy::input::mouse::MouseButton::Forward => MouseButton::Other(4),
            bevy::input::mouse::MouseButton::Other(n) => MouseButton::Other(*n as u8),
        };
        let _ = event_tx.0.try_send(GraphicEvent::MouseButtonReleased {
            window_id,
            button: btn,
            x,
            y,
        });
    }
}

/// System to handle window events
fn handle_window_events(
    event_tx: Res<EventSenderRes>,
    registry: Res<WindowRegistry>,
    mut resize_events: EventReader<bevy::window::WindowResized>,
    mut focus_events: EventReader<bevy::window::WindowFocused>,
    mut moved_events: EventReader<bevy::window::WindowMoved>,
    mut close_requested_events: EventReader<bevy::window::WindowCloseRequested>,
) {
    for event in resize_events.read() {
        if let Some(window_id) = registry.get_id(event.window) {
            let _ = event_tx.0.try_send(GraphicEvent::WindowResized {
                window_id,
                width: event.width as u32,
                height: event.height as u32,
            });
        }
    }

    for event in focus_events.read() {
        if let Some(window_id) = registry.get_id(event.window) {
            let _ = event_tx.0.try_send(GraphicEvent::WindowFocused {
                window_id,
                focused: event.focused,
            });
        }
    }

    for event in moved_events.read() {
        if let Some(window_id) = registry.get_id(event.window) {
            let _ = event_tx.0.try_send(GraphicEvent::WindowMoved {
                window_id,
                x: event.position.x,
                y: event.position.y,
            });
        }
    }

    // Handle window close requests (when user clicks the X button)
    for event in close_requested_events.read() {
        if let Some(window_id) = registry.get_id(event.window) {
            tracing::debug!("Window {} close requested by user", window_id);
            let _ = event_tx.0.try_send(GraphicEvent::WindowClosed { window_id });
        }
    }
}

/// System to handle widget interactions (buttons, hover, etc.)
fn handle_widget_interactions(
    event_tx: Res<EventSenderRes>,
    mut query: Query<
        (
            &Interaction,
            &mut BackgroundColor,
            &ButtonColors,
            &StamWidget,
            &WidgetEventSubscriptions,
            &mut PreviousInteraction,
        ),
        (Changed<Interaction>, With<Button>, Without<ButtonDisabled>),
    >,
    windows: Query<&Window, With<PrimaryWindow>>,
) {
    // Get cursor position for click events
    let cursor_pos = windows
        .single()
        .ok()
        .and_then(|w| w.cursor_position())
        .unwrap_or(Vec2::ZERO);

    for (interaction, mut bg_color, colors, widget, subs, mut prev_interaction) in query.iter_mut()
    {
        let prev = prev_interaction.0;
        prev_interaction.0 = *interaction;

        match *interaction {
            Interaction::Pressed => {
                *bg_color = BackgroundColor(colors.pressed);

                // Send click event if subscribed
                if subs.on_click {
                    let _ = event_tx.0.try_send(GraphicEvent::WidgetClicked {
                        window_id: widget.window_id,
                        widget_id: widget.id,
                        x: cursor_pos.x,
                        y: cursor_pos.y,
                        button: MouseButton::Left,
                    });
                }
            }
            Interaction::Hovered => {
                *bg_color = BackgroundColor(colors.hovered);

                // Send hover enter event if subscribed and wasn't hovered before
                if subs.on_hover && prev != Interaction::Hovered {
                    let _ = event_tx.0.try_send(GraphicEvent::WidgetHovered {
                        window_id: widget.window_id,
                        widget_id: widget.id,
                        entered: true,
                        x: cursor_pos.x,
                        y: cursor_pos.y,
                    });
                }
            }
            Interaction::None => {
                *bg_color = BackgroundColor(colors.normal);

                // Send hover leave event if subscribed and was hovered before
                if subs.on_hover && prev == Interaction::Hovered {
                    let _ = event_tx.0.try_send(GraphicEvent::WidgetHovered {
                        window_id: widget.window_id,
                        widget_id: widget.id,
                        entered: false,
                        x: cursor_pos.x,
                        y: cursor_pos.y,
                    });
                }
            }
        }
    }
}
