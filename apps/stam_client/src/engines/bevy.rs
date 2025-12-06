//! Bevy Engine Implementation
//!
//! This module implements the `GraphicEngine` trait for the Bevy game engine.
//! Bevy runs on the main thread and communicates with the worker thread via channels.

use bevy::prelude::*;
use bevy::window::{PrimaryWindow, VideoModeSelection, WindowMode, WindowResolution};
use bevy::winit::{UpdateMode, WinitSettings, WINIT_WINDOWS};
use std::collections::HashMap;
use std::sync::mpsc::Receiver;
use std::sync::Mutex;
use tokio::sync::mpsc::Sender;

use stam_mod_runtimes::api::{
    ColorValue, EdgeInsets, FlexDirection, GraphicCommand, GraphicEngine, GraphicEngineInfo,
    GraphicEngines, GraphicEvent, InitialWindowConfig, JustifyContent, KeyModifiers, MouseButton,
    PropertyValue, SizeValue, WidgetConfig, WidgetEventType, WidgetType, WindowPositionMode,
    AlignItems, WindowMode as StamWindowMode, ResourceType, ResourceState, ResourceInfo,
    ImageScaleMode, ImageSource,
};

/// System sets for ordering Bevy systems
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
enum BevySystemSet {
    /// Process commands from the worker thread (first)
    ProcessCommands,
    /// Systems that run after commands are processed
    AfterCommands,
}

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
        app.insert_resource(ResourceRegistry::default());
        app.insert_resource(PendingAssetRegistry::default());
        app.insert_resource(EngineReadySent::default());

        // Force continuous updates even without windows or when unfocused
        // This ensures the Update schedule runs continuously to process commands
        app.insert_resource(WinitSettings {
            focused_mode: UpdateMode::Continuous,
            unfocused_mode: UpdateMode::Continuous,
        });

        // Add startup system to register the primary window in our registry
        app.add_systems(Startup, register_primary_window);

        // Configure system set ordering
        app.configure_sets(
            Update,
            BevySystemSet::AfterCommands.after(BevySystemSet::ProcessCommands),
        );

        // Add our systems with explicit ordering via SystemSets
        // send_engine_ready_event and check_pending_assets run AFTER process_commands
        // to ensure the command channel is being processed before JS handlers
        // try to call createWindow() in response to EngineReady
        //
        // Note: process_commands has too many parameters to use .in_set() directly.
        // We use a separate add_systems call and configure the set to run first.
        app.add_systems(Update, process_commands);
        app.add_systems(
            Update,
            (
                send_engine_ready_event,
                check_pending_assets,
                send_frame_events,
                handle_keyboard_input,
                handle_mouse_input,
                handle_window_events,
                handle_widget_interactions,
                update_cover_contain_images,
            ).in_set(BevySystemSet::AfterCommands),
        );

        // Ensure AfterCommands runs after the default ordering (process_commands)
        // by adding an explicit First/Last ordering
        app.configure_sets(
            Update,
            BevySystemSet::ProcessCommands.before(BevySystemSet::AfterCommands),
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

/// Marker component for images that need Cover/Contain scaling
///
/// This component is added to Image widgets that use Cover or Contain scale modes.
/// A system runs each frame to update the widget's size based on:
/// - The actual image dimensions (once loaded)
/// - The parent container's size
#[derive(Component, Clone, Debug)]
struct CoverContainImage {
    /// The scale mode (Cover or Contain)
    scale_mode: ImageScaleMode,
    /// The image handle to get dimensions from
    image_handle: Handle<Image>,
    /// Whether the image dimensions have been applied
    applied: bool,
}

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

// ============================================================================
// Resource Registry
// ============================================================================

/// Entry in the resource registry
struct ResourceRegistryEntry {
    /// Unique asset ID (matches ResourceProxy's EngineHandle)
    asset_id: u64,
    /// Resource alias
    alias: String,
    /// Original path
    path: String,
    /// Resource type
    resource_type: ResourceType,
    /// Handle to the loaded asset (keeps it alive)
    handle: ResourceHandle,
}

/// Enum holding different types of Bevy asset handles
#[derive(Clone)]
enum ResourceHandle {
    Image(Handle<Image>),
    Font(Handle<Font>),
    // Audio(Handle<AudioSource>), // Add when audio is enabled
    // TODO: Add other handle types as needed
}

/// Registry for loaded resources (images, fonts, etc.)
///
/// This registry holds handles to loaded assets, preventing Bevy from
/// garbage collecting them. Resources are identified by their asset_id
/// (which matches ResourceProxy's EngineHandle).
#[derive(Resource, Default)]
struct ResourceRegistry {
    /// Map from asset_id to resource entry
    entries: HashMap<u64, ResourceRegistryEntry>,
    /// Map from alias to asset_id (for quick lookup by name)
    alias_to_id: HashMap<String, u64>,
}

impl ResourceRegistry {
    /// Register a new resource
    fn register(
        &mut self,
        asset_id: u64,
        alias: String,
        path: String,
        resource_type: ResourceType,
        handle: ResourceHandle,
    ) {
        self.alias_to_id.insert(alias.clone(), asset_id);
        self.entries.insert(
            asset_id,
            ResourceRegistryEntry {
                asset_id,
                alias,
                path,
                resource_type,
                handle,
            },
        );
    }

    /// Unregister a resource by asset_id
    fn unregister(&mut self, asset_id: u64) -> Option<ResourceRegistryEntry> {
        if let Some(entry) = self.entries.remove(&asset_id) {
            self.alias_to_id.remove(&entry.alias);
            Some(entry)
        } else {
            None
        }
    }

    /// Clear all resources
    fn clear(&mut self) {
        self.entries.clear();
        self.alias_to_id.clear();
    }

    /// Get resource entry by asset_id
    fn get(&self, asset_id: u64) -> Option<&ResourceRegistryEntry> {
        self.entries.get(&asset_id)
    }

    /// Get resource entry by alias
    fn get_by_alias(&self, alias: &str) -> Option<&ResourceRegistryEntry> {
        self.alias_to_id
            .get(alias)
            .and_then(|id| self.entries.get(id))
    }

    /// Get image handle by alias (for widget use)
    fn get_image_handle(&self, alias: &str) -> Option<Handle<Image>> {
        self.get_by_alias(alias).and_then(|entry| {
            if let ResourceHandle::Image(handle) = &entry.handle {
                Some(handle.clone())
            } else {
                None
            }
        })
    }
}

// ============================================================================
// Pending Asset Tracker
// ============================================================================

/// Entry for a pending asset load (waiting for Bevy to finish loading)
struct PendingAssetEntry {
    /// Asset ID from ResourceProxy
    asset_id: u64,
    /// Resource alias
    alias: String,
    /// Untyped handle ID for checking load state
    handle_id: bevy::asset::UntypedAssetId,
}

/// Registry for assets that are loading in Bevy
///
/// When `asset_server.load()` is called, we add an entry here.
/// The `check_pending_assets` system checks `is_loaded_with_dependencies` for each entry
/// and sends a `ResourceLoaded` event when the asset is ready.
#[derive(Resource, Default)]
struct PendingAssetRegistry {
    /// List of assets waiting to be loaded
    pending: Vec<PendingAssetEntry>,
}

impl PendingAssetRegistry {
    /// Add a pending asset
    fn add(&mut self, asset_id: u64, alias: String, handle_id: bevy::asset::UntypedAssetId) {
        self.pending.push(PendingAssetEntry {
            asset_id,
            alias,
            handle_id,
        });
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
    channels: (NonSend<CommandReceiverRes>, Res<EventSenderRes>),
    mut commands: Commands,
    mut registries: (
        ResMut<WindowRegistry>,
        ResMut<WidgetRegistry>,
        ResMut<FontRegistry>,
        ResMut<ResourceRegistry>,
        ResMut<PendingAssetRegistry>,
    ),
    asset_server: Res<AssetServer>,
    mut windows: Query<&mut Window>,
    mut app_exit: EventWriter<bevy::app::AppExit>,
    primary_window_query: Query<Entity, With<PrimaryWindow>>,
    mut widget_queries: (
        Query<&mut Text>,
        Query<&mut BackgroundColor>,
        Query<&mut Node>,
        Query<&mut TextColor>,
        Query<(&mut ButtonColors, Option<&Interaction>, Option<&Children>)>,
    ),
    ui_camera: Option<Res<UiCamera>>,
) {
    let (cmd_rx, event_tx) = channels;
    let (registry, widget_registry, font_registry, resource_registry, pending_assets) = &mut registries;
    let (text_query, bg_color_query, node_query, text_color_query, button_query) = &mut widget_queries;
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

            GraphicCommand::SetWindowMode {
                id,
                mode,
                response_tx,
            } => {
                if let Some(entity) = registry.get_entity(id) {
                    if let Ok(mut window) = windows.get_mut(entity) {
                        window.mode = match mode {
                            StamWindowMode::Windowed => WindowMode::Windowed,
                            StamWindowMode::Fullscreen => WindowMode::Fullscreen(MonitorSelection::Current, VideoModeSelection::Current),
                            StamWindowMode::BorderlessFullscreen => WindowMode::BorderlessFullscreen(MonitorSelection::Current),
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
                    &resource_registry,
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
                        text_query,
                        bg_color_query,
                        node_query,
                        text_color_query,
                        button_query,
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
                            text_query,
                            bg_color_query,
                            node_query,
                            text_color_query,
                            button_query,
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
                            text_query,
                            bg_color_query,
                            node_query,
                            text_color_query,
                            button_query,
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

            // ================================================================
            // Screen/Monitor Commands
            // ================================================================
            GraphicCommand::GetPrimaryScreen { response_tx } => {
                // Get primary monitor via winit using thread_local WINIT_WINDOWS
                let result = (|| -> Result<u32, String> {
                    let primary_entity = primary_window_query
                        .single()
                        .map_err(|_| "No primary window found".to_string())?;

                    // Access WINIT_WINDOWS thread_local
                    WINIT_WINDOWS.with(|winit_windows| {
                        let winit_wins = winit_windows.borrow();
                        if let Some(winit_window) = winit_wins.get_window(primary_entity) {
                            if let Some(monitor) = winit_window.primary_monitor() {
                                // Use monitor name hash as ID, or 0 for primary
                                let monitor_id = monitor
                                    .name()
                                    .map(|name| {
                                        use std::hash::{Hash, Hasher};
                                        let mut hasher = std::collections::hash_map::DefaultHasher::new();
                                        name.hash(&mut hasher);
                                        hasher.finish() as u32
                                    })
                                    .unwrap_or(0);
                                return Ok(monitor_id);
                            }
                        }
                        // Fallback: return 0 as default primary screen ID
                        Ok(0)
                    })
                })();

                let _ = response_tx.send(result);
            }

            GraphicCommand::GetScreenResolution { screen_id, response_tx } => {
                // Get resolution of a specific screen/monitor using thread_local WINIT_WINDOWS
                let result = (|| -> Result<(u32, u32), String> {
                    let primary_entity = primary_window_query
                        .single()
                        .map_err(|_| "No primary window found".to_string())?;

                    // Access WINIT_WINDOWS thread_local
                    WINIT_WINDOWS.with(|winit_windows| {
                        let winit_wins = winit_windows.borrow();
                        if let Some(winit_window) = winit_wins.get_window(primary_entity) {
                            // For screen_id 0, try multiple methods to get primary monitor
                            if screen_id == 0 {
                                // Try primary_monitor first
                                if let Some(monitor) = winit_window.primary_monitor() {
                                    let size = monitor.size();
                                    return Ok((size.width, size.height));
                                }
                                // Fallback to current_monitor (the monitor the window is on)
                                if let Some(monitor) = winit_window.current_monitor() {
                                    let size = monitor.size();
                                    return Ok((size.width, size.height));
                                }
                                // Fallback to first available monitor
                                if let Some(monitor) = winit_window.available_monitors().next() {
                                    let size = monitor.size();
                                    return Ok((size.width, size.height));
                                }
                            }

                            // Search through available monitors for specific screen_id
                            for monitor in winit_window.available_monitors() {
                                let monitor_id = monitor
                                    .name()
                                    .map(|name| {
                                        use std::hash::{Hash, Hasher};
                                        let mut hasher = std::collections::hash_map::DefaultHasher::new();
                                        name.hash(&mut hasher);
                                        hasher.finish() as u32
                                    })
                                    .unwrap_or(0);

                                if monitor_id == screen_id {
                                    let size = monitor.size();
                                    return Ok((size.width, size.height));
                                }
                            }
                        }
                        Err(format!("Screen {} not found", screen_id))
                    })
                })();

                let _ = response_tx.send(result);
            }

            // ================================================================
            // Resource Commands
            // ================================================================
            GraphicCommand::LoadResource {
                path,
                alias,
                resource_type,
                asset_id,
                force_reload,
                response_tx,
            } => {
                tracing::debug!(
                    "Loading resource: path='{}', alias='{}', type={:?}, asset_id={}",
                    path, alias, resource_type, asset_id
                );

                // Check if already loaded (unless force_reload)
                if !force_reload {
                    if let Some(_existing) = resource_registry.get_by_alias(&alias) {
                        // Already loaded, return existing info
                        let info = ResourceInfo {
                            alias: alias.clone(),
                            path: path.clone(),
                            resolved_path: path.clone(),
                            resource_type,
                            state: ResourceState::Loaded,
                            size: None,
                            error: None,
                        };
                        let _ = response_tx.send(Ok(info));
                        continue;
                    }
                } else {
                    // Force reload: remove existing entry if any
                    if let Some(existing_id) = resource_registry.alias_to_id.get(&alias).copied() {
                        resource_registry.unregister(existing_id);
                    }
                }

                // Load the resource via AssetServer
                // NOTE: asset_server.load() is async - it returns a handle immediately
                // but the actual asset loading happens in the background.
                // We return ResourceState::Loading and track the asset in PendingAssetRegistry.
                // The check_pending_assets system will send ResourceLoaded event when done.
                let result = match resource_type {
                    ResourceType::Image => {
                        let handle: Handle<Image> = asset_server.load(&path);
                        let untyped_id = handle.id().untyped();
                        resource_registry.register(
                            asset_id,
                            alias.clone(),
                            path.clone(),
                            resource_type,
                            ResourceHandle::Image(handle),
                        );
                        // Track this asset as pending
                        pending_assets.add(asset_id, alias.clone(), untyped_id);
                        tracing::debug!(
                            "Resource '{}' (asset_id={}) queued for loading, tracking in PendingAssetRegistry",
                            alias, asset_id
                        );
                        Ok(ResourceInfo {
                            alias,
                            path: path.clone(),
                            resolved_path: path,
                            resource_type,
                            state: ResourceState::Loading, // NOT Loaded yet!
                            size: None,
                            error: None,
                        })
                    }
                    ResourceType::Font => {
                        let handle: Handle<Font> = asset_server.load(&path);
                        let untyped_id = handle.id().untyped();
                        resource_registry.register(
                            asset_id,
                            alias.clone(),
                            path.clone(),
                            resource_type,
                            ResourceHandle::Font(handle),
                        );
                        // Track this asset as pending
                        pending_assets.add(asset_id, alias.clone(), untyped_id);
                        tracing::debug!(
                            "Font '{}' (asset_id={}) queued for loading, tracking in PendingAssetRegistry",
                            alias, asset_id
                        );
                        Ok(ResourceInfo {
                            alias,
                            path: path.clone(),
                            resolved_path: path,
                            resource_type,
                            state: ResourceState::Loading, // NOT Loaded yet!
                            size: None,
                            error: None,
                        })
                    }
                    // TODO: Add Audio, Shader, Model3D handlers when Bevy features are enabled
                    _ => {
                        Err(format!(
                            "Resource type {:?} is not yet supported by Bevy engine",
                            resource_type
                        ))
                    }
                };

                let _ = response_tx.send(result);
            }

            GraphicCommand::UnloadResource { asset_id, response_tx } => {
                tracing::debug!("Unloading resource: asset_id={}", asset_id);

                if resource_registry.unregister(asset_id).is_some() {
                    let _ = response_tx.send(Ok(()));
                } else {
                    let _ = response_tx.send(Err(format!(
                        "Resource with asset_id {} not found",
                        asset_id
                    )));
                }
            }

            GraphicCommand::UnloadAllResources { response_tx } => {
                tracing::debug!("Unloading all resources");
                resource_registry.clear();
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
    resource_registry: &ResourceRegistry,
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
            // Get image configuration from config.image
            let image_config = config.image.as_ref();
            tracing::debug!("Creating Image widget: image_config={:?}", image_config);

            // Try to get image handle from resource_id or path
            let image_handle: Option<Handle<Image>> = image_config.and_then(|img_cfg| {
                tracing::debug!("Image effective_source: {:?}", img_cfg.effective_source());
                match img_cfg.effective_source() {
                    Some(ImageSource::ResourceId(ref resource_id)) => {
                        // Look up pre-loaded resource by alias
                        let handle = resource_registry.get_image_handle(resource_id);
                        tracing::debug!("Looking up resource_id '{}': handle={:?}", resource_id, handle.is_some());
                        handle
                    }
                    Some(ImageSource::Path(_path)) => {
                        // Direct path loading would require AssetServer access here
                        // For now, resources must be pre-loaded via Resource.load()
                        tracing::warn!(
                            "Image widget: direct path loading not yet supported. Use Resource.load() first."
                        );
                        None
                    }
                    None => None,
                }
            });

            // Get the scale mode for later use
            let scale_mode = image_config
                .map(|cfg| cfg.scale_mode.clone())
                .unwrap_or_default();

            // Check if this is a Cover or Contain mode
            let is_cover_contain = matches!(scale_mode, ImageScaleMode::Cover | ImageScaleMode::Contain);

            // Convert ImageScaleMode to Bevy's NodeImageMode
            let image_mode = match &scale_mode {
                    ImageScaleMode::Auto => bevy::ui::widget::NodeImageMode::Auto,
                    ImageScaleMode::Stretch => bevy::ui::widget::NodeImageMode::Stretch,
                    ImageScaleMode::Tiled { tile_x, tile_y, stretch_value } => {
                        bevy::ui::widget::NodeImageMode::Tiled {
                            tile_x: *tile_x,
                            tile_y: *tile_y,
                            stretch_value: *stretch_value,
                        }
                    }
                    ImageScaleMode::Sliced { top, right, bottom, left, center } => {
                        bevy::ui::widget::NodeImageMode::Sliced(bevy::sprite::TextureSlicer {
                            border: bevy::sprite::BorderRect {
                                top: *top,
                                right: *right,
                                bottom: *bottom,
                                left: *left,
                            },
                            center_scale_mode: if *center {
                                bevy::sprite::SliceScaleMode::Stretch
                            } else {
                                bevy::sprite::SliceScaleMode::Tile { stretch_value: 0.0 }
                            },
                            sides_scale_mode: bevy::sprite::SliceScaleMode::Stretch,
                            max_corner_scale: 1.0,
                        })
                    }
                // Cover and Contain use Stretch mode to fill the node.
                // The node dimensions are calculated by update_cover_contain_images system
                // to achieve the Cover/Contain effect while maintaining aspect ratio.
                // - Cover: node is sized to cover container (may overflow, uses overflow: clip)
                // - Contain: node is sized to fit within container (may letterbox)
                ImageScaleMode::Contain | ImageScaleMode::Cover => {
                    bevy::ui::widget::NodeImageMode::Stretch
                }
            };

            // Build the entity
            tracing::debug!("Image widget: handle present = {:?}, node.width={:?}, node.height={:?}",
                image_handle.is_some(), node.width, node.height);
            if let Some(handle) = image_handle {
                // We have an image - create ImageNode
                tracing::debug!("Creating ImageNode with image_mode={:?}, handle={:?}", image_mode, handle);
                let mut image_node = bevy::ui::widget::ImageNode::new(handle.clone());
                image_node.image_mode = image_mode;

                // Apply flip if specified
                if let Some(img_cfg) = image_config {
                    image_node.flip_x = img_cfg.flip_x;
                    image_node.flip_y = img_cfg.flip_y;

                    // Apply tint color if specified
                    if let Some(ref tint) = img_cfg.tint {
                        image_node.color = color_value_to_bevy(tint);
                    }
                }

                if is_cover_contain {
                    // For Cover/Contain modes, we create a two-node structure:
                    // 1. Outer container: has the requested dimensions, overflow: clip
                    // 2. Inner image: positioned absolutely, centered, sized for Cover/Contain
                    //
                    // This emulates CSS object-fit: cover/contain behavior

                    // Create outer container with requested dimensions and overflow clipping
                    let mut container_node = node.clone();
                    container_node.overflow = bevy::ui::Overflow::clip();

                    // Create inner image node with absolute positioning for centering
                    let mut inner_image_node = Node::default();
                    inner_image_node.position_type = bevy::ui::PositionType::Absolute;
                    // Position at center of container
                    inner_image_node.left = Val::Percent(50.0);
                    inner_image_node.top = Val::Percent(50.0);
                    // Start with 100% dimensions, will be adjusted by update system
                    inner_image_node.width = Val::Percent(100.0);
                    inner_image_node.height = Val::Percent(100.0);

                    // Spawn outer container
                    let container_entity = commands.spawn((
                        container_node,
                        StamWidget {
                            id: widget_id,
                            window_id,
                            widget_type,
                        },
                        WidgetEventSubscriptions::default(),
                        ChildOf(parent_entity),
                    )).id();

                    // Spawn inner image as child of container
                    let mut image_cmd = commands.spawn((
                        inner_image_node,
                        image_node,
                        CoverContainImage {
                            scale_mode: scale_mode.clone(),
                            image_handle: handle.clone(),
                            applied: false,
                        },
                        ChildOf(container_entity),
                    ));

                    // Apply border radius to container if specified
                    if let Some(radius) = config.border_radius {
                        commands.entity(container_entity).insert(bevy::ui::BorderRadius::all(Val::Px(radius)));
                    }

                    // Apply background color to container if specified
                    if let Some(ref bg_color) = config.background_color {
                        commands.entity(container_entity).insert(BackgroundColor(color_value_to_bevy(bg_color)));
                    }

                    container_entity
                } else {
                    // Normal image mode - single node with image
                    let mut entity_cmd = commands.spawn((
                        node,
                        image_node,
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

                    // Apply background color if specified (visible around image if it doesn't fill)
                    if let Some(ref bg_color) = config.background_color {
                        entity_cmd.insert(BackgroundColor(color_value_to_bevy(bg_color)));
                    }

                    entity_cmd.id()
                }
            } else {
                // No image - create placeholder with background color
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

/// System to check pending assets and send ResourceLoaded events when ready
///
/// This system runs every frame and checks if any pending assets have finished
/// loading in Bevy's AssetServer. When an asset is ready (is_loaded_with_dependencies),
/// we send a ResourceLoaded event to notify the ResourceProxy.
fn check_pending_assets(
    mut pending_assets: ResMut<PendingAssetRegistry>,
    asset_server: Res<AssetServer>,
    event_tx: Res<EventSenderRes>,
) {
    // Skip if no pending assets
    if pending_assets.pending.is_empty() {
        return;
    }

    tracing::trace!("check_pending_assets: {} assets pending", pending_assets.pending.len());

    // Take all pending assets and check each one
    let mut still_pending = Vec::new();

    for entry in std::mem::take(&mut pending_assets.pending) {
        // Check if the asset is loaded
        let load_state = asset_server.get_load_state(entry.handle_id);
        tracing::trace!(
            "Asset '{}' (id={:?}) load_state={:?}",
            entry.alias, entry.handle_id, load_state
        );

        match load_state {
            Some(bevy::asset::LoadState::Loaded) => {
                // Asset is fully loaded, send event
                tracing::debug!(
                    "Resource '{}' (asset_id={}) finished loading in Bevy",
                    entry.alias, entry.asset_id
                );
                let _ = event_tx.0.try_send(GraphicEvent::ResourceLoaded {
                    alias: entry.alias,
                    asset_id: entry.asset_id,
                });
            }
            Some(bevy::asset::LoadState::Failed(err)) => {
                // Asset failed to load
                let error_msg = format!("Failed to load asset: {:?}", err);
                tracing::error!(
                    "Resource '{}' (asset_id={}) failed to load: {}",
                    entry.alias, entry.asset_id, error_msg
                );
                let _ = event_tx.0.try_send(GraphicEvent::ResourceFailed {
                    alias: entry.alias,
                    asset_id: entry.asset_id,
                    error: error_msg,
                });
            }
            Some(bevy::asset::LoadState::Loading) | Some(bevy::asset::LoadState::NotLoaded) | None => {
                // Still loading, keep tracking
                still_pending.push(entry);
            }
        }
    }

    // Put back the ones still pending
    pending_assets.pending = still_pending;
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

/// System to update Cover/Contain images based on actual image dimensions
///
/// This system runs each frame and checks for images with CoverContainImage component
/// that haven't been sized yet. Once the image asset is loaded, it calculates the
/// appropriate dimensions to achieve the Cover or Contain effect.
///
/// For Cover: Image fills container while maintaining aspect ratio (may be cropped)
/// For Contain: Image fits inside container while maintaining aspect ratio (may letterbox)
fn update_cover_contain_images(
    mut query: Query<(Entity, &mut Node, &mut CoverContainImage, &ChildOf)>,
    parent_query: Query<&ComputedNode>,
    images: Res<Assets<Image>>,
) {
    for (entity, mut node, mut cover_contain, child_of) in query.iter_mut() {
        // Skip if already applied
        if cover_contain.applied {
            continue;
        }

        // Get the image to find its dimensions
        let Some(image) = images.get(&cover_contain.image_handle) else {
            // Image not loaded yet, try again next frame
            continue;
        };

        // Get image dimensions
        let image_width = image.width() as f32;
        let image_height = image.height() as f32;
        let image_ratio = image_width / image_height;

        // Get parent container dimensions (this is the wrapper container we created)
        let parent_size = parent_query
            .get(child_of.parent())
            .map(|cn| cn.size())
            .unwrap_or(Vec2::ZERO);

        let container_width = parent_size.x;
        let container_height = parent_size.y;

        // Skip if container has zero dimensions (not yet laid out)
        if container_width <= 0.0 || container_height <= 0.0 {
            continue;
        }

        let container_ratio = container_width / container_height;

        tracing::debug!(
            "Cover/Contain sizing for entity {:?}: image={}x{} (ratio={:.3}), container={}x{} (ratio={:.3}), mode={:?}",
            entity, image_width, image_height, image_ratio,
            container_width, container_height, container_ratio,
            cover_contain.scale_mode
        );

        match cover_contain.scale_mode {
            ImageScaleMode::Cover => {
                // For Cover: the node must fill the container completely while maintaining aspect ratio.
                // The image uses Stretch mode, so the node dimensions determine the final appearance.
                // We calculate node dimensions such that:
                // - At least one dimension fills the container exactly
                // - The other dimension overflows (and is clipped by the parent)
                // - The aspect ratio of the node matches the image aspect ratio
                // - The node is centered using negative margins

                let (node_width, node_height) = if image_ratio > container_ratio {
                    // Image is relatively wider than container
                    // Height must fill container, width calculated from aspect ratio
                    let height = container_height;
                    let width = height * image_ratio;
                    (width, height)
                } else {
                    // Image is relatively taller than container
                    // Width must fill container, height calculated from aspect ratio
                    let width = container_width;
                    let height = width / image_ratio;
                    (width, height)
                };

                node.width = Val::Px(node_width);
                node.height = Val::Px(node_height);

                // Center the image by using negative margins (half of width/height)
                // This works with position: absolute, left: 50%, top: 50%
                node.margin.left = Val::Px(-node_width / 2.0);
                node.margin.top = Val::Px(-node_height / 2.0);

                tracing::debug!(
                    "Cover applied: width={:.1}px, height={:.1}px, margin=({:.1}, {:.1}) (container: {:.1}x{:.1})",
                    node_width, node_height, -node_width / 2.0, -node_height / 2.0, container_width, container_height
                );
            }
            ImageScaleMode::Contain => {
                // For Contain: the node must fit entirely within the container while maintaining aspect ratio.
                // We calculate node dimensions such that:
                // - At least one dimension fills the container exactly
                // - The other dimension is smaller (letterboxing)
                // - The aspect ratio of the node matches the image aspect ratio
                // - The node is centered using negative margins

                let (node_width, node_height) = if image_ratio > container_ratio {
                    // Image is relatively wider than container
                    // Width must fit container, height calculated from aspect ratio
                    let width = container_width;
                    let height = width / image_ratio;
                    (width, height)
                } else {
                    // Image is relatively taller than container
                    // Height must fit container, width calculated from aspect ratio
                    let height = container_height;
                    let width = height * image_ratio;
                    (width, height)
                };

                node.width = Val::Px(node_width);
                node.height = Val::Px(node_height);

                // Center the image by using negative margins (half of width/height)
                node.margin.left = Val::Px(-node_width / 2.0);
                node.margin.top = Val::Px(-node_height / 2.0);

                tracing::debug!(
                    "Contain applied: width={:.1}px, height={:.1}px, margin=({:.1}, {:.1}) (container: {:.1}x{:.1})",
                    node_width, node_height, -node_width / 2.0, -node_height / 2.0, container_width, container_height
                );
            }
            _ => {
                // Other modes shouldn't have this component, but just mark as applied
            }
        }

        cover_contain.applied = true;
    }
}
