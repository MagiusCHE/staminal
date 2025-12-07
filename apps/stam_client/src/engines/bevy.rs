//! Bevy Engine Implementation
//!
//! This module implements the `GraphicEngine` trait for the Bevy game engine.
//! Bevy runs on the main thread and communicates with the worker thread via channels.

use bevy::prelude::*;
use bevy::window::{PrimaryWindow, VideoModeSelection, WindowMode, WindowResolution, WindowRef, CursorIcon, SystemCursorIcon};
use bevy::winit::{UpdateMode, WinitSettings, WINIT_WINDOWS};
use bevy::camera::RenderTarget;
use std::collections::HashMap;
use std::sync::mpsc::Receiver;
use std::sync::Mutex;
use tokio::sync::mpsc::Sender;

use stam_mod_runtimes::api::{
    ColorValue, EdgeInsets, FlexDirection, GraphicCommand, GraphicEngine, GraphicEngineInfo,
    GraphicEngines, GraphicEvent, InitialWindowConfig, JustifyContent, KeyModifiers, MouseButton,
    SizeValue, WindowPositionMode, AlignItems, WindowMode as StamWindowMode,
    ResourceType, ResourceState, ResourceInfo, ImageScaleMode, ImageSource,
    graphic::ecs::{ComponentSchema, DeclaredSystem, QueryOptions, QueryResult, FieldType, SystemBehavior},
};

/// System sets for ordering Bevy systems
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
enum BevySystemSet {
    /// Process commands from the worker thread (first)
    ProcessCommands,
    /// Run declared systems (behaviors and formulas)
    DeclaredSystems,
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
        app.insert_resource(WindowUIRegistry::default());
        app.insert_resource(FontRegistry::default());
        app.insert_resource(ResourceRegistry::default());
        app.insert_resource(PendingAssetRegistry::default());
        app.insert_resource(EngineReadySent::default());
        // ECS scripting resources
        app.insert_resource(ScriptEntityRegistry::default());
        app.insert_resource(ScriptComponentRegistry::default());
        app.insert_resource(DeclaredSystemRegistry::default());
        app.insert_resource(EntityEventCallbackRegistry::default());

        // Force continuous updates even without windows or when unfocused
        // This ensures the Update schedule runs continuously to process commands
        app.insert_resource(WinitSettings {
            focused_mode: UpdateMode::Continuous,
            unfocused_mode: UpdateMode::Continuous,
        });

        // Add startup system to register the primary window in our registry
        app.add_systems(Startup, register_primary_window);

        // Configure system set ordering:
        // ProcessCommands -> DeclaredSystems -> AfterCommands
        app.configure_sets(
            Update,
            (
                BevySystemSet::ProcessCommands,
                BevySystemSet::DeclaredSystems.after(BevySystemSet::ProcessCommands),
                BevySystemSet::AfterCommands.after(BevySystemSet::DeclaredSystems),
            ),
        );

        // Add our systems with explicit ordering via SystemSets
        // send_engine_ready_event and check_pending_assets run AFTER process_commands
        // to ensure the command channel is being processed before JS handlers
        // try to call createWindow() in response to EngineReady
        //
        // Note: process_commands has too many parameters to use .in_set() directly.
        // We use a separate add_systems call and configure the set to run first.
        app.add_systems(Update, process_commands);
        app.add_systems(Update, run_declared_systems.in_set(BevySystemSet::DeclaredSystems));
        app.add_systems(
            Update,
            (
                send_engine_ready_event,
                check_pending_assets,
                send_frame_events,
                handle_keyboard_input,
                handle_mouse_input,
                handle_window_events,
                handle_script_entity_interactions,
                apply_script_button_colors,
                apply_disabled_button_colors,
                apply_enabled_button_colors,
                update_cover_contain_images,
            ).in_set(BevySystemSet::AfterCommands),
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

// ============================================================================
// Widget System Resources and Components
// ============================================================================

/// Registry for window UI infrastructure (cameras and root nodes)
///
/// Note: The legacy widget system has been removed. Use the ECS API instead.
/// This registry now only manages cameras and root UI nodes for each window.
#[derive(Resource, Default)]
struct WindowUIRegistry {
    /// Root UI nodes for each window (window_id -> root node entity)
    window_roots: HashMap<u64, Entity>,
    /// Camera entities for each window (window_id -> camera entity)
    window_cameras: HashMap<u64, Entity>,
}

impl WindowUIRegistry {
    fn set_window_root(&mut self, window_id: u64, root_entity: Entity) {
        self.window_roots.insert(window_id, root_entity);
    }

    fn get_window_root(&self, window_id: u64) -> Option<Entity> {
        self.window_roots.get(&window_id).copied()
    }

    fn set_window_camera(&mut self, window_id: u64, camera_entity: Entity) {
        self.window_cameras.insert(window_id, camera_entity);
    }

    fn get_window_camera(&self, window_id: u64) -> Option<Entity> {
        self.window_cameras.get(&window_id).copied()
    }

    fn remove_window_camera(&mut self, window_id: u64) -> Option<Entity> {
        self.window_cameras.remove(&window_id)
    }

    fn remove_window_root(&mut self, window_id: u64) -> Option<Entity> {
        self.window_roots.remove(&window_id)
    }
}

// Note: StamWidget and WidgetEventSubscriptions removed - use ECS API with ScriptEntity instead

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
    /// Last known container size - used to detect when recalculation is needed
    last_container_size: Vec2,
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

/// Button colors component for ECS script entities
///
/// Optional component that defines background colors for different button states.
/// When present on an entity with Button+Interaction, Bevy will automatically
/// apply the appropriate color based on the interaction state.
#[derive(Component, Clone, Debug)]
struct ScriptButtonColors {
    /// Background color in normal state
    normal: Color,
    /// Background color when hovered
    hovered: Option<Color>,
    /// Background color when pressed
    pressed: Option<Color>,
    /// Background color when disabled
    disabled: Option<Color>,
}

impl ScriptButtonColors {
    /// Get the color for the current interaction state
    fn color_for_interaction(&self, interaction: Interaction, is_disabled: bool) -> Color {
        if is_disabled {
            return self.disabled.unwrap_or(self.normal);
        }
        match interaction {
            Interaction::None => self.normal,
            Interaction::Hovered => self.hovered.unwrap_or(self.normal),
            Interaction::Pressed => self.pressed.unwrap_or(self.hovered.unwrap_or(self.normal)),
        }
    }
}

/// Marker component for disabled script entities (buttons)
#[derive(Component)]
struct ScriptButtonDisabled;

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
// Entity Event Callback Registry
// ============================================================================

/// Registry of entities that have registered event callbacks
///
/// When an entity in this registry receives an interaction matching
/// a registered event type, the engine sends EntityEventCallback
/// instead of the generic EntityInteractionChanged.
#[derive(Resource, Default)]
struct EntityEventCallbackRegistry {
    /// Map of entity ID -> set of registered event types (e.g., "click", "hover")
    entities: std::collections::HashMap<u64, std::collections::HashSet<String>>,
}

impl EntityEventCallbackRegistry {
    /// Register an entity for a specific event type
    fn register(&mut self, entity_id: u64, event_type: &str) {
        self.entities
            .entry(entity_id)
            .or_default()
            .insert(event_type.to_string());
    }

    /// Unregister an entity for a specific event type
    fn unregister(&mut self, entity_id: u64, event_type: &str) {
        if let Some(events) = self.entities.get_mut(&entity_id) {
            events.remove(event_type);
            if events.is_empty() {
                self.entities.remove(&entity_id);
            }
        }
    }

    /// Remove all callbacks for an entity (used on despawn)
    fn remove_entity(&mut self, entity_id: u64) {
        self.entities.remove(&entity_id);
    }

    /// Check if entity has a registered callback for an event type
    fn has_callback(&self, entity_id: u64, event_type: &str) -> bool {
        self.entities
            .get(&entity_id)
            .map_or(false, |events| events.contains(event_type))
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

// ============================================================================
// ECS Scripting Support
// ============================================================================

use std::sync::atomic::{AtomicU64, Ordering};

/// Marker component for entities created by scripts
///
/// This component identifies entities that were spawned via the ECS scripting API.
/// It stores the script-facing ID and the owning mod for security purposes.
#[derive(Component)]
struct ScriptEntity {
    /// Unique ID exposed to scripts (NOT the Bevy Entity index)
    script_id: u64,
    /// The mod that created this entity
    owner_mod: String,
}

/// Component holding custom data defined by scripts
///
/// Script-defined components are stored as JSON since Rust cannot dynamically
/// create struct types at runtime. Each ScriptComponent stores data for ONE
/// custom component type.
#[derive(Component, Clone, Debug)]
struct ScriptComponent {
    /// Component type name (e.g., "Player", "Velocity")
    type_name: String,
    /// Component data as JSON
    data: serde_json::Value,
}

/// Registry mapping script IDs to Bevy Entities
///
/// This registry provides bidirectional mapping between the IDs exposed to
/// scripts and the actual Bevy Entity handles.
#[derive(Resource)]
struct ScriptEntityRegistry {
    /// Map from script ID to Bevy Entity
    id_to_entity: HashMap<u64, Entity>,
    /// Map from Bevy Entity to script ID
    entity_to_id: HashMap<Entity, u64>,
    /// Next available script ID
    next_id: AtomicU64,
}

impl Default for ScriptEntityRegistry {
    fn default() -> Self {
        Self {
            id_to_entity: HashMap::new(),
            entity_to_id: HashMap::new(),
            next_id: AtomicU64::new(1), // Start from 1, 0 is reserved/invalid
        }
    }
}

impl ScriptEntityRegistry {
    /// Allocate a new script ID
    fn allocate_id(&self) -> u64 {
        self.next_id.fetch_add(1, Ordering::SeqCst)
    }

    /// Register an entity with a script ID
    fn register(&mut self, script_id: u64, entity: Entity) {
        self.id_to_entity.insert(script_id, entity);
        self.entity_to_id.insert(entity, script_id);
    }

    /// Unregister an entity by script ID
    fn unregister(&mut self, script_id: u64) -> Option<Entity> {
        if let Some(entity) = self.id_to_entity.remove(&script_id) {
            self.entity_to_id.remove(&entity);
            Some(entity)
        } else {
            None
        }
    }

    /// Get Bevy Entity by script ID
    fn get_entity(&self, script_id: u64) -> Option<Entity> {
        self.id_to_entity.get(&script_id).copied()
    }

    /// Get script ID by Bevy Entity
    fn get_id(&self, entity: Entity) -> Option<u64> {
        self.entity_to_id.get(&entity).copied()
    }
}

/// Registry for custom component types defined by scripts
///
/// Scripts must register component types with schemas before using them.
/// The registry validates component data against schemas.
#[derive(Resource, Default)]
struct ScriptComponentRegistry {
    /// Registered component schemas by name
    schemas: HashMap<String, ComponentSchema>,
}

impl ScriptComponentRegistry {
    /// Register a component schema
    fn register(&mut self, schema: ComponentSchema) -> Result<(), String> {
        if self.schemas.contains_key(&schema.name) {
            return Err(format!("Component '{}' is already registered", schema.name));
        }
        tracing::debug!("Registered component type: {}", schema.name);
        self.schemas.insert(schema.name.clone(), schema);
        Ok(())
    }

    /// Get a schema by name
    fn get_schema(&self, name: &str) -> Option<&ComponentSchema> {
        self.schemas.get(name)
    }

    /// Validate component data against its schema
    fn validate(&self, name: &str, data: &serde_json::Value) -> Result<(), String> {
        if let Some(schema) = self.schemas.get(name) {
            schema.validate(data)
        } else {
            // Allow unregistered components (for flexibility)
            // They won't be validated but can still be used
            Ok(())
        }
    }
}

/// Registry for declared systems (behaviors and formulas)
#[derive(Resource, Default)]
struct DeclaredSystemRegistry {
    /// Declared systems by name
    systems: HashMap<String, DeclaredSystem>,
}

impl DeclaredSystemRegistry {
    /// Register a declared system
    fn register(&mut self, system: DeclaredSystem) -> Result<(), String> {
        if self.systems.contains_key(&system.name) {
            return Err(format!("System '{}' is already declared", system.name));
        }
        tracing::debug!("Declared system: {} (behavior: {:?})", system.name, system.behavior);
        self.systems.insert(system.name.clone(), system);
        Ok(())
    }

    /// Get a system by name
    fn get(&self, name: &str) -> Option<&DeclaredSystem> {
        self.systems.get(name)
    }

    /// Get mutable reference to a system
    fn get_mut(&mut self, name: &str) -> Option<&mut DeclaredSystem> {
        self.systems.get_mut(name)
    }

    /// Remove a system
    fn remove(&mut self, name: &str) -> Option<DeclaredSystem> {
        self.systems.remove(name)
    }

    /// Iterate over enabled systems in order
    fn iter_enabled(&self) -> impl Iterator<Item = &DeclaredSystem> {
        let mut systems: Vec<_> = self.systems.values().filter(|s| s.enabled).collect();
        systems.sort_by_key(|s| s.order);
        systems.into_iter()
    }
}

// ============================================================================
// Native Component Reflection System
// ============================================================================

/// Whitelist of native Bevy components that scripts can access
///
/// This enum defines which Bevy components are safe for scripts to read/write.
/// Each variant maps to a specific Bevy component type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum NativeComponent {
    // === Core Components ===
    /// Transform component (translation, rotation, scale)
    Transform,
    /// Sprite component for 2D rendering
    Sprite,
    /// Visibility component
    Visibility,

    // === UI Components ===
    /// Node component for UI layout (flexbox)
    Node,
    /// BackgroundColor component for UI elements
    BackgroundColor,
    /// Text component for UI text rendering
    Text,
    /// BorderRadius component for rounded corners
    BorderRadius,
    /// Interaction component for UI input handling (click, hover)
    Interaction,
    /// Button marker component for clickable UI elements
    Button,
    /// ImageNode component for UI image rendering
    ImageNode,
}

impl NativeComponent {
    /// Get the component name as used in JavaScript
    fn name(&self) -> &'static str {
        match self {
            NativeComponent::Transform => "Transform",
            NativeComponent::Sprite => "Sprite",
            NativeComponent::Visibility => "Visibility",
            NativeComponent::Node => "Node",
            NativeComponent::BackgroundColor => "BackgroundColor",
            NativeComponent::Text => "Text",
            NativeComponent::BorderRadius => "BorderRadius",
            NativeComponent::Interaction => "Interaction",
            NativeComponent::Button => "Button",
            NativeComponent::ImageNode => "ImageNode",
        }
    }

    /// Try to parse a component name into a native component
    fn from_name(name: &str) -> Option<Self> {
        match name {
            "Transform" => Some(NativeComponent::Transform),
            "Sprite" => Some(NativeComponent::Sprite),
            "Visibility" => Some(NativeComponent::Visibility),
            "Node" => Some(NativeComponent::Node),
            "BackgroundColor" => Some(NativeComponent::BackgroundColor),
            "Text" => Some(NativeComponent::Text),
            "BorderRadius" => Some(NativeComponent::BorderRadius),
            "Interaction" => Some(NativeComponent::Interaction),
            "Button" => Some(NativeComponent::Button),
            "ImageNode" => Some(NativeComponent::ImageNode),
            _ => None,
        }
    }

    /// Check if a component name is a native component
    fn is_native(name: &str) -> bool {
        Self::from_name(name).is_some()
    }
}

/// Helper functions for converting between JSON and Bevy components
mod native_component_converters {
    use super::*;
    use serde_json::{json, Value};

    /// Merge two JSON objects, with `updates` overwriting `base`
    ///
    /// Only top-level keys from `updates` are merged into `base`.
    /// Nested objects are replaced entirely, not recursively merged.
    pub fn merge_json(base: &Value, updates: &Value) -> Value {
        if let (Some(base_obj), Some(updates_obj)) = (base.as_object(), updates.as_object()) {
            let mut result = base_obj.clone();
            for (key, value) in updates_obj {
                result.insert(key.clone(), value.clone());
            }
            Value::Object(result)
        } else {
            // If not both objects, just return the update value
            updates.clone()
        }
    }

    /// Convert a Transform component to JSON
    pub fn transform_to_json(transform: &Transform) -> Value {
        json!({
            "translation": {
                "x": transform.translation.x,
                "y": transform.translation.y,
                "z": transform.translation.z
            },
            "rotation": {
                "x": transform.rotation.x,
                "y": transform.rotation.y,
                "z": transform.rotation.z,
                "w": transform.rotation.w
            },
            "scale": {
                "x": transform.scale.x,
                "y": transform.scale.y,
                "z": transform.scale.z
            }
        })
    }

    /// Convert JSON to Transform values (for updating)
    pub fn json_to_transform(json: &Value, current: &mut Transform) -> Result<(), String> {
        if let Some(obj) = json.as_object() {
            // Update translation if provided
            if let Some(translation) = obj.get("translation") {
                if let Some(t) = translation.as_object() {
                    if let Some(x) = t.get("x").and_then(|v| v.as_f64()) {
                        current.translation.x = x as f32;
                    }
                    if let Some(y) = t.get("y").and_then(|v| v.as_f64()) {
                        current.translation.y = y as f32;
                    }
                    if let Some(z) = t.get("z").and_then(|v| v.as_f64()) {
                        current.translation.z = z as f32;
                    }
                }
            }

            // Update rotation if provided
            if let Some(rotation) = obj.get("rotation") {
                if let Some(r) = rotation.as_object() {
                    let x = r.get("x").and_then(|v| v.as_f64()).unwrap_or(current.rotation.x as f64) as f32;
                    let y = r.get("y").and_then(|v| v.as_f64()).unwrap_or(current.rotation.y as f64) as f32;
                    let z = r.get("z").and_then(|v| v.as_f64()).unwrap_or(current.rotation.z as f64) as f32;
                    let w = r.get("w").and_then(|v| v.as_f64()).unwrap_or(current.rotation.w as f64) as f32;
                    current.rotation = Quat::from_xyzw(x, y, z, w);
                }
            }

            // Update scale if provided
            if let Some(scale) = obj.get("scale") {
                if let Some(s) = scale.as_object() {
                    if let Some(x) = s.get("x").and_then(|v| v.as_f64()) {
                        current.scale.x = x as f32;
                    }
                    if let Some(y) = s.get("y").and_then(|v| v.as_f64()) {
                        current.scale.y = y as f32;
                    }
                    if let Some(z) = s.get("z").and_then(|v| v.as_f64()) {
                        current.scale.z = z as f32;
                    }
                }
            }

            Ok(())
        } else {
            Err("Transform data must be an object".to_string())
        }
    }

    /// Create a new Transform from JSON
    pub fn json_to_new_transform(json: &Value) -> Result<Transform, String> {
        let mut transform = Transform::IDENTITY;
        json_to_transform(json, &mut transform)?;
        Ok(transform)
    }

    /// Convert a Sprite component to JSON
    ///
    /// Note: We only expose safe fields. Handle<Image> is converted to path if available.
    pub fn sprite_to_json(sprite: &Sprite) -> Value {
        let color = sprite.color;
        let (r, g, b, a) = color.to_srgba().to_f32_array().into();

        json!({
            "color": {
                "r": r,
                "g": g,
                "b": b,
                "a": a
            },
            "flip_x": sprite.flip_x,
            "flip_y": sprite.flip_y,
            "custom_size": sprite.custom_size.map(|v| json!({"width": v.x, "height": v.y})),
            "rect": sprite.rect.map(|r| json!({
                "min": {"x": r.min.x, "y": r.min.y},
                "max": {"x": r.max.x, "y": r.max.y}
            }))
        })
    }

    /// Update a Sprite from JSON
    pub fn json_to_sprite(json: &Value, current: &mut Sprite) -> Result<(), String> {
        if let Some(obj) = json.as_object() {
            // Update color if provided
            if let Some(color) = obj.get("color") {
                if let Some(c) = color.as_object() {
                    let r = c.get("r").and_then(|v| v.as_f64()).unwrap_or(1.0) as f32;
                    let g = c.get("g").and_then(|v| v.as_f64()).unwrap_or(1.0) as f32;
                    let b = c.get("b").and_then(|v| v.as_f64()).unwrap_or(1.0) as f32;
                    let a = c.get("a").and_then(|v| v.as_f64()).unwrap_or(1.0) as f32;
                    current.color = Color::srgba(r, g, b, a);
                }
            }

            // Update flip flags
            if let Some(flip_x) = obj.get("flip_x").and_then(|v| v.as_bool()) {
                current.flip_x = flip_x;
            }
            if let Some(flip_y) = obj.get("flip_y").and_then(|v| v.as_bool()) {
                current.flip_y = flip_y;
            }

            // Update custom_size
            if let Some(custom_size) = obj.get("custom_size") {
                if custom_size.is_null() {
                    current.custom_size = None;
                } else if let Some(size) = custom_size.as_object() {
                    let width = size.get("width").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32;
                    let height = size.get("height").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32;
                    current.custom_size = Some(Vec2::new(width, height));
                }
            }

            // Update rect
            if let Some(rect) = obj.get("rect") {
                if rect.is_null() {
                    current.rect = None;
                } else if let Some(r) = rect.as_object() {
                    if let (Some(min), Some(max)) = (r.get("min"), r.get("max")) {
                        let min_x = min.get("x").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32;
                        let min_y = min.get("y").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32;
                        let max_x = max.get("x").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32;
                        let max_y = max.get("y").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32;
                        current.rect = Some(Rect::new(min_x, min_y, max_x, max_y));
                    }
                }
            }

            Ok(())
        } else {
            Err("Sprite data must be an object".to_string())
        }
    }

    /// Create a new default Sprite from JSON
    pub fn json_to_new_sprite(json: &Value) -> Result<Sprite, String> {
        let mut sprite = Sprite::default();
        json_to_sprite(json, &mut sprite)?;
        Ok(sprite)
    }

    /// Convert Visibility to JSON
    pub fn visibility_to_json(visibility: &Visibility) -> Value {
        let visible = match visibility {
            Visibility::Inherited => "inherited",
            Visibility::Hidden => "hidden",
            Visibility::Visible => "visible",
        };
        json!({ "value": visible })
    }

    /// Update Visibility from JSON
    pub fn json_to_visibility(json: &Value) -> Result<Visibility, String> {
        if let Some(obj) = json.as_object() {
            if let Some(value) = obj.get("value").and_then(|v| v.as_str()) {
                match value {
                    "inherited" => Ok(Visibility::Inherited),
                    "hidden" => Ok(Visibility::Hidden),
                    "visible" => Ok(Visibility::Visible),
                    _ => Err(format!("Invalid visibility value: {}. Expected 'inherited', 'hidden', or 'visible'", value)),
                }
            } else {
                Err("Visibility must have a 'value' field".to_string())
            }
        } else if let Some(s) = json.as_str() {
            // Also accept direct string value
            match s {
                "inherited" => Ok(Visibility::Inherited),
                "hidden" => Ok(Visibility::Hidden),
                "visible" => Ok(Visibility::Visible),
                _ => Err(format!("Invalid visibility value: {}. Expected 'inherited', 'hidden', or 'visible'", s)),
            }
        } else {
            Err("Visibility data must be an object with 'value' field or a string".to_string())
        }
    }

    // ========================================================================
    // UI Component Converters
    // ========================================================================

    /// Helper to parse a Val from JSON
    fn parse_val(json: &Value) -> Val {
        if let Some(s) = json.as_str() {
            if s == "auto" {
                return Val::Auto;
            }
            if let Some(pct) = s.strip_suffix('%') {
                if let Ok(v) = pct.parse::<f32>() {
                    return Val::Percent(v);
                }
            }
            if let Some(px) = s.strip_suffix("px") {
                if let Ok(v) = px.parse::<f32>() {
                    return Val::Px(v);
                }
            }
            // Try parsing as plain number (px)
            if let Ok(v) = s.parse::<f32>() {
                return Val::Px(v);
            }
        }
        if let Some(n) = json.as_f64() {
            return Val::Px(n as f32);
        }
        Val::Auto
    }

    /// Convert a Node component to JSON
    pub fn node_to_json(node: &Node) -> Value {
        json!({
            "width": val_to_json(&node.width),
            "height": val_to_json(&node.height),
            "min_width": val_to_json(&node.min_width),
            "min_height": val_to_json(&node.min_height),
            "max_width": val_to_json(&node.max_width),
            "max_height": val_to_json(&node.max_height),
            "left": val_to_json(&node.left),
            "right": val_to_json(&node.right),
            "top": val_to_json(&node.top),
            "bottom": val_to_json(&node.bottom),
            "display": format!("{:?}", node.display).to_lowercase(),
            "position_type": format!("{:?}", node.position_type).to_lowercase(),
            "flex_direction": format!("{:?}", node.flex_direction).to_lowercase(),
            "justify_content": format!("{:?}", node.justify_content).to_lowercase(),
            "align_items": format!("{:?}", node.align_items).to_lowercase()
        })
    }

    fn val_to_json(val: &Val) -> Value {
        match val {
            Val::Auto => json!("auto"),
            Val::Px(v) => json!(v),
            Val::Percent(v) => json!(format!("{}%", v)),
            Val::Vw(v) => json!(format!("{}vw", v)),
            Val::Vh(v) => json!(format!("{}vh", v)),
            Val::VMin(v) => json!(format!("{}vmin", v)),
            Val::VMax(v) => json!(format!("{}vmax", v)),
        }
    }

    /// Update a Node from JSON
    pub fn json_to_node(json: &Value, current: &mut Node) -> Result<(), String> {
        if let Some(obj) = json.as_object() {
            if let Some(width) = obj.get("width") {
                current.width = parse_val(width);
            }
            if let Some(height) = obj.get("height") {
                current.height = parse_val(height);
            }
            if let Some(min_width) = obj.get("min_width") {
                current.min_width = parse_val(min_width);
            }
            if let Some(min_height) = obj.get("min_height") {
                current.min_height = parse_val(min_height);
            }
            if let Some(max_width) = obj.get("max_width") {
                current.max_width = parse_val(max_width);
            }
            if let Some(max_height) = obj.get("max_height") {
                current.max_height = parse_val(max_height);
            }
            if let Some(left) = obj.get("left") {
                current.left = parse_val(left);
            }
            if let Some(right) = obj.get("right") {
                current.right = parse_val(right);
            }
            if let Some(top) = obj.get("top") {
                current.top = parse_val(top);
            }
            if let Some(bottom) = obj.get("bottom") {
                current.bottom = parse_val(bottom);
            }
            if let Some(display) = obj.get("display").and_then(|v| v.as_str()) {
                current.display = match display {
                    "flex" => Display::Flex,
                    "grid" => Display::Grid,
                    "block" => Display::Block,
                    "none" => Display::None,
                    _ => Display::Flex,
                };
            }
            if let Some(pos_val) = obj.get("position_type") {
                current.position_type = if let Some(pos) = pos_val.as_str() {
                    match pos {
                        "relative" => PositionType::Relative,
                        "absolute" => PositionType::Absolute,
                        _ => PositionType::Relative,
                    }
                } else if let Some(n) = pos_val.as_u64() {
                    // PositionType enum: Relative=0, Absolute=1
                    match n {
                        0 => PositionType::Relative,
                        1 => PositionType::Absolute,
                        _ => PositionType::Relative,
                    }
                } else {
                    PositionType::Relative
                };
            }
            if let Some(dir_val) = obj.get("flex_direction") {
                current.flex_direction = if let Some(dir) = dir_val.as_str() {
                    match dir {
                        "row" => bevy::ui::FlexDirection::Row,
                        "column" => bevy::ui::FlexDirection::Column,
                        "row_reverse" | "rowreverse" => bevy::ui::FlexDirection::RowReverse,
                        "column_reverse" | "columnreverse" => bevy::ui::FlexDirection::ColumnReverse,
                        _ => bevy::ui::FlexDirection::Row,
                    }
                } else if let Some(n) = dir_val.as_u64() {
                    // FlexDirection enum: Row=0, Column=1, RowReverse=2, ColumnReverse=3
                    match n {
                        0 => bevy::ui::FlexDirection::Row,
                        1 => bevy::ui::FlexDirection::Column,
                        2 => bevy::ui::FlexDirection::RowReverse,
                        3 => bevy::ui::FlexDirection::ColumnReverse,
                        _ => bevy::ui::FlexDirection::Row,
                    }
                } else {
                    bevy::ui::FlexDirection::Row
                };
            }
            if let Some(justify_val) = obj.get("justify_content") {
                current.justify_content = if let Some(justify) = justify_val.as_str() {
                    match justify {
                        "start" | "flex_start" | "flexstart" => bevy::ui::JustifyContent::Start,
                        "end" | "flex_end" | "flexend" => bevy::ui::JustifyContent::End,
                        "center" => bevy::ui::JustifyContent::Center,
                        "space_between" | "spacebetween" => bevy::ui::JustifyContent::SpaceBetween,
                        "space_around" | "spacearound" => bevy::ui::JustifyContent::SpaceAround,
                        "space_evenly" | "spaceevenly" => bevy::ui::JustifyContent::SpaceEvenly,
                        _ => bevy::ui::JustifyContent::Start,
                    }
                } else if let Some(n) = justify_val.as_u64() {
                    // JustifyContent enum: FlexStart=0, FlexEnd=1, Center=2, SpaceBetween=3, SpaceAround=4, SpaceEvenly=5
                    match n {
                        0 => bevy::ui::JustifyContent::FlexStart,
                        1 => bevy::ui::JustifyContent::FlexEnd,
                        2 => bevy::ui::JustifyContent::Center,
                        3 => bevy::ui::JustifyContent::SpaceBetween,
                        4 => bevy::ui::JustifyContent::SpaceAround,
                        5 => bevy::ui::JustifyContent::SpaceEvenly,
                        _ => bevy::ui::JustifyContent::Start,
                    }
                } else {
                    bevy::ui::JustifyContent::Start
                };
            }
            if let Some(align_val) = obj.get("align_items") {
                current.align_items = if let Some(align) = align_val.as_str() {
                    match align {
                        "start" | "flex_start" | "flexstart" => bevy::ui::AlignItems::Start,
                        "end" | "flex_end" | "flexend" => bevy::ui::AlignItems::End,
                        "center" => bevy::ui::AlignItems::Center,
                        "stretch" => bevy::ui::AlignItems::Stretch,
                        "baseline" => bevy::ui::AlignItems::Baseline,
                        _ => bevy::ui::AlignItems::Start,
                    }
                } else if let Some(n) = align_val.as_u64() {
                    // AlignItems enum: Stretch=0, FlexStart=1, FlexEnd=2, Center=3, Baseline=4
                    match n {
                        0 => bevy::ui::AlignItems::Stretch,
                        1 => bevy::ui::AlignItems::FlexStart,
                        2 => bevy::ui::AlignItems::FlexEnd,
                        3 => bevy::ui::AlignItems::Center,
                        4 => bevy::ui::AlignItems::Baseline,
                        _ => bevy::ui::AlignItems::Start,
                    }
                } else {
                    bevy::ui::AlignItems::Start
                };
            }
            // Padding support - can be number (all sides) or {top, right, bottom, left}
            if let Some(padding) = obj.get("padding") {
                current.padding = parse_ui_rect(padding);
            }
            // Margin support
            if let Some(margin) = obj.get("margin") {
                current.margin = parse_ui_rect(margin);
            }
            // Gap support
            if let Some(row_gap) = obj.get("row_gap") {
                current.row_gap = parse_val(row_gap);
            }
            if let Some(column_gap) = obj.get("column_gap") {
                current.column_gap = parse_val(column_gap);
            }
            // Combined gap (sets both row and column gap)
            if let Some(gap) = obj.get("gap") {
                let gap_val = parse_val(gap);
                current.row_gap = gap_val;
                current.column_gap = gap_val;
            }
            Ok(())
        } else {
            Err("Node data must be an object".to_string())
        }
    }

    /// Parse a UiRect from JSON (can be number or {top, right, bottom, left})
    fn parse_ui_rect(json: &Value) -> UiRect {
        if let Some(n) = json.as_f64() {
            UiRect::all(Val::Px(n as f32))
        } else if let Some(obj) = json.as_object() {
            UiRect {
                top: obj.get("top").map(parse_val).unwrap_or(Val::Px(0.0)),
                right: obj.get("right").map(parse_val).unwrap_or(Val::Px(0.0)),
                bottom: obj.get("bottom").map(parse_val).unwrap_or(Val::Px(0.0)),
                left: obj.get("left").map(parse_val).unwrap_or(Val::Px(0.0)),
            }
        } else {
            UiRect::all(Val::Px(0.0))
        }
    }

    /// Create a new Node from JSON
    pub fn json_to_new_node(json: &Value) -> Result<Node, String> {
        let mut node = Node::default();
        json_to_node(json, &mut node)?;
        Ok(node)
    }

    /// Convert BackgroundColor to JSON
    pub fn background_color_to_json(bg: &BackgroundColor) -> Value {
        let color = bg.0;
        let rgba = color.to_srgba().to_f32_array();
        json!({
            "r": rgba[0],
            "g": rgba[1],
            "b": rgba[2],
            "a": rgba[3]
        })
    }

    /// Parse a color from JSON (supports {r,g,b,a} or hex string)
    pub fn json_to_color(json: &Value) -> Result<Color, String> {
        if let Some(obj) = json.as_object() {
            let r = obj.get("r").and_then(|v| v.as_f64()).unwrap_or(1.0) as f32;
            let g = obj.get("g").and_then(|v| v.as_f64()).unwrap_or(1.0) as f32;
            let b = obj.get("b").and_then(|v| v.as_f64()).unwrap_or(1.0) as f32;
            let a = obj.get("a").and_then(|v| v.as_f64()).unwrap_or(1.0) as f32;
            Ok(Color::srgba(r, g, b, a))
        } else if let Some(hex) = json.as_str() {
            // Parse hex color
            let hex = hex.trim_start_matches('#');
            if hex.len() == 6 {
                let r = u8::from_str_radix(&hex[0..2], 16).map_err(|_| "Invalid hex color")?;
                let g = u8::from_str_radix(&hex[2..4], 16).map_err(|_| "Invalid hex color")?;
                let b = u8::from_str_radix(&hex[4..6], 16).map_err(|_| "Invalid hex color")?;
                Ok(Color::srgb_u8(r, g, b))
            } else if hex.len() == 8 {
                let r = u8::from_str_radix(&hex[0..2], 16).map_err(|_| "Invalid hex color")?;
                let g = u8::from_str_radix(&hex[2..4], 16).map_err(|_| "Invalid hex color")?;
                let b = u8::from_str_radix(&hex[4..6], 16).map_err(|_| "Invalid hex color")?;
                let a = u8::from_str_radix(&hex[6..8], 16).map_err(|_| "Invalid hex color")?;
                Ok(Color::srgba_u8(r, g, b, a))
            } else {
                Err(format!("Invalid hex color: {}", hex))
            }
        } else {
            Err("Color must be {r,g,b,a} object or hex string".to_string())
        }
    }

    /// Create BackgroundColor from JSON
    pub fn json_to_background_color(json: &Value) -> Result<BackgroundColor, String> {
        let color = json_to_color(json)?;
        Ok(BackgroundColor(color))
    }

    /// Convert Text component to JSON
    pub fn text_to_json(text: &bevy::prelude::Text) -> Value {
        json!({
            "content": text.0.clone()
        })
    }

    /// Text bundle result containing Text, TextFont, and TextColor components
    pub struct TextComponents {
        pub text: bevy::prelude::Text,
        pub font: bevy::text::TextFont,
        pub color: bevy::text::TextColor,
    }

    /// Parsed text configuration from JSON (without font handle resolution)
    pub struct TextConfig {
        pub content: String,
        pub font_alias: Option<String>,
        pub font_size: f32,
        pub color: bevy::color::Color,
    }

    /// Parse Text config from JSON without resolving font handle
    pub fn json_to_text_config(json: &Value) -> Result<TextConfig, String> {
        if let Some(obj) = json.as_object() {
            // Support both "value" and "content" as the text field
            let content = obj.get("value")
                .or_else(|| obj.get("content"))
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();

            let font_alias = obj.get("font")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());

            let font_size = obj.get("font_size")
                .and_then(|v| v.as_f64())
                .map(|v| v as f32)
                .unwrap_or(16.0);

            let color = if let Some(color_val) = obj.get("color") {
                json_to_color(color_val).unwrap_or(bevy::color::Color::WHITE)
            } else {
                bevy::color::Color::WHITE
            };

            Ok(TextConfig {
                content,
                font_alias,
                font_size,
                color,
            })
        } else if let Some(s) = json.as_str() {
            Ok(TextConfig {
                content: s.to_string(),
                font_alias: None,
                font_size: 16.0,
                color: bevy::color::Color::WHITE,
            })
        } else {
            Err("Text must be {value: string, font?: string, font_size?: number, color?: string} or a string".to_string())
        }
    }

    /// Create Text components from JSON (legacy, without font handle)
    /// Supports both {value, font_size, color} and {content} formats, plus plain string
    pub fn json_to_new_text_components(json: &Value) -> Result<TextComponents, String> {
        let config = json_to_text_config(json)?;
        Ok(TextComponents {
            text: bevy::prelude::Text::new(config.content),
            font: bevy::text::TextFont {
                font_size: config.font_size,
                ..default()
            },
            color: bevy::text::TextColor(config.color),
        })
    }

    /// Create Text from JSON (legacy, for compatibility)
    pub fn json_to_new_text(json: &Value) -> Result<bevy::prelude::Text, String> {
        if let Some(obj) = json.as_object() {
            let content = obj.get("value")
                .or_else(|| obj.get("content"))
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            Ok(bevy::prelude::Text::new(content))
        } else if let Some(s) = json.as_str() {
            Ok(bevy::prelude::Text::new(s.to_string()))
        } else {
            Err("Text must be {value: string} or a string".to_string())
        }
    }

    /// Update Text from JSON
    pub fn json_to_text(json: &Value, current: &mut bevy::prelude::Text) -> Result<(), String> {
        if let Some(obj) = json.as_object() {
            if let Some(content) = obj.get("value").or_else(|| obj.get("content")).and_then(|v| v.as_str()) {
                current.0 = content.to_string();
            }
            Ok(())
        } else if let Some(s) = json.as_str() {
            current.0 = s.to_string();
            Ok(())
        } else {
            Err("Text must be {value: string} or a string".to_string())
        }
    }

    /// Convert BorderRadius to JSON
    pub fn border_radius_to_json(radius: &BorderRadius) -> Value {
        json!({
            "top_left": val_to_json(&radius.top_left),
            "top_right": val_to_json(&radius.top_right),
            "bottom_left": val_to_json(&radius.bottom_left),
            "bottom_right": val_to_json(&radius.bottom_right)
        })
    }

    /// Create BorderRadius from JSON
    pub fn json_to_border_radius(json: &Value) -> Result<BorderRadius, String> {
        if let Some(n) = json.as_f64() {
            // Single value for all corners
            let v = Val::Px(n as f32);
            Ok(BorderRadius::all(v))
        } else if let Some(obj) = json.as_object() {
            let top_left = obj.get("top_left").map(parse_val).unwrap_or(Val::Px(0.0));
            let top_right = obj.get("top_right").map(parse_val).unwrap_or(Val::Px(0.0));
            let bottom_left = obj.get("bottom_left").map(parse_val).unwrap_or(Val::Px(0.0));
            let bottom_right = obj.get("bottom_right").map(parse_val).unwrap_or(Val::Px(0.0));
            Ok(BorderRadius {
                top_left,
                top_right,
                bottom_left,
                bottom_right,
            })
        } else {
            Err("BorderRadius must be a number or {top_left, top_right, bottom_left, bottom_right}".to_string())
        }
    }

    /// Convert Interaction component to JSON
    pub fn interaction_to_json(interaction: &Interaction) -> Value {
        match interaction {
            Interaction::None => json!("none"),
            Interaction::Hovered => json!("hovered"),
            Interaction::Pressed => json!("pressed"),
        }
    }

    /// Create Interaction from JSON
    /// Note: Interaction is typically managed by Bevy internally, but scripts can read it
    pub fn json_to_interaction(json: &Value) -> Result<Interaction, String> {
        if let Some(s) = json.as_str() {
            match s {
                "none" => Ok(Interaction::None),
                "hovered" => Ok(Interaction::Hovered),
                "pressed" => Ok(Interaction::Pressed),
                _ => Err(format!("Invalid Interaction value: {}. Use 'none', 'hovered', or 'pressed'", s)),
            }
        } else if json.is_object() || json.is_null() {
            // Default to None if object or null - allows spawning with Interaction: {}
            Ok(Interaction::None)
        } else {
            Err("Interaction must be a string ('none', 'hovered', 'pressed') or empty object".to_string())
        }
    }

    /// Button component doesn't need conversion - it's a marker component
    /// Scripts spawn it with Button: {} or Button: true
    pub fn json_to_button(json: &Value) -> Result<bevy::ui::widget::Button, String> {
        // Button is a marker component, any value means "add button"
        if json.is_null() {
            Err("Button component cannot be null. Use {} or true to add it.".to_string())
        } else {
            Ok(bevy::ui::widget::Button)
        }
    }

    /// ImageNode configuration from JSON
    ///
    /// Expected format:
    /// ```json
    /// {
    ///   "resource_id": "my-image-alias",  // Required: alias from Resource.load()
    ///   "image_mode": 0,                  // Optional: NodeImageMode enum value (0=Auto, 1=Stretch, 2=Sliced, 3=Tiled, 4=Contain, 5=Cover)
    ///   "flip_x": false,                  // Optional: horizontal flip
    ///   "flip_y": false,                  // Optional: vertical flip
    ///   "color": "#FFFFFF"                // Optional: tint color
    /// }
    /// ```
    pub struct ImageNodeConfig {
        pub resource_id: String,
        pub image_mode: bevy::ui::widget::NodeImageMode,
        /// Original scale mode requested (for Cover/Contain which use Stretch internally)
        pub scale_mode: Option<ImageScaleMode>,
        pub flip_x: bool,
        pub flip_y: bool,
        pub color: Option<Color>,
    }

    /// Parse NodeImageMode from JSON value
    /// Accepts: number (0-5), string ("auto", "stretch", "sliced", "tiled", "contain", "cover"), or object with mode config
    ///
    /// Returns: (NodeImageMode, Option<ImageScaleMode>)
    /// For Cover/Contain, returns Stretch as the base mode and the original ImageScaleMode
    pub fn parse_node_image_mode(json: &Value) -> Result<(bevy::ui::widget::NodeImageMode, Option<ImageScaleMode>), String> {
        // Handle number
        if let Some(n) = json.as_u64() {
            return match n {
                0 => Ok((bevy::ui::widget::NodeImageMode::Auto, None)),
                1 => Ok((bevy::ui::widget::NodeImageMode::Stretch, None)),
                2 => {
                    // Sliced with default values
                    Ok((bevy::ui::widget::NodeImageMode::Sliced(bevy::sprite::TextureSlicer::default()), None))
                }
                3 => {
                    // Tiled with default values
                    Ok((bevy::ui::widget::NodeImageMode::Tiled {
                        tile_x: true,
                        tile_y: true,
                        stretch_value: 1.0,
                    }, None))
                }
                4 => {
                    // Contain - uses Stretch internally, sizing handled by CoverContainImage system
                    Ok((bevy::ui::widget::NodeImageMode::Stretch, Some(ImageScaleMode::Contain)))
                }
                5 => {
                    // Cover - uses Stretch internally, sizing handled by CoverContainImage system
                    Ok((bevy::ui::widget::NodeImageMode::Stretch, Some(ImageScaleMode::Cover)))
                }
                _ => Err(format!("Invalid image_mode: {}. Use 0=Auto, 1=Stretch, 2=Sliced, 3=Tiled, 4=Contain, 5=Cover", n)),
            };
        }

        // Handle string
        if let Some(s) = json.as_str() {
            return match s.to_lowercase().as_str() {
                "auto" => Ok((bevy::ui::widget::NodeImageMode::Auto, None)),
                "stretch" => Ok((bevy::ui::widget::NodeImageMode::Stretch, None)),
                "sliced" => Ok((bevy::ui::widget::NodeImageMode::Sliced(bevy::sprite::TextureSlicer::default()), None)),
                "tiled" => Ok((bevy::ui::widget::NodeImageMode::Tiled {
                    tile_x: true,
                    tile_y: true,
                    stretch_value: 1.0,
                }, None)),
                "contain" => Ok((bevy::ui::widget::NodeImageMode::Stretch, Some(ImageScaleMode::Contain))),
                "cover" => Ok((bevy::ui::widget::NodeImageMode::Stretch, Some(ImageScaleMode::Cover))),
                _ => Err(format!("Invalid image_mode: '{}'. Use 'auto', 'stretch', 'sliced', 'tiled', 'contain', or 'cover'", s)),
            };
        }

        // Handle object for detailed configuration
        if let Some(obj) = json.as_object() {
            if let Some(mode_type) = obj.get("type").and_then(|v| v.as_str()) {
                return match mode_type.to_lowercase().as_str() {
                    "auto" => Ok((bevy::ui::widget::NodeImageMode::Auto, None)),
                    "stretch" => Ok((bevy::ui::widget::NodeImageMode::Stretch, None)),
                    "sliced" => {
                        // Parse TextureSlicer configuration
                        let border = obj.get("border").and_then(|b| b.as_object());
                        let top = border.and_then(|b| b.get("top")).and_then(|v| v.as_f64()).unwrap_or(0.0) as f32;
                        let right = border.and_then(|b| b.get("right")).and_then(|v| v.as_f64()).unwrap_or(0.0) as f32;
                        let bottom = border.and_then(|b| b.get("bottom")).and_then(|v| v.as_f64()).unwrap_or(0.0) as f32;
                        let left = border.and_then(|b| b.get("left")).and_then(|v| v.as_f64()).unwrap_or(0.0) as f32;

                        Ok((bevy::ui::widget::NodeImageMode::Sliced(bevy::sprite::TextureSlicer {
                            border: bevy::sprite::BorderRect { top, right, bottom, left },
                            center_scale_mode: bevy::sprite::SliceScaleMode::Stretch,
                            sides_scale_mode: bevy::sprite::SliceScaleMode::Stretch,
                            max_corner_scale: 1.0,
                        }), None))
                    }
                    "tiled" => {
                        let tile_x = obj.get("tile_x").and_then(|v| v.as_bool()).unwrap_or(true);
                        let tile_y = obj.get("tile_y").and_then(|v| v.as_bool()).unwrap_or(true);
                        let stretch_value = obj.get("stretch_value").and_then(|v| v.as_f64()).unwrap_or(1.0) as f32;

                        Ok((bevy::ui::widget::NodeImageMode::Tiled {
                            tile_x,
                            tile_y,
                            stretch_value,
                        }, None))
                    }
                    "contain" => Ok((bevy::ui::widget::NodeImageMode::Stretch, Some(ImageScaleMode::Contain))),
                    "cover" => Ok((bevy::ui::widget::NodeImageMode::Stretch, Some(ImageScaleMode::Cover))),
                    _ => Err(format!("Invalid image_mode type: '{}'. Use 'auto', 'stretch', 'sliced', 'tiled', 'contain', or 'cover'", mode_type)),
                };
            }
        }

        // Default to Auto
        Ok((bevy::ui::widget::NodeImageMode::Auto, None))
    }

    /// Parse ImageNode configuration from JSON
    pub fn json_to_image_node_config(json: &Value) -> Result<ImageNodeConfig, String> {
        let obj = json.as_object().ok_or("ImageNode must be an object")?;

        // resource_id is required
        let resource_id = obj
            .get("resource_id")
            .and_then(|v| v.as_str())
            .ok_or("ImageNode requires 'resource_id' field")?
            .to_string();

        // Parse image_mode (optional, defaults to Auto)
        // Returns (NodeImageMode, Option<ImageScaleMode>) for Cover/Contain support
        let (image_mode, scale_mode) = if let Some(mode_val) = obj.get("image_mode") {
            parse_node_image_mode(mode_val)?
        } else {
            (bevy::ui::widget::NodeImageMode::Auto, None)
        };

        // Parse flip options
        let flip_x = obj.get("flip_x").and_then(|v| v.as_bool()).unwrap_or(false);
        let flip_y = obj.get("flip_y").and_then(|v| v.as_bool()).unwrap_or(false);

        // Parse color/tint
        let color = if let Some(color_val) = obj.get("color") {
            Some(parse_color_value(color_val)?)
        } else {
            None
        };

        Ok(ImageNodeConfig {
            resource_id,
            image_mode,
            scale_mode,
            flip_x,
            flip_y,
            color,
        })
    }

    /// Parse color value from JSON (hex string or {r,g,b,a} object)
    fn parse_color_value(json: &Value) -> Result<Color, String> {
        if let Some(s) = json.as_str() {
            // Parse hex color
            let s = s.trim_start_matches('#');
            match s.len() {
                6 => {
                    let r = u8::from_str_radix(&s[0..2], 16).map_err(|_| "Invalid hex color")?;
                    let g = u8::from_str_radix(&s[2..4], 16).map_err(|_| "Invalid hex color")?;
                    let b = u8::from_str_radix(&s[4..6], 16).map_err(|_| "Invalid hex color")?;
                    Ok(Color::srgba(r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0, 1.0))
                }
                8 => {
                    let r = u8::from_str_radix(&s[0..2], 16).map_err(|_| "Invalid hex color")?;
                    let g = u8::from_str_radix(&s[2..4], 16).map_err(|_| "Invalid hex color")?;
                    let b = u8::from_str_radix(&s[4..6], 16).map_err(|_| "Invalid hex color")?;
                    let a = u8::from_str_radix(&s[6..8], 16).map_err(|_| "Invalid hex color")?;
                    Ok(Color::srgba(r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0, a as f32 / 255.0))
                }
                _ => Err("Hex color must be 6 or 8 characters".to_string()),
            }
        } else if let Some(obj) = json.as_object() {
            let r = obj.get("r").and_then(|v| v.as_f64()).unwrap_or(1.0) as f32;
            let g = obj.get("g").and_then(|v| v.as_f64()).unwrap_or(1.0) as f32;
            let b = obj.get("b").and_then(|v| v.as_f64()).unwrap_or(1.0) as f32;
            let a = obj.get("a").and_then(|v| v.as_f64()).unwrap_or(1.0) as f32;
            Ok(Color::srgba(r, g, b, a))
        } else {
            Err("Color must be hex string or {r,g,b,a} object".to_string())
        }
    }

    /// Convert ImageNode component to JSON
    pub fn image_node_to_json(image_node: &bevy::ui::widget::ImageNode) -> Value {
        let image_mode = match image_node.image_mode {
            bevy::ui::widget::NodeImageMode::Auto => "auto",
            bevy::ui::widget::NodeImageMode::Stretch => "stretch",
            bevy::ui::widget::NodeImageMode::Sliced(_) => "sliced",
            bevy::ui::widget::NodeImageMode::Tiled { .. } => "tiled",
        };

        let rgba = image_node.color.to_srgba().to_f32_array();

        json!({
            "image_mode": image_mode,
            "flip_x": image_node.flip_x,
            "flip_y": image_node.flip_y,
            "color": {
                "r": rgba[0],
                "g": rgba[1],
                "b": rgba[2],
                "a": rgba[3]
            }
        })
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
        ResMut<WindowUIRegistry>,
        ResMut<FontRegistry>,
        ResMut<ResourceRegistry>,
        ResMut<PendingAssetRegistry>,
    ),
    mut ecs_registries: (
        ResMut<ScriptEntityRegistry>,
        ResMut<ScriptComponentRegistry>,
        ResMut<DeclaredSystemRegistry>,
        ResMut<EntityEventCallbackRegistry>,
    ),
    asset_server: Res<AssetServer>,
    mut windows: Query<&mut Window>,
    mut app_exit: EventWriter<bevy::app::AppExit>,
    primary_window_query: Query<Entity, With<PrimaryWindow>>,
    mut widget_queries: (
        Query<&mut Text, Without<ScriptEntity>>,
        Query<&mut BackgroundColor, Without<ScriptEntity>>,
        Query<&mut Node, Without<ScriptEntity>>,
        Query<&mut TextColor>,
        Query<(&mut ButtonColors, Option<&Interaction>, Option<&Children>)>,
    ),
    // ECS queries for script components
    script_entity_query: Query<(Entity, &ScriptEntity)>,
    mut script_component_query: Query<(Entity, &mut ScriptComponent)>,
    // ECS queries for native components
    mut native_queries: (
        Query<&mut Transform, With<ScriptEntity>>,
        Query<&mut Sprite, With<ScriptEntity>>,
        Query<&mut Visibility, With<ScriptEntity>>,
    ),
    // Additional UI component queries for ECS API
    mut ui_queries: (
        Query<&mut Node, With<ScriptEntity>>,
        Query<&mut BackgroundColor, With<ScriptEntity>>,
        Query<&mut Text, With<ScriptEntity>>,
        Query<&mut BorderRadius, With<ScriptEntity>>,
        Query<&Interaction, With<ScriptEntity>>,
        Query<&bevy::ui::widget::Button, With<ScriptEntity>>,
        Query<&bevy::ui::widget::ImageNode, With<ScriptEntity>>,
    ),
    // Query for ScriptButtonColors (pseudo-components for button state colors)
    mut button_colors_query: Query<&mut ScriptButtonColors, With<ScriptEntity>>,
) {
    let (cmd_rx, event_tx) = channels;
    let (registry, window_ui_registry, font_registry, resource_registry, pending_assets) = &mut registries;
    let (script_entity_registry, script_component_registry, declared_system_registry, entity_event_callback_registry) = &mut ecs_registries;
    let (text_query, bg_color_query, node_query, text_color_query, button_query) = &mut widget_queries;
    let (transform_query, sprite_query, visibility_query) = &mut native_queries;
    let (ecs_node_query, ecs_bg_color_query, ecs_text_query, ecs_border_radius_query, ecs_interaction_query, ecs_button_query, ecs_image_node_query) = &mut ui_queries;
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

                let window_entity = commands.spawn(window).id();
                registry.register(id, window_entity);

                // Create camera and root UI node for this window immediately
                // This ensures ECS entities can be parented to the window even before any widget is created
                let camera_entity = commands.spawn((
                    Camera2d::default(),
                    Camera {
                        target: RenderTarget::Window(WindowRef::Entity(window_entity)),
                        clear_color: ClearColorConfig::Custom(Color::srgb(0.1, 0.1, 0.1)),
                        ..default()
                    },
                )).id();
                window_ui_registry.set_window_camera(id, camera_entity);
                tracing::debug!("Created camera {:?} for window {}", camera_entity, id);

                // Create root UI node for this window
                let root = commands.spawn((
                    Node {
                        width: Val::Percent(100.0),
                        height: Val::Percent(100.0),
                        flex_direction: bevy::ui::FlexDirection::Column,
                        align_items: bevy::ui::AlignItems::Stretch,
                        ..default()
                    },
                    UiTargetCamera(camera_entity),
                )).id();
                window_ui_registry.set_window_root(id, root);
                tracing::debug!("Created root UI node {:?} for window {}", root, id);

                let _ = response_tx.send(Ok(()));

                // Send window created event
                let _ = event_tx.0.try_send(GraphicEvent::WindowCreated { window_id: id });
            }

            GraphicCommand::CloseWindow { id, response_tx } => {
                tracing::debug!("Closing window {}", id);

                if let Some(entity) = registry.unregister(id) {
                    // Cleanup window-associated resources
                    // Remove and despawn the camera for this window
                    if let Some(camera_entity) = window_ui_registry.remove_window_camera(id) {
                        commands.entity(camera_entity).despawn();
                        tracing::debug!("Despawned camera {:?} for window {}", camera_entity, id);
                    }

                    // Remove and despawn the root UI node for this window (and all children)
                    // Note: In Bevy 0.17+, despawn() automatically despawns descendants
                    if let Some(root_entity) = window_ui_registry.remove_window_root(id) {
                        commands.entity(root_entity).despawn();
                        tracing::debug!("Despawned root UI node {:?} for window {}", root_entity, id);
                    }

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

            // ================================================================
            // ECS Commands
            // ================================================================

            GraphicCommand::SpawnEntity {
                components,
                owner_mod,
                parent,
                response_tx,
            } => {
                use native_component_converters::*;

                // Allocate a new script ID
                let script_id = script_entity_registry.allocate_id();

                // Spawn the entity with the ScriptEntity marker
                let mut entity_commands = commands.spawn(ScriptEntity {
                    script_id,
                    owner_mod: owner_mod.clone(),
                });

                // Track if this entity has UI components (Node, Text, BackgroundColor)
                // If so, we'll parent it to the main window's root UI node (unless parent is specified)
                let mut is_ui_entity = false;

                // Track button color pseudo-components
                let mut normal_color: Option<Color> = None;
                let mut hover_color: Option<Color> = None;
                let mut pressed_color: Option<Color> = None;
                let mut disabled_color: Option<Color> = None;
                let mut is_disabled: Option<bool> = None;

                // Add initial components, separating native from custom
                for (component_name, component_data) in components {
                    // Handle button color pseudo-components
                    match component_name.as_str() {
                        "HoverBackgroundColor" => {
                            if let Ok(bg) = json_to_background_color(&component_data) {
                                hover_color = Some(bg.0);
                            }
                            continue;
                        }
                        "PressedBackgroundColor" => {
                            if let Ok(bg) = json_to_background_color(&component_data) {
                                pressed_color = Some(bg.0);
                            }
                            continue;
                        }
                        "DisabledBackgroundColor" => {
                            if let Ok(bg) = json_to_background_color(&component_data) {
                                disabled_color = Some(bg.0);
                            }
                            continue;
                        }
                        "Disabled" => {
                            // Parse boolean value for disabled state
                            if let Some(disabled) = component_data.as_bool() {
                                is_disabled = Some(disabled);
                            } else if let Some(disabled_str) = component_data.as_str() {
                                is_disabled = Some(disabled_str.eq_ignore_ascii_case("true"));
                            }
                            continue;
                        }
                        _ => {}
                    }

                    // Check if this is a native Bevy component
                    if let Some(native) = NativeComponent::from_name(&component_name) {
                        match native {
                            NativeComponent::Transform => {
                                match json_to_new_transform(&component_data) {
                                    Ok(transform) => {
                                        entity_commands.insert(transform);
                                        tracing::debug!("Added native Transform component to entity {}", script_id);
                                    }
                                    Err(e) => {
                                        tracing::warn!("Failed to create Transform component: {}", e);
                                    }
                                }
                            }
                            NativeComponent::Sprite => {
                                match json_to_new_sprite(&component_data) {
                                    Ok(sprite) => {
                                        entity_commands.insert(sprite);
                                        tracing::debug!("Added native Sprite component to entity {}", script_id);
                                    }
                                    Err(e) => {
                                        tracing::warn!("Failed to create Sprite component: {}", e);
                                    }
                                }
                            }
                            NativeComponent::Visibility => {
                                match json_to_visibility(&component_data) {
                                    Ok(visibility) => {
                                        entity_commands.insert(visibility);
                                        tracing::debug!("Added native Visibility component to entity {}", script_id);
                                    }
                                    Err(e) => {
                                        tracing::warn!("Failed to create Visibility component: {}", e);
                                    }
                                }
                            }
                            // UI Components
                            NativeComponent::Node => {
                                match json_to_new_node(&component_data) {
                                    Ok(node) => {
                                        entity_commands.insert(node);
                                        is_ui_entity = true;  // Mark as UI entity
                                        tracing::debug!("Added native Node component to entity {}", script_id);
                                    }
                                    Err(e) => {
                                        tracing::warn!("Failed to create Node component: {}", e);
                                    }
                                }
                            }
                            NativeComponent::BackgroundColor => {
                                match json_to_background_color(&component_data) {
                                    Ok(bg) => {
                                        // Store the normal color for ScriptButtonColors
                                        normal_color = Some(bg.0);
                                        entity_commands.insert(bg);
                                        is_ui_entity = true;  // Mark as UI entity
                                        tracing::debug!("Added native BackgroundColor component to entity {}", script_id);
                                    }
                                    Err(e) => {
                                        tracing::warn!("Failed to create BackgroundColor component: {}", e);
                                    }
                                }
                            }
                            NativeComponent::Text => {
                                match json_to_text_config(&component_data) {
                                    Ok(config) => {
                                        // Resolve font handle from registry if specified, or use window default
                                        let font_handle = config.font_alias
                                            .as_ref()
                                            .and_then(|alias| font_registry.get_font(alias))
                                            .or_else(|| {
                                                // Try to get window's default font
                                                window_ui_registry.window_roots.keys().next()
                                                    .and_then(|&window_id| {
                                                        let window_font = font_registry.get_window_font(window_id);
                                                        font_registry.get_font(&window_font.family)
                                                    })
                                            });

                                        let text_font = bevy::text::TextFont {
                                            font: font_handle.unwrap_or_default(),
                                            font_size: config.font_size,
                                            ..default()
                                        };

                                        entity_commands.insert((
                                            bevy::prelude::Text::new(config.content),
                                            text_font,
                                            bevy::text::TextColor(config.color),
                                        ));
                                        is_ui_entity = true;  // Mark as UI entity
                                        tracing::debug!("Added native Text component to entity {}", script_id);
                                    }
                                    Err(e) => {
                                        tracing::warn!("Failed to create Text component: {}", e);
                                    }
                                }
                            }
                            NativeComponent::BorderRadius => {
                                match json_to_border_radius(&component_data) {
                                    Ok(radius) => {
                                        entity_commands.insert(radius);
                                        tracing::debug!("Added native BorderRadius component to entity {}", script_id);
                                    }
                                    Err(e) => {
                                        tracing::warn!("Failed to create BorderRadius component: {}", e);
                                    }
                                }
                            }
                            NativeComponent::Interaction => {
                                match json_to_interaction(&component_data) {
                                    Ok(interaction) => {
                                        // Also add PreviousInteraction for change tracking
                                        entity_commands.insert((
                                            interaction,
                                            ScriptEntityPreviousInteraction::default(),
                                        ));
                                        tracing::debug!("Added native Interaction component to entity {}", script_id);
                                    }
                                    Err(e) => {
                                        tracing::warn!("Failed to create Interaction component: {}", e);
                                    }
                                }
                            }
                            NativeComponent::Button => {
                                match json_to_button(&component_data) {
                                    Ok(button) => {
                                        entity_commands.insert(button);
                                        tracing::debug!("Added native Button component to entity {}", script_id);
                                    }
                                    Err(e) => {
                                        tracing::warn!("Failed to create Button component: {}", e);
                                    }
                                }
                            }
                            NativeComponent::ImageNode => {
                                match json_to_image_node_config(&component_data) {
                                    Ok(config) => {
                                        tracing::debug!("ImageNode config: resource='{}', image_mode={:?}, scale_mode={:?}, flip_x={}, flip_y={}",
                                            config.resource_id, config.image_mode, config.scale_mode, config.flip_x, config.flip_y);
                                        // Look up the image handle from the resource registry
                                        if let Some(image_handle) = resource_registry.get_image_handle(&config.resource_id) {
                                            let mut image_node = bevy::ui::widget::ImageNode::new(image_handle.clone());
                                            image_node.image_mode = config.image_mode;
                                            image_node.flip_x = config.flip_x;
                                            image_node.flip_y = config.flip_y;
                                            if let Some(color) = config.color {
                                                image_node.color = color;
                                            }
                                            entity_commands.insert(image_node);

                                            // If Cover or Contain mode, add the CoverContainImage component
                                            // and configure the Node for proper positioning
                                            if let Some(ref scale_mode) = config.scale_mode {
                                                match scale_mode {
                                                    ImageScaleMode::Cover | ImageScaleMode::Contain => {
                                                        entity_commands.insert(CoverContainImage {
                                                            scale_mode: scale_mode.clone(),
                                                            image_handle: image_handle.clone(),
                                                            last_container_size: Vec2::ZERO,
                                                        });
                                                        tracing::debug!("Added CoverContainImage component for {:?} mode on entity {}", scale_mode, script_id);
                                                    }
                                                    _ => {}
                                                }
                                            }

                                            is_ui_entity = true;  // Mark as UI entity
                                            tracing::debug!("Added native ImageNode component to entity {} with resource '{}'", script_id, config.resource_id);
                                        } else {
                                            tracing::warn!("Resource '{}' not found in registry. Make sure to call Resource.load() first.", config.resource_id);
                                        }
                                    }
                                    Err(e) => {
                                        tracing::warn!("Failed to create ImageNode component: {}", e);
                                    }
                                }
                            }
                        }
                    } else {
                        // Custom script component
                        // Validate component data if schema exists
                        if let Err(e) = script_component_registry.validate(&component_name, &component_data) {
                            tracing::warn!("Component '{}' validation warning: {}", component_name, e);
                            // Continue anyway - validation is advisory
                        }

                        entity_commands.with_child(ScriptComponent {
                            type_name: component_name,
                            data: component_data,
                        });
                    }
                }

                // If any button color was specified, add ScriptButtonColors component
                if hover_color.is_some() || pressed_color.is_some() || disabled_color.is_some() {
                    if let Some(normal) = normal_color {
                        entity_commands.insert(ScriptButtonColors {
                            normal,
                            hovered: hover_color,
                            pressed: pressed_color,
                            disabled: disabled_color,
                        });
                        tracing::debug!("Added ScriptButtonColors to entity {}", script_id);
                    } else {
                        tracing::warn!("Button color states specified without BackgroundColor on entity {}", script_id);
                    }
                }

                // If Disabled is true, add the marker component
                if is_disabled == Some(true) {
                    entity_commands.insert(ScriptButtonDisabled);
                    tracing::debug!("Added ScriptButtonDisabled marker to entity {}", script_id);
                }

                let entity = entity_commands.id();
                script_entity_registry.register(script_id, entity);

                // Handle parenting: explicit parent takes precedence over auto-parenting
                if let Some(parent_script_id) = parent {
                    // Explicit parent specified - use it
                    if let Some(parent_entity) = script_entity_registry.get_entity(parent_script_id) {
                        commands.entity(entity).insert(ChildOf(parent_entity));
                        tracing::debug!(
                            "Entity {} parented to entity {} (Bevy {:?})",
                            script_id, parent_script_id, parent_entity
                        );
                    } else {
                        tracing::warn!(
                            "Parent entity {} not found, entity {} will not be parented",
                            parent_script_id, script_id
                        );
                    }
                } else if is_ui_entity {
                    // No explicit parent but this is a UI entity - parent to window root
                    if let Some(main_window_id) = window_ui_registry.window_roots.keys().next().copied() {
                        if let Some(root_entity) = window_ui_registry.get_window_root(main_window_id) {
                            commands.entity(entity).insert(ChildOf(root_entity));
                            tracing::debug!(
                                "UI entity {} parented to window {} root {:?}",
                                script_id, main_window_id, root_entity
                            );
                        } else {
                            tracing::warn!(
                                "No root UI node found for window {}, UI entity {} may not be visible",
                                main_window_id, script_id
                            );
                        }
                    } else {
                        tracing::warn!(
                            "No window available, UI entity {} may not be visible",
                            script_id
                        );
                    }
                }

                tracing::debug!(
                    "Spawned entity {} (Bevy {:?}) for mod '{}'",
                    script_id, entity, owner_mod
                );

                let _ = response_tx.send(Ok(script_id));
            }

            GraphicCommand::DespawnEntity { entity_id, response_tx } => {
                if let Some(entity) = script_entity_registry.unregister(entity_id) {
                    // Clean up any registered event callbacks for this entity
                    entity_event_callback_registry.remove_entity(entity_id);
                    commands.entity(entity).despawn();
                    tracing::debug!("Despawned entity {} (Bevy {:?})", entity_id, entity);
                    let _ = response_tx.send(Ok(()));
                } else {
                    let _ = response_tx.send(Err(format!("Entity {} not found", entity_id)));
                }
            }

            GraphicCommand::InsertComponent {
                entity_id,
                component_name,
                component_data,
                response_tx,
            } => {
                use native_component_converters::*;

                let result = (|| -> Result<(), String> {
                    let entity = script_entity_registry
                        .get_entity(entity_id)
                        .ok_or_else(|| format!("Entity {} not found", entity_id))?;

                    // Check if this is a native Bevy component
                    if let Some(native) = NativeComponent::from_name(&component_name) {
                        match native {
                            NativeComponent::Transform => {
                                // Check if entity already has Transform, update or insert
                                if let Ok(mut transform) = transform_query.get_mut(entity) {
                                    json_to_transform(&component_data, &mut transform)?;
                                    tracing::debug!("Updated native Transform on entity {}", entity_id);
                                } else {
                                    let transform = json_to_new_transform(&component_data)?;
                                    commands.entity(entity).insert(transform);
                                    tracing::debug!("Inserted native Transform on entity {}", entity_id);
                                }
                            }
                            NativeComponent::Sprite => {
                                if let Ok(mut sprite) = sprite_query.get_mut(entity) {
                                    json_to_sprite(&component_data, &mut sprite)?;
                                    tracing::debug!("Updated native Sprite on entity {}", entity_id);
                                } else {
                                    let sprite = json_to_new_sprite(&component_data)?;
                                    commands.entity(entity).insert(sprite);
                                    tracing::debug!("Inserted native Sprite on entity {}", entity_id);
                                }
                            }
                            NativeComponent::Visibility => {
                                let visibility = json_to_visibility(&component_data)?;
                                commands.entity(entity).insert(visibility);
                                tracing::debug!("Inserted/Updated native Visibility on entity {}", entity_id);
                            }
                            // UI Components
                            NativeComponent::Node => {
                                if let Ok(mut node) = ecs_node_query.get_mut(entity) {
                                    json_to_node(&component_data, &mut node)?;
                                    tracing::debug!("Updated native Node on entity {}", entity_id);
                                } else {
                                    let node = json_to_new_node(&component_data)?;
                                    commands.entity(entity).insert(node);
                                    tracing::debug!("Inserted native Node on entity {}", entity_id);
                                }
                            }
                            NativeComponent::BackgroundColor => {
                                let bg = json_to_background_color(&component_data)?;
                                commands.entity(entity).insert(bg);
                                tracing::debug!("Inserted/Updated native BackgroundColor on entity {}", entity_id);
                            }
                            NativeComponent::Text => {
                                // Always re-insert all text components to ensure font_size, font, and color are updated
                                let config = json_to_text_config(&component_data)?;

                                // Resolve font handle from registry if specified, or use window default
                                let font_handle = config.font_alias
                                    .as_ref()
                                    .and_then(|alias| font_registry.get_font(alias))
                                    .or_else(|| {
                                        // Try to get window's default font
                                        window_ui_registry.window_roots.keys().next()
                                            .and_then(|&window_id| {
                                                let window_font = font_registry.get_window_font(window_id);
                                                font_registry.get_font(&window_font.family)
                                            })
                                    });

                                let text_font = bevy::text::TextFont {
                                    font: font_handle.unwrap_or_default(),
                                    font_size: config.font_size,
                                    ..default()
                                };

                                commands.entity(entity).insert((
                                    bevy::prelude::Text::new(config.content),
                                    text_font,
                                    bevy::text::TextColor(config.color),
                                ));
                                tracing::debug!("Updated native Text on entity {}", entity_id);
                            }
                            NativeComponent::BorderRadius => {
                                let radius = json_to_border_radius(&component_data)?;
                                commands.entity(entity).insert(radius);
                                tracing::debug!("Inserted/Updated native BorderRadius on entity {}", entity_id);
                            }
                            NativeComponent::Interaction => {
                                let interaction = json_to_interaction(&component_data)?;
                                // Also add PreviousInteraction for change tracking
                                commands.entity(entity).insert((
                                    interaction,
                                    ScriptEntityPreviousInteraction::default(),
                                ));
                                tracing::debug!("Inserted/Updated native Interaction on entity {}", entity_id);
                            }
                            NativeComponent::Button => {
                                let button = json_to_button(&component_data)?;
                                commands.entity(entity).insert(button);
                                tracing::debug!("Inserted native Button on entity {}", entity_id);
                            }
                            NativeComponent::ImageNode => {
                                let config = json_to_image_node_config(&component_data)?;
                                if let Some(image_handle) = resource_registry.get_image_handle(&config.resource_id) {
                                    let mut image_node = bevy::ui::widget::ImageNode::new(image_handle.clone());
                                    image_node.image_mode = config.image_mode;
                                    image_node.flip_x = config.flip_x;
                                    image_node.flip_y = config.flip_y;
                                    if let Some(color) = config.color {
                                        image_node.color = color;
                                    }
                                    commands.entity(entity).insert(image_node);

                                    // Handle Cover/Contain modes
                                    if let Some(ref scale_mode) = config.scale_mode {
                                        match scale_mode {
                                            ImageScaleMode::Cover | ImageScaleMode::Contain => {
                                                commands.entity(entity).insert(CoverContainImage {
                                                    scale_mode: scale_mode.clone(),
                                                    image_handle: image_handle.clone(),
                                                    last_container_size: Vec2::ZERO,
                                                });
                                                tracing::debug!("Added CoverContainImage component for {:?} mode on entity {}", scale_mode, entity_id);
                                            }
                                            _ => {
                                                // Remove CoverContainImage if switching away from Cover/Contain
                                                commands.entity(entity).remove::<CoverContainImage>();
                                            }
                                        }
                                    } else {
                                        // Remove CoverContainImage if no special scale mode
                                        commands.entity(entity).remove::<CoverContainImage>();
                                    }

                                    tracing::debug!("Inserted/Updated native ImageNode on entity {} with resource '{}'", entity_id, config.resource_id);
                                } else {
                                    return Err(format!("Resource '{}' not found. Make sure to call Resource.load() first.", config.resource_id));
                                }
                            }
                        }
                        return Ok(());
                    }

                    // Custom script component
                    // Validate component data if schema exists
                    if let Err(e) = script_component_registry.validate(&component_name, &component_data) {
                        tracing::warn!("Component '{}' validation warning: {}", component_name, e);
                    }

                    // Check if this component type already exists on the entity
                    // We need to iterate children to find ScriptComponents
                    let mut found = false;
                    for (_comp_entity, mut comp) in script_component_query.iter_mut() {
                        // Check if this component belongs to our entity (is a child)
                        // For now, we use a simpler approach: spawn as child
                        if comp.type_name == component_name {
                            // Component exists, update it
                            comp.data = component_data.clone();
                            found = true;
                            break;
                        }
                    }

                    if !found {
                        // Add new component as child entity
                        commands.entity(entity).with_child(ScriptComponent {
                            type_name: component_name.clone(),
                            data: component_data,
                        });
                    }

                    tracing::debug!("Inserted component '{}' on entity {}", component_name, entity_id);
                    Ok(())
                })();

                let _ = response_tx.send(result);
            }

            GraphicCommand::UpdateComponent {
                entity_id,
                component_name,
                component_data,
                response_tx,
            } => {
                use native_component_converters::*;

                let result = (|| -> Result<(), String> {
                    let entity = script_entity_registry
                        .get_entity(entity_id)
                        .ok_or_else(|| format!("Entity {} not found", entity_id))?;

                    // Handle Disabled pseudo-component
                    if component_name == "Disabled" {
                        let disabled = component_data.as_bool()
                            .or_else(|| component_data.as_str().map(|s| s.eq_ignore_ascii_case("true")))
                            .unwrap_or(false);

                        if disabled {
                            commands.entity(entity).insert(ScriptButtonDisabled);
                            tracing::debug!("Added ScriptButtonDisabled marker to entity {}", entity_id);
                        } else {
                            commands.entity(entity).remove::<ScriptButtonDisabled>();
                            tracing::debug!("Removed ScriptButtonDisabled marker from entity {}", entity_id);
                        }
                        return Ok(());
                    }

                    // Handle button color pseudo-components (HoverBackgroundColor, PressedBackgroundColor, DisabledBackgroundColor)
                    // These update the ScriptButtonColors component if present on the entity
                    if component_name == "HoverBackgroundColor"
                        || component_name == "PressedBackgroundColor"
                        || component_name == "DisabledBackgroundColor"
                    {
                        if let Ok(mut button_colors) = button_colors_query.get_mut(entity) {
                            let color = json_to_color(&component_data)?;
                            match component_name.as_str() {
                                "HoverBackgroundColor" => {
                                    button_colors.hovered = Some(color);
                                    tracing::debug!("Updated HoverBackgroundColor on entity {}", entity_id);
                                }
                                "PressedBackgroundColor" => {
                                    button_colors.pressed = Some(color);
                                    tracing::debug!("Updated PressedBackgroundColor on entity {}", entity_id);
                                }
                                "DisabledBackgroundColor" => {
                                    button_colors.disabled = Some(color);
                                    tracing::debug!("Updated DisabledBackgroundColor on entity {}", entity_id);
                                }
                                _ => unreachable!()
                            }
                            return Ok(());
                        } else {
                            // Entity doesn't have ScriptButtonColors - create one with default normal color
                            // and the specified state color
                            let normal_color = if let Ok(bg) = ecs_bg_color_query.get(entity) {
                                bg.0
                            } else {
                                Color::srgb(0.3, 0.3, 0.3) // default gray
                            };
                            let color = json_to_color(&component_data)?;
                            let button_colors = match component_name.as_str() {
                                "HoverBackgroundColor" => ScriptButtonColors {
                                    normal: normal_color,
                                    hovered: Some(color),
                                    pressed: None,
                                    disabled: None,
                                },
                                "PressedBackgroundColor" => ScriptButtonColors {
                                    normal: normal_color,
                                    hovered: None,
                                    pressed: Some(color),
                                    disabled: None,
                                },
                                "DisabledBackgroundColor" => ScriptButtonColors {
                                    normal: normal_color,
                                    hovered: None,
                                    pressed: None,
                                    disabled: Some(color),
                                },
                                _ => unreachable!()
                            };
                            commands.entity(entity).insert(button_colors);
                            tracing::debug!("Created ScriptButtonColors with {} on entity {}", component_name, entity_id);
                            return Ok(());
                        }
                    }

                    // Check if this is a native Bevy component
                    if let Some(native) = NativeComponent::from_name(&component_name) {
                        match native {
                            NativeComponent::Node => {
                                // Get existing Node and merge with new data
                                // Use ecs_node_query which includes ScriptEntity entities
                                if let Ok(existing_node) = ecs_node_query.get(entity) {
                                    let mut new_node = existing_node.clone();
                                    json_to_node(&component_data, &mut new_node)?;
                                    commands.entity(entity).insert(new_node);
                                    tracing::debug!("Updated native Node on entity {}", entity_id);
                                } else {
                                    return Err(format!("Entity {} does not have Node component", entity_id));
                                }
                            }
                            NativeComponent::Text => {
                                // For Text update, we only support updating the value field
                                // Other fields (font_size, color) require full insert
                                if let Ok(mut existing_text) = ecs_text_query.get_mut(entity) {
                                    // Only update text content if provided
                                    if let Some(value) = component_data.get("value").or(component_data.get("content")) {
                                        if let Some(text_str) = value.as_str() {
                                            **existing_text = text_str.to_string();
                                            tracing::debug!("Updated native Text value on entity {}", entity_id);
                                        }
                                    }
                                } else {
                                    return Err(format!("Entity {} does not have Text component", entity_id));
                                }
                            }
                            NativeComponent::BackgroundColor => {
                                // For simple components, just replace (no merge needed for single value)
                                let bg = json_to_background_color(&component_data)?;
                                commands.entity(entity).insert(bg);
                                // Also update normal color in ScriptButtonColors if present
                                if let Ok(mut button_colors) = button_colors_query.get_mut(entity) {
                                    button_colors.normal = bg.0;
                                    tracing::debug!("Updated normal color in ScriptButtonColors on entity {}", entity_id);
                                }
                                tracing::debug!("Updated native BackgroundColor on entity {}", entity_id);
                            }
                            NativeComponent::BorderRadius => {
                                let radius = json_to_border_radius(&component_data)?;
                                commands.entity(entity).insert(radius);
                                tracing::debug!("Updated native BorderRadius on entity {}", entity_id);
                            }
                            _ => {
                                return Err(format!("Update not supported for component '{}'", component_name));
                            }
                        }
                        return Ok(());
                    }

                    // Custom script component - merge data
                    for (_comp_entity, mut comp) in script_component_query.iter_mut() {
                        if comp.type_name == component_name {
                            let merged = merge_json(&comp.data, &component_data);
                            comp.data = merged;
                            tracing::debug!("Updated component '{}' on entity {}", component_name, entity_id);
                            return Ok(());
                        }
                    }

                    Err(format!("Component '{}' not found on entity {}", component_name, entity_id))
                })();

                let _ = response_tx.send(result);
            }

            GraphicCommand::RemoveComponent {
                entity_id,
                component_name,
                response_tx,
            } => {
                let result = (|| -> Result<(), String> {
                    let entity = script_entity_registry
                        .get_entity(entity_id)
                        .ok_or_else(|| format!("Entity {} not found", entity_id))?;

                    // Check if this is a native Bevy component
                    if let Some(native) = NativeComponent::from_name(&component_name) {
                        match native {
                            NativeComponent::Transform => {
                                if transform_query.get(entity).is_ok() {
                                    commands.entity(entity).remove::<Transform>();
                                    tracing::debug!("Removed native Transform from entity {}", entity_id);
                                    return Ok(());
                                }
                            }
                            NativeComponent::Sprite => {
                                if sprite_query.get(entity).is_ok() {
                                    commands.entity(entity).remove::<Sprite>();
                                    tracing::debug!("Removed native Sprite from entity {}", entity_id);
                                    return Ok(());
                                }
                            }
                            NativeComponent::Visibility => {
                                if visibility_query.get(entity).is_ok() {
                                    commands.entity(entity).remove::<Visibility>();
                                    tracing::debug!("Removed native Visibility from entity {}", entity_id);
                                    return Ok(());
                                }
                            }
                            // UI Components
                            NativeComponent::Node => {
                                if ecs_node_query.get(entity).is_ok() {
                                    commands.entity(entity).remove::<Node>();
                                    tracing::debug!("Removed native Node from entity {}", entity_id);
                                    return Ok(());
                                }
                            }
                            NativeComponent::BackgroundColor => {
                                if ecs_bg_color_query.get(entity).is_ok() {
                                    commands.entity(entity).remove::<BackgroundColor>();
                                    tracing::debug!("Removed native BackgroundColor from entity {}", entity_id);
                                    return Ok(());
                                }
                            }
                            NativeComponent::Text => {
                                if ecs_text_query.get(entity).is_ok() {
                                    commands.entity(entity).remove::<Text>();
                                    tracing::debug!("Removed native Text from entity {}", entity_id);
                                    return Ok(());
                                }
                            }
                            NativeComponent::BorderRadius => {
                                if ecs_border_radius_query.get(entity).is_ok() {
                                    commands.entity(entity).remove::<BorderRadius>();
                                    tracing::debug!("Removed native BorderRadius from entity {}", entity_id);
                                    return Ok(());
                                }
                            }
                            NativeComponent::Interaction => {
                                commands.entity(entity).remove::<Interaction>();
                                commands.entity(entity).remove::<ScriptEntityPreviousInteraction>();
                                tracing::debug!("Removed native Interaction from entity {}", entity_id);
                                return Ok(());
                            }
                            NativeComponent::Button => {
                                commands.entity(entity).remove::<bevy::ui::widget::Button>();
                                tracing::debug!("Removed native Button from entity {}", entity_id);
                                return Ok(());
                            }
                            NativeComponent::ImageNode => {
                                commands.entity(entity).remove::<bevy::ui::widget::ImageNode>();
                                tracing::debug!("Removed native ImageNode from entity {}", entity_id);
                                return Ok(());
                            }
                        }
                        return Err(format!(
                            "Native component '{}' not found on entity {}",
                            component_name, entity_id
                        ));
                    }

                    // Custom script component
                    // Find and despawn the component child entity
                    let mut removed = false;
                    for (comp_entity, comp) in script_component_query.iter() {
                        if comp.type_name == component_name {
                            commands.entity(comp_entity).despawn();
                            removed = true;
                            break;
                        }
                    }

                    if removed {
                        tracing::debug!("Removed component '{}' from entity {}", component_name, entity_id);
                        Ok(())
                    } else {
                        Err(format!(
                            "Component '{}' not found on entity {}",
                            component_name, entity_id
                        ))
                    }
                })();

                let _ = response_tx.send(result);
            }

            GraphicCommand::GetComponent {
                entity_id,
                component_name,
                response_tx,
            } => {
                use native_component_converters::*;

                let result = (|| -> Result<Option<serde_json::Value>, String> {
                    let entity = script_entity_registry
                        .get_entity(entity_id)
                        .ok_or_else(|| format!("Entity {} not found", entity_id))?;

                    // Check if this is a native Bevy component
                    if let Some(native) = NativeComponent::from_name(&component_name) {
                        match native {
                            NativeComponent::Transform => {
                                if let Ok(transform) = transform_query.get(entity) {
                                    return Ok(Some(transform_to_json(&transform)));
                                }
                            }
                            NativeComponent::Sprite => {
                                if let Ok(sprite) = sprite_query.get(entity) {
                                    return Ok(Some(sprite_to_json(&sprite)));
                                }
                            }
                            NativeComponent::Visibility => {
                                if let Ok(visibility) = visibility_query.get(entity) {
                                    return Ok(Some(visibility_to_json(&visibility)));
                                }
                            }
                            // UI Components
                            NativeComponent::Node => {
                                if let Ok(node) = ecs_node_query.get(entity) {
                                    return Ok(Some(node_to_json(&node)));
                                }
                            }
                            NativeComponent::BackgroundColor => {
                                if let Ok(bg) = ecs_bg_color_query.get(entity) {
                                    return Ok(Some(background_color_to_json(&bg)));
                                }
                            }
                            NativeComponent::Text => {
                                if let Ok(text) = ecs_text_query.get(entity) {
                                    return Ok(Some(text_to_json(&text)));
                                }
                            }
                            NativeComponent::BorderRadius => {
                                if let Ok(radius) = ecs_border_radius_query.get(entity) {
                                    return Ok(Some(border_radius_to_json(&radius)));
                                }
                            }
                            NativeComponent::Interaction => {
                                if let Ok(interaction) = ecs_interaction_query.get(entity) {
                                    return Ok(Some(interaction_to_json(&interaction)));
                                }
                            }
                            NativeComponent::Button => {
                                if ecs_button_query.get(entity).is_ok() {
                                    // Button is a marker component, return true/empty object
                                    return Ok(Some(serde_json::json!(true)));
                                }
                            }
                            NativeComponent::ImageNode => {
                                if let Ok(image_node) = ecs_image_node_query.get(entity) {
                                    return Ok(Some(image_node_to_json(&image_node)));
                                }
                            }
                        }
                        return Ok(None);
                    }

                    // Custom script component
                    // Find the component among children
                    for (_comp_entity, comp) in script_component_query.iter() {
                        if comp.type_name == component_name {
                            return Ok(Some(comp.data.clone()));
                        }
                    }

                    Ok(None)
                })();

                let _ = response_tx.send(result);
            }

            GraphicCommand::HasComponent {
                entity_id,
                component_name,
                response_tx,
            } => {
                let result = (|| -> Result<bool, String> {
                    let entity = script_entity_registry
                        .get_entity(entity_id)
                        .ok_or_else(|| format!("Entity {} not found", entity_id))?;

                    // Check if this is a native Bevy component
                    if let Some(native) = NativeComponent::from_name(&component_name) {
                        match native {
                            NativeComponent::Transform => {
                                return Ok(transform_query.get(entity).is_ok());
                            }
                            NativeComponent::Sprite => {
                                return Ok(sprite_query.get(entity).is_ok());
                            }
                            NativeComponent::Visibility => {
                                return Ok(visibility_query.get(entity).is_ok());
                            }
                            // UI Components
                            NativeComponent::Node => {
                                return Ok(ecs_node_query.get(entity).is_ok());
                            }
                            NativeComponent::BackgroundColor => {
                                return Ok(ecs_bg_color_query.get(entity).is_ok());
                            }
                            NativeComponent::Text => {
                                return Ok(ecs_text_query.get(entity).is_ok());
                            }
                            NativeComponent::BorderRadius => {
                                return Ok(ecs_border_radius_query.get(entity).is_ok());
                            }
                            NativeComponent::Interaction => {
                                return Ok(ecs_interaction_query.get(entity).is_ok());
                            }
                            NativeComponent::Button => {
                                return Ok(ecs_button_query.get(entity).is_ok());
                            }
                            NativeComponent::ImageNode => {
                                return Ok(ecs_image_node_query.get(entity).is_ok());
                            }
                        }
                    }

                    // Custom script component
                    // Check if component exists among children
                    for (_comp_entity, comp) in script_component_query.iter() {
                        if comp.type_name == component_name {
                            return Ok(true);
                        }
                    }

                    Ok(false)
                })();

                let _ = response_tx.send(result);
            }

            GraphicCommand::QueryEntities { options, response_tx } => {
                use native_component_converters::*;

                let mut results = Vec::new();

                // Iterate all script entities
                for (entity, script_entity) in script_entity_query.iter() {
                    let mut matches = true;
                    let mut components_data = HashMap::new();

                    // Collect native components for this entity
                    if let Ok(transform) = transform_query.get(entity) {
                        components_data.insert("Transform".to_string(), transform_to_json(&transform));
                    }
                    if let Ok(sprite) = sprite_query.get(entity) {
                        components_data.insert("Sprite".to_string(), sprite_to_json(&sprite));
                    }
                    if let Ok(visibility) = visibility_query.get(entity) {
                        components_data.insert("Visibility".to_string(), visibility_to_json(&visibility));
                    }

                    // Collect UI components for this entity
                    if let Ok(node) = ecs_node_query.get(entity) {
                        components_data.insert("Node".to_string(), node_to_json(&node));
                    }
                    if let Ok(bg_color) = ecs_bg_color_query.get(entity) {
                        components_data.insert("BackgroundColor".to_string(), background_color_to_json(&bg_color));
                    }
                    if let Ok(text) = ecs_text_query.get(entity) {
                        components_data.insert("Text".to_string(), text_to_json(&text));
                    }
                    if let Ok(border_radius) = ecs_border_radius_query.get(entity) {
                        components_data.insert("BorderRadius".to_string(), border_radius_to_json(&border_radius));
                    }
                    if let Ok(interaction) = ecs_interaction_query.get(entity) {
                        components_data.insert("Interaction".to_string(), interaction_to_json(&interaction));
                    }
                    if ecs_button_query.get(entity).is_ok() {
                        components_data.insert("Button".to_string(), serde_json::json!(true));
                    }

                    // Collect custom script components for this entity
                    // Note: This is a simplified approach. In a real implementation,
                    // we'd need to properly associate ScriptComponents with their parent entities.
                    for (_comp_entity, comp) in script_component_query.iter() {
                        components_data.insert(comp.type_name.clone(), comp.data.clone());
                    }

                    // Check "with" conditions
                    for with_name in &options.with_components {
                        if !components_data.contains_key(with_name) {
                            matches = false;
                            break;
                        }
                    }

                    // Check "without" conditions
                    if matches {
                        for without_name in &options.without_components {
                            if components_data.contains_key(without_name) {
                                matches = false;
                                break;
                            }
                        }
                    }

                    if matches {
                        // Only include requested components in result
                        let mut result_components = HashMap::new();
                        for with_name in &options.with_components {
                            if let Some(data) = components_data.get(with_name) {
                                result_components.insert(with_name.clone(), data.clone());
                            }
                        }

                        results.push(QueryResult {
                            entity_id: script_entity.script_id,
                            components: result_components,
                        });

                        // Check limit
                        if let Some(limit) = options.limit {
                            if results.len() >= limit {
                                break;
                            }
                        }
                    }
                }

                tracing::debug!("Query returned {} entities", results.len());
                let _ = response_tx.send(Ok(results));
            }

            GraphicCommand::RegisterComponent { schema, response_tx } => {
                let result = script_component_registry.register(schema);
                let _ = response_tx.send(result);
            }

            GraphicCommand::DeclareSystem { system, response_tx } => {
                let result = declared_system_registry.register(system);
                let _ = response_tx.send(result);
            }

            GraphicCommand::SetSystemEnabled { name, enabled, response_tx } => {
                let result = if let Some(system) = declared_system_registry.get_mut(&name) {
                    system.enabled = enabled;
                    tracing::debug!("System '{}' enabled={}", name, enabled);
                    Ok(())
                } else {
                    Err(format!("System '{}' not found", name))
                };
                let _ = response_tx.send(result);
            }

            GraphicCommand::RemoveSystem { name, response_tx } => {
                let result = if declared_system_registry.remove(&name).is_some() {
                    tracing::debug!("Removed system '{}'", name);
                    Ok(())
                } else {
                    Err(format!("System '{}' not found", name))
                };
                let _ = response_tx.send(result);
            }

            GraphicCommand::RegisterEntityEventCallback {
                entity_id,
                event_type,
                response_tx,
            } => {
                entity_event_callback_registry.register(entity_id, &event_type);
                tracing::trace!("Registered '{}' callback for entity {}", event_type, entity_id);
                let _ = response_tx.send(Ok(()));
            }

            GraphicCommand::UnregisterEntityEventCallback {
                entity_id,
                event_type,
                response_tx,
            } => {
                entity_event_callback_registry.unregister(entity_id, &event_type);
                tracing::trace!("Unregistered '{}' callback for entity {}", event_type, entity_id);
                let _ = response_tx.send(Ok(()));
            }
        }
    }
}

// Note: Legacy widget functions (create_widget_entity, update_widget_property) have been removed.
// Use the ECS API (World.spawn, entity.insert, entity.update) instead.

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
/// Note: Camera creation is now lazy - cameras are created per-window when the first
/// ECS entity is created for that window (in SpawnEntity handler).
fn register_primary_window(
    mut registry: ResMut<WindowRegistry>,
    mut window_ui_registry: ResMut<WindowUIRegistry>,
    primary_window_query: Query<Entity, With<PrimaryWindow>>,
    mut commands: Commands,
) {
    if let Ok(window_entity) = primary_window_query.single() {
        registry.register(PRIMARY_WINDOW_ID, window_entity);
        tracing::debug!("Registered primary window as ID {}", PRIMARY_WINDOW_ID);

        // Create camera and root UI node for the primary window immediately
        // This ensures ECS entities can be parented to the window even before any widget is created
        let camera_entity = commands.spawn((
            Camera2d::default(),
            Camera {
                target: RenderTarget::Window(WindowRef::Entity(window_entity)),
                clear_color: ClearColorConfig::Custom(Color::srgb(0.1, 0.1, 0.1)),
                ..default()
            },
        )).id();
        window_ui_registry.set_window_camera(PRIMARY_WINDOW_ID, camera_entity);
        tracing::debug!("Created camera {:?} for primary window {}", camera_entity, PRIMARY_WINDOW_ID);

        // Create root UI node for this window
        let root = commands.spawn((
            Node {
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                flex_direction: bevy::ui::FlexDirection::Column,
                align_items: bevy::ui::AlignItems::Stretch,
                ..default()
            },
            UiTargetCamera(camera_entity),
        )).id();
        window_ui_registry.set_window_root(PRIMARY_WINDOW_ID, root);
        tracing::debug!("Created root UI node {:?} for primary window {}", root, PRIMARY_WINDOW_ID);
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

    //tracing::trace!("check_pending_assets: {} assets pending", pending_assets.pending.len());

    // Take all pending assets and check each one
    let mut still_pending = Vec::new();

    for entry in std::mem::take(&mut pending_assets.pending) {
        // Check if the asset is loaded
        let load_state = asset_server.get_load_state(entry.handle_id);
        // tracing::trace!(
        //     "Asset '{}' (id={:?}) load_state={:?}",
        //     entry.alias, entry.handle_id, load_state
        // );

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

// Note: handle_widget_interactions has been removed - use ECS entity event callbacks instead

/// Component to track previous interaction state for ECS entities
#[derive(Component, Default)]
struct ScriptEntityPreviousInteraction(Interaction);

/// System to handle interactions on ECS entities with Button + Interaction components
fn handle_script_entity_interactions(
    mut commands: Commands,
    event_tx: Res<EventSenderRes>,
    event_callback_registry: Res<EntityEventCallbackRegistry>,
    mut changed_query: Query<
        (
            &ScriptEntity,
            &Interaction,
            &mut ScriptEntityPreviousInteraction,
        ),
        (Changed<Interaction>, With<bevy::ui::widget::Button>),
    >,
    all_buttons_query: Query<&Interaction, With<bevy::ui::widget::Button>>,
    windows: Query<(Entity, &Window), With<PrimaryWindow>>,
) {
    // Get window entity and cursor position for events
    let (window_entity, cursor_pos) = windows
        .single()
        .map(|(entity, w)| (Some(entity), w.cursor_position().unwrap_or(Vec2::ZERO)))
        .unwrap_or((None, Vec2::ZERO));

    // Process changed interactions and send events
    for (script_entity, interaction, mut prev_interaction) in changed_query.iter_mut() {
        let prev = prev_interaction.0;
        prev_interaction.0 = *interaction;

        // Only send event when state actually changes
        if *interaction != prev {
            let entity_id = script_entity.script_id;

            // Check if entity has a direct click callback registered
            // If so, send EntityEventCallback on "pressed" instead of EntityInteractionChanged
            if *interaction == Interaction::Pressed && event_callback_registry.has_callback(entity_id, "click") {
                tracing::trace!(
                    "Entity {} clicked - triggering direct callback at ({}, {})",
                    entity_id,
                    cursor_pos.x,
                    cursor_pos.y
                );
                let _ = event_tx.0.try_send(GraphicEvent::EntityEventCallback {
                    entity_id,
                    event_type: "click".to_string(),
                    x: cursor_pos.x,
                    y: cursor_pos.y,
                });
            } else {
                // Send generic interaction changed event
                let interaction_str = match *interaction {
                    Interaction::None => "none",
                    Interaction::Hovered => "hovered",
                    Interaction::Pressed => "pressed",
                };

                tracing::trace!(
                    "Entity {} interaction changed to '{}' at ({}, {})",
                    entity_id,
                    interaction_str,
                    cursor_pos.x,
                    cursor_pos.y
                );

                let _ = event_tx.0.try_send(GraphicEvent::EntityInteractionChanged {
                    entity_id,
                    interaction: interaction_str.to_string(),
                    x: cursor_pos.x,
                    y: cursor_pos.y,
                });
            }
        }
    }

    // Check if any button is hovered to update cursor
    let any_hovered = all_buttons_query.iter().any(|interaction| {
        *interaction == Interaction::Hovered || *interaction == Interaction::Pressed
    });

    // Update window cursor based on hover state
    if let Some(window_entity) = window_entity {
        let cursor = if any_hovered {
            CursorIcon::System(SystemCursorIcon::Pointer)
        } else {
            CursorIcon::System(SystemCursorIcon::Default)
        };
        commands.entity(window_entity).insert(cursor);
    }
}

/// System to apply ScriptButtonColors based on interaction state
///
/// This system automatically updates the BackgroundColor of entities that have
/// ScriptButtonColors component based on their current Interaction state.
fn apply_script_button_colors(
    mut query: Query<
        (
            &Interaction,
            &ScriptButtonColors,
            &mut BackgroundColor,
            Option<&ScriptButtonDisabled>,
        ),
        (Changed<Interaction>, With<bevy::ui::widget::Button>),
    >,
) {
    for (interaction, colors, mut bg_color, disabled) in query.iter_mut() {
        let is_disabled = disabled.is_some();
        let new_color = colors.color_for_interaction(*interaction, is_disabled);
        *bg_color = BackgroundColor(new_color);
    }
}

/// System to update button colors when ScriptButtonDisabled is added or removed.
/// This complements apply_script_button_colors which only triggers on Interaction changes.
fn apply_disabled_button_colors(
    mut query: Query<
        (
            &Interaction,
            &ScriptButtonColors,
            &mut BackgroundColor,
        ),
        (
            Changed<ScriptButtonDisabled>,
            With<bevy::ui::widget::Button>,
        ),
    >,
    disabled_query: Query<&ScriptButtonDisabled>,
) {
    for (interaction, colors, mut bg_color) in query.iter_mut() {
        // Check if entity has ScriptButtonDisabled (we can't use Option in Changed<> filter)
        let is_disabled = true; // If we got here via Changed<ScriptButtonDisabled>, it was added
        let new_color = colors.color_for_interaction(*interaction, is_disabled);
        *bg_color = BackgroundColor(new_color);
    }
}

/// System to update button colors when ScriptButtonDisabled is removed.
/// RemovedComponents detects when the marker component is removed from an entity.
fn apply_enabled_button_colors(
    mut removed: RemovedComponents<ScriptButtonDisabled>,
    mut query: Query<
        (
            &Interaction,
            &ScriptButtonColors,
            &mut BackgroundColor,
        ),
        With<bevy::ui::widget::Button>,
    >,
) {
    for entity in removed.read() {
        if let Ok((interaction, colors, mut bg_color)) = query.get_mut(entity) {
            let new_color = colors.color_for_interaction(*interaction, false);
            *bg_color = BackgroundColor(new_color);
        }
    }
}

/// System to update Cover/Contain images based on actual image dimensions
///
/// This system runs each frame and checks for images with CoverContainImage component.
/// It calculates the appropriate dimensions to achieve the Cover or Contain effect,
/// and recalculates when the container size changes (e.g., window resize).
///
/// For Cover: Image fills container while maintaining aspect ratio (may be cropped)
/// For Contain: Image fits inside container while maintaining aspect ratio (may letterbox)
fn update_cover_contain_images(
    mut query: Query<(Entity, &mut Node, &mut CoverContainImage, &ChildOf)>,
    parent_query: Query<&ComputedNode>,
    images: Res<Assets<Image>>,
) {
    for (entity, mut node, mut cover_contain, child_of) in query.iter_mut() {
        // Get the image to find its dimensions
        let Some(image) = images.get(&cover_contain.image_handle) else {
            // Image not loaded yet, try again next frame
            continue;
        };

        // Get parent container dimensions (this is the wrapper container we created)
        let parent_size = parent_query
            .get(child_of.parent())
            .map(|cn| cn.size())
            .unwrap_or(Vec2::ZERO);

        // Skip if container has zero dimensions (not yet laid out)
        if parent_size.x <= 0.0 || parent_size.y <= 0.0 {
            continue;
        }

        // Skip if container size hasn't changed (optimization to avoid recalculating every frame)
        if cover_contain.last_container_size == parent_size {
            continue;
        }

        // Get image dimensions
        let image_width = image.width() as f32;
        let image_height = image.height() as f32;
        let image_ratio = image_width / image_height;

        let container_width = parent_size.x;
        let container_height = parent_size.y;
        let container_ratio = container_width / container_height;

        tracing::debug!(
            "Cover/Contain sizing for entity {:?}: image={}x{} (ratio={:.3}), container={}x{} (ratio={:.3}), mode={:?}",
            entity, image_width, image_height, image_ratio,
            container_width, container_height, container_ratio,
            cover_contain.scale_mode
        );

        // For both Cover and Contain, we use absolute positioning with transform centering.
        // Set position_type to Absolute and position at 50%/50%, then use negative margins to center.
        node.position_type = PositionType::Absolute;
        node.left = Val::Percent(50.0);
        node.top = Val::Percent(50.0);

        match cover_contain.scale_mode {
            ImageScaleMode::Cover => {
                // For Cover: the node must fill the container completely while maintaining aspect ratio.
                // The image uses Stretch mode, so the node dimensions determine the final appearance.
                // We calculate node dimensions such that:
                // - At least one dimension fills the container exactly
                // - The other dimension overflows (and is clipped by the parent)
                // - The aspect ratio of the node matches the image aspect ratio
                // - The node is centered using position: absolute + left/top: 50% + negative margins

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
                // - The node is centered using position: absolute + left/top: 50% + negative margins

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
                // Other modes shouldn't have this component
            }
        }

        // Remember the container size to detect changes
        cover_contain.last_container_size = parent_size;
    }
}

// ============================================================================
// Declared Systems Execution
// ============================================================================

/// Run all declared systems (behaviors and formulas)
///
/// This system iterates through all enabled declared systems and executes
/// their behaviors on matching entities. This runs every frame after
/// process_commands but before event handlers.
fn run_declared_systems(
    time: Res<Time>,
    declared_system_registry: Res<DeclaredSystemRegistry>,
    script_entity_registry: Res<ScriptEntityRegistry>,
    script_entity_query: Query<(Entity, &ScriptEntity)>,
    mut script_component_query: Query<(Entity, &mut ScriptComponent)>,
    mut transform_query: Query<&mut Transform, With<ScriptEntity>>,
    mut commands: Commands,
) {
    let dt = time.delta_secs();
    let total_time = time.elapsed_secs();

    // Get sorted list of enabled systems
    let systems: Vec<_> = {
        let mut systems: Vec<_> = declared_system_registry
            .systems
            .values()
            .filter(|s| s.enabled)
            .collect();
        systems.sort_by_key(|s| s.order);
        systems
    };

    if systems.is_empty() {
        return;
    }

    // For each declared system
    for system in systems {
        // Skip systems without behavior or formulas
        if system.behavior.is_none() && system.formulas.is_none() {
            continue;
        }

        // Find matching entities based on query
        let matching_entities: Vec<Entity> = script_entity_query
            .iter()
            .filter_map(|(entity, script_entity)| {
                // Check "with" components
                let has_required = system.query.with_components.iter().all(|comp_name| {
                    // Check if entity has this component
                    // For native components, check directly
                    if comp_name == "Transform" {
                        return transform_query.get(entity).is_ok();
                    }
                    // For script components, search
                    script_component_query.iter().any(|(comp_entity, comp)| {
                        // Check if this component belongs to this entity
                        // Note: We need a parent relationship check here
                        comp.type_name == *comp_name
                    })
                });

                if !has_required {
                    return None;
                }

                // Check "without" components
                let has_excluded = system.query.without_components.iter().any(|comp_name| {
                    if comp_name == "Transform" {
                        return transform_query.get(entity).is_ok();
                    }
                    script_component_query
                        .iter()
                        .any(|(_, comp)| comp.type_name == *comp_name)
                });

                if has_excluded {
                    return None;
                }

                Some(entity)
            })
            .collect();

        // Execute behavior or formulas for each matching entity
        for entity in matching_entities {
            // Execute behavior if present
            if let Some(behavior) = &system.behavior {
                execute_behavior(
                    behavior,
                    entity,
                    &system.config,
                    dt,
                    total_time,
                    &mut transform_query,
                    &mut script_component_query,
                    &mut commands,
                );
            }

            // Execute formulas if present
            if let Some(formulas) = &system.formulas {
                execute_formulas(
                    formulas,
                    entity,
                    dt,
                    total_time,
                    &mut transform_query,
                    &mut script_component_query,
                );
            }
        }
    }
}

/// Execute a single behavior on an entity
fn execute_behavior(
    behavior: &SystemBehavior,
    entity: Entity,
    config: &Option<serde_json::Value>,
    dt: f32,
    _total_time: f32,
    transform_query: &mut Query<&mut Transform, With<ScriptEntity>>,
    script_component_query: &mut Query<(Entity, &mut ScriptComponent)>,
    _commands: &mut Commands,
) {
    match behavior {
        SystemBehavior::ApplyVelocity => {
            // Get Velocity component data
            let velocity_data = script_component_query
                .iter()
                .find(|(_, comp)| comp.type_name == "Velocity")
                .map(|(_, comp)| comp.data.clone());

            if let Some(velocity) = velocity_data {
                let vx = velocity.get("x").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32;
                let vy = velocity.get("y").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32;
                let vz = velocity.get("z").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32;

                if let Ok(mut transform) = transform_query.get_mut(entity) {
                    transform.translation.x += vx * dt;
                    transform.translation.y += vy * dt;
                    transform.translation.z += vz * dt;
                }
            }
        }

        SystemBehavior::ApplyGravity => {
            // Get config
            let strength = config
                .as_ref()
                .and_then(|c| c.get("strength"))
                .and_then(|v| v.as_f64())
                .unwrap_or(980.0) as f32;

            let direction = config
                .as_ref()
                .and_then(|c| c.get("direction"))
                .and_then(|v| v.as_str())
                .unwrap_or("down");

            let (gx, gy) = match direction {
                "down" => (0.0, -strength),
                "up" => (0.0, strength),
                "left" => (-strength, 0.0),
                "right" => (strength, 0.0),
                _ => (0.0, -strength),
            };

            // Update Velocity component
            for (_, mut comp) in script_component_query.iter_mut() {
                if comp.type_name == "Velocity" {
                    if let Some(obj) = comp.data.as_object_mut() {
                        let current_x = obj.get("x").and_then(|v| v.as_f64()).unwrap_or(0.0);
                        let current_y = obj.get("y").and_then(|v| v.as_f64()).unwrap_or(0.0);
                        obj.insert("x".to_string(), serde_json::json!(current_x + gx as f64 * dt as f64));
                        obj.insert("y".to_string(), serde_json::json!(current_y + gy as f64 * dt as f64));
                    }
                }
            }
        }

        SystemBehavior::ApplyFriction => {
            let factor = config
                .as_ref()
                .and_then(|c| c.get("factor"))
                .and_then(|v| v.as_f64())
                .unwrap_or(0.98) as f32;

            // Friction factor per second - convert to per-frame
            let frame_factor = factor.powf(dt);

            for (_, mut comp) in script_component_query.iter_mut() {
                if comp.type_name == "Velocity" {
                    if let Some(obj) = comp.data.as_object_mut() {
                        if let Some(x) = obj.get("x").and_then(|v| v.as_f64()) {
                            obj.insert("x".to_string(), serde_json::json!(x * frame_factor as f64));
                        }
                        if let Some(y) = obj.get("y").and_then(|v| v.as_f64()) {
                            obj.insert("y".to_string(), serde_json::json!(y * frame_factor as f64));
                        }
                        if let Some(z) = obj.get("z").and_then(|v| v.as_f64()) {
                            obj.insert("z".to_string(), serde_json::json!(z * frame_factor as f64));
                        }
                    }
                }
            }
        }

        SystemBehavior::RegenerateOverTime => {
            let field = config
                .as_ref()
                .and_then(|c| c.get("field"))
                .and_then(|v| v.as_str())
                .unwrap_or("current");
            let rate = config
                .as_ref()
                .and_then(|c| c.get("rate"))
                .and_then(|v| v.as_f64())
                .unwrap_or(1.0);
            let max_field = config
                .as_ref()
                .and_then(|c| c.get("max_field"))
                .and_then(|v| v.as_str());

            // Find and update the relevant component
            for (_, mut comp) in script_component_query.iter_mut() {
                if let Some(obj) = comp.data.as_object_mut() {
                    if let Some(current) = obj.get(field).and_then(|v| v.as_f64()) {
                        let new_value = current + rate * dt as f64;
                        let clamped = if let Some(max_name) = max_field {
                            if let Some(max_val) = obj.get(max_name).and_then(|v| v.as_f64()) {
                                new_value.min(max_val)
                            } else {
                                new_value
                            }
                        } else {
                            new_value
                        };
                        obj.insert(field.to_string(), serde_json::json!(clamped));
                    }
                }
            }
        }

        SystemBehavior::DecayOverTime => {
            let field = config
                .as_ref()
                .and_then(|c| c.get("field"))
                .and_then(|v| v.as_str())
                .unwrap_or("current");
            let rate = config
                .as_ref()
                .and_then(|c| c.get("rate"))
                .and_then(|v| v.as_f64())
                .unwrap_or(1.0);
            let min_field = config
                .as_ref()
                .and_then(|c| c.get("min_field"))
                .and_then(|v| v.as_str());

            for (_, mut comp) in script_component_query.iter_mut() {
                if let Some(obj) = comp.data.as_object_mut() {
                    if let Some(current) = obj.get(field).and_then(|v| v.as_f64()) {
                        let new_value = current - rate * dt as f64;
                        let clamped = if let Some(min_name) = min_field {
                            if let Some(min_val) = obj.get(min_name).and_then(|v| v.as_f64()) {
                                new_value.max(min_val)
                            } else {
                                new_value.max(0.0)
                            }
                        } else {
                            new_value.max(0.0)
                        };
                        obj.insert(field.to_string(), serde_json::json!(clamped));
                    }
                }
            }
        }

        SystemBehavior::DespawnWhenZero => {
            let field = config
                .as_ref()
                .and_then(|c| c.get("field"))
                .and_then(|v| v.as_str())
                .unwrap_or("health");

            for (_, comp) in script_component_query.iter() {
                if let Some(obj) = comp.data.as_object() {
                    if let Some(value) = obj.get(field).and_then(|v| v.as_f64()) {
                        if value <= 0.0 {
                            _commands.entity(entity).despawn();
                            tracing::debug!("Despawned entity due to {} reaching zero", field);
                        }
                    }
                }
            }
        }

        // The following behaviors require more complex implementation
        // and will be completed in a future iteration
        SystemBehavior::FollowEntity => {
            tracing::warn!("TODO: FollowEntity behavior not yet implemented");
        }

        SystemBehavior::OrbitAround => {
            tracing::warn!("TODO: OrbitAround behavior not yet implemented");
        }

        SystemBehavior::BounceOnBounds => {
            tracing::warn!("TODO: BounceOnBounds behavior not yet implemented");
        }

        SystemBehavior::AnimateSprite => {
            tracing::warn!("TODO: AnimateSprite behavior not yet implemented");
        }
    }
}

/// Execute mathematical formulas on an entity
///
/// Formulas are parsed and evaluated using evalexpr.
/// Available variables:
/// - `dt` - Delta time (seconds since last frame)
/// - `time` - Total elapsed time (seconds)
/// - `ComponentName_field` - Component field values (dot replaced with underscore)
///
/// Formula format: "ComponentName.field = expression"
fn execute_formulas(
    formulas: &[String],
    entity: Entity,
    dt: f32,
    total_time: f32,
    transform_query: &mut Query<&mut Transform, With<ScriptEntity>>,
    script_component_query: &mut Query<(Entity, &mut ScriptComponent)>,
) {
    use evalexpr::*;

    // Build context with all available variables
    let mut context = HashMapContext::new();

    // Add time variables
    if let Err(e) = context.set_value("dt".to_string(), Value::Float(dt as f64)) {
        tracing::warn!("Failed to set dt: {:?}", e);
    }
    if let Err(e) = context.set_value("time".to_string(), Value::Float(total_time as f64)) {
        tracing::warn!("Failed to set time: {:?}", e);
    }

    // Add Transform fields if entity has Transform
    if let Ok(transform) = transform_query.get(entity) {
        let _ = context.set_value("Transform_translation_x".to_string(), Value::Float(transform.translation.x as f64));
        let _ = context.set_value("Transform_translation_y".to_string(), Value::Float(transform.translation.y as f64));
        let _ = context.set_value("Transform_translation_z".to_string(), Value::Float(transform.translation.z as f64));
        let _ = context.set_value("Transform_scale_x".to_string(), Value::Float(transform.scale.x as f64));
        let _ = context.set_value("Transform_scale_y".to_string(), Value::Float(transform.scale.y as f64));
        let _ = context.set_value("Transform_scale_z".to_string(), Value::Float(transform.scale.z as f64));
    }

    // Add script component fields
    for (_, comp) in script_component_query.iter() {
        if let Some(obj) = comp.data.as_object() {
            for (field_name, value) in obj {
                let var_name = format!("{}_{}", comp.type_name, field_name);
                if let Some(num) = value.as_f64() {
                    let _ = context.set_value(var_name, Value::Float(num));
                } else if let Some(s) = value.as_str() {
                    let _ = context.set_value(var_name, Value::String(s.to_string()));
                } else if let Some(b) = value.as_bool() {
                    let _ = context.set_value(var_name, Value::Boolean(b));
                }
            }
        }
    }

    // Define math functions
    let _ = context.set_function("sin".to_string(), Function::new(|arg: &Value| {
        Ok(Value::Float(arg.as_float()?.sin()))
    }));
    let _ = context.set_function("cos".to_string(), Function::new(|arg: &Value| {
        Ok(Value::Float(arg.as_float()?.cos()))
    }));
    let _ = context.set_function("tan".to_string(), Function::new(|arg: &Value| {
        Ok(Value::Float(arg.as_float()?.tan()))
    }));
    let _ = context.set_function("abs".to_string(), Function::new(|arg: &Value| {
        Ok(Value::Float(arg.as_float()?.abs()))
    }));
    let _ = context.set_function("sqrt".to_string(), Function::new(|arg: &Value| {
        Ok(Value::Float(arg.as_float()?.sqrt()))
    }));
    let _ = context.set_function("pow".to_string(), Function::new(|arg: &Value| {
        let tuple = arg.as_tuple()?;
        if tuple.len() != 2 {
            return Err(EvalexprError::WrongFunctionArgumentAmount { expected: 2..=2, actual: tuple.len() });
        }
        let base: f64 = tuple[0].as_float()?;
        let exp: f64 = tuple[1].as_float()?;
        Ok(Value::Float(base.powf(exp)))
    }));
    let _ = context.set_function("min".to_string(), Function::new(|arg: &Value| {
        let tuple = arg.as_tuple()?;
        if tuple.len() != 2 {
            return Err(EvalexprError::WrongFunctionArgumentAmount { expected: 2..=2, actual: tuple.len() });
        }
        let a: f64 = tuple[0].as_float()?;
        let b: f64 = tuple[1].as_float()?;
        Ok(Value::Float(a.min(b)))
    }));
    let _ = context.set_function("max".to_string(), Function::new(|arg: &Value| {
        let tuple = arg.as_tuple()?;
        if tuple.len() != 2 {
            return Err(EvalexprError::WrongFunctionArgumentAmount { expected: 2..=2, actual: tuple.len() });
        }
        let a: f64 = tuple[0].as_float()?;
        let b: f64 = tuple[1].as_float()?;
        Ok(Value::Float(a.max(b)))
    }));
    let _ = context.set_function("clamp".to_string(), Function::new(|arg: &Value| {
        let tuple = arg.as_tuple()?;
        if tuple.len() != 3 {
            return Err(EvalexprError::WrongFunctionArgumentAmount { expected: 3..=3, actual: tuple.len() });
        }
        let val: f64 = tuple[0].as_float()?;
        let min_val: f64 = tuple[1].as_float()?;
        let max_val: f64 = tuple[2].as_float()?;
        Ok(Value::Float(val.clamp(min_val, max_val)))
    }));
    let _ = context.set_function("lerp".to_string(), Function::new(|arg: &Value| {
        let tuple = arg.as_tuple()?;
        if tuple.len() != 3 {
            return Err(EvalexprError::WrongFunctionArgumentAmount { expected: 3..=3, actual: tuple.len() });
        }
        let a: f64 = tuple[0].as_float()?;
        let b: f64 = tuple[1].as_float()?;
        let t: f64 = tuple[2].as_float()?;
        Ok(Value::Float(a + (b - a) * t))
    }));

    // Store results to apply after evaluation
    let mut results: Vec<(String, String, f64)> = Vec::new();

    // Parse and evaluate each formula
    for formula in formulas {
        // Parse formula: "Component.field = expression"
        let parts: Vec<&str> = formula.splitn(2, '=').collect();
        if parts.len() != 2 {
            tracing::warn!("Invalid formula syntax (missing '='): {}", formula);
            continue;
        }

        let target = parts[0].trim();
        let expression = parts[1].trim();

        // Parse target: "Component.field"
        let target_parts: Vec<&str> = target.split('.').collect();
        if target_parts.len() != 2 {
            tracing::warn!("Invalid target format (expected Component.field): {}", target);
            continue;
        }

        let component_name = target_parts[0];
        let field_name = target_parts[1];

        // Convert expression to use underscore notation for variable names
        // e.g., "Transform.translation.x" -> "Transform_translation_x"
        let converted_expr = convert_dot_notation(expression);

        // Evaluate the expression
        match eval_with_context(&converted_expr, &context) {
            Ok(result) => {
                if let Ok(value) = result.as_float() {
                    results.push((component_name.to_string(), field_name.to_string(), value));
                } else {
                    tracing::warn!("Formula result is not a number: {} = {:?}", formula, result);
                }
            }
            Err(e) => {
                tracing::warn!("Formula evaluation error: {} - {:?}", formula, e);
            }
        }
    }

    // Apply results to components
    for (component_name, field_name, value) in results {
        if component_name == "Transform" {
            if let Ok(mut transform) = transform_query.get_mut(entity) {
                match field_name.as_str() {
                    "x" | "translation_x" => transform.translation.x = value as f32,
                    "y" | "translation_y" => transform.translation.y = value as f32,
                    "z" | "translation_z" => transform.translation.z = value as f32,
                    "scale_x" => transform.scale.x = value as f32,
                    "scale_y" => transform.scale.y = value as f32,
                    "scale_z" => transform.scale.z = value as f32,
                    "scale" => {
                        transform.scale.x = value as f32;
                        transform.scale.y = value as f32;
                        transform.scale.z = value as f32;
                    }
                    _ => {
                        tracing::warn!("Unknown Transform field: {}", field_name);
                    }
                }
            }
        } else {
            // Update script component
            for (_, mut comp) in script_component_query.iter_mut() {
                if comp.type_name == component_name {
                    if let Some(obj) = comp.data.as_object_mut() {
                        obj.insert(field_name.clone(), serde_json::json!(value));
                    }
                }
            }
        }
    }
}

/// Convert dot notation to underscore notation for evalexpr
/// e.g., "Transform.translation.x" -> "Transform_translation_x"
/// e.g., "Oscillator.speed" -> "Oscillator_speed"
fn convert_dot_notation(expr: &str) -> String {
    let mut result = String::with_capacity(expr.len());
    let mut chars = expr.chars().peekable();
    let mut in_identifier = false;

    while let Some(c) = chars.next() {
        if c.is_alphanumeric() || c == '_' {
            in_identifier = true;
            result.push(c);
        } else if c == '.' && in_identifier {
            // Check if next char is alphanumeric (meaning this is a component.field access)
            if chars.peek().map_or(false, |next| next.is_alphanumeric()) {
                result.push('_');
            } else {
                result.push(c);
            }
        } else {
            in_identifier = false;
            result.push(c);
        }
    }

    result
}
