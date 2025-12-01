//! Bevy Engine Implementation
//!
//! This module implements the `GraphicEngine` trait for the Bevy game engine.
//! Bevy runs on the main thread and communicates with the worker thread via channels.

use bevy::prelude::*;
use bevy::window::{PrimaryWindow, WindowMode};
use bevy::winit::{UpdateMode, WinitSettings};
use std::collections::HashMap;
use std::sync::mpsc::Receiver;
use std::sync::Mutex;
use tokio::sync::mpsc::Sender;

use stam_mod_runtimes::api::{
    GraphicCommand, GraphicEngine, GraphicEngineInfo, GraphicEngines, GraphicEvent,
    InitialWindowConfig, KeyModifiers, MouseButton, WindowPositionMode,
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
        app.add_plugins(
            DefaultPlugins
                .build()
                .disable::<bevy::log::LogPlugin>()
                .set(WindowPlugin {
                    primary_window: Some(Window {
                        title: win_config.title.clone(),
                        resolution: (win_config.width as f32, win_config.height as f32).into(),
                        resizable: win_config.resizable,
                        mode,
                        position,
                        visible: true,
                        ..default()
                    }),
                    ..default()
                }),
        );

        // Insert command receiver as non-send resource (uses Mutex for thread safety)
        app.insert_non_send_resource(CommandReceiverRes(Mutex::new(command_rx)));
        app.insert_resource(EventSenderRes(event_tx.clone()));
        app.insert_resource(WindowRegistry::default());
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
            version: "0.15.3".to_string(), // Match our Cargo.toml version
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

/// System to process commands from the worker thread
fn process_commands(
    cmd_rx: NonSend<CommandReceiverRes>,
    event_tx: Res<EventSenderRes>,
    mut commands: Commands,
    mut registry: ResMut<WindowRegistry>,
    mut windows: Query<&mut Window>,
    mut app_exit: EventWriter<AppExit>,
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
                    resolution: (config.width as f32, config.height as f32).into(),
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
                app_exit.send(AppExit::Success);
            }

            GraphicCommand::GetEngineInfo { response_tx } => {
                // Create and send engine info
                let info = GraphicEngineInfo {
                    engine_type: "Bevy".to_string(),
                    engine_type_id: GraphicEngines::Bevy.to_u32(),
                    name: "Bevy Game Engine".to_string(),
                    version: "0.15.3".to_string(),
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
        }
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
fn register_primary_window(
    mut registry: ResMut<WindowRegistry>,
    primary_window_query: Query<Entity, With<PrimaryWindow>>,
) {
    if let Ok(entity) = primary_window_query.get_single() {
        registry.register(PRIMARY_WINDOW_ID, entity);
        tracing::debug!("Registered primary window as ID {}", PRIMARY_WINDOW_ID);
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
    let (x, y) = if let Ok(window) = windows.get_single() {
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
