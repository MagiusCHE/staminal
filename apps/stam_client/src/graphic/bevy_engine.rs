//! Bevy Engine Implementation
//!
//! This module implements the GraphicEngine trait using Bevy.
//! The engine runs in a separate thread and communicates with the main
//! application via channels.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use bevy::prelude::*;
use bevy::window::{PresentMode, WindowMode, WindowPosition, WindowResolution, WindowRef};
use bevy::input::keyboard::KeyboardInput;
use bevy::input::mouse::{MouseButtonInput, MouseMotion, MouseWheel};
use bevy::input::ButtonState;
use bevy::winit::WinitPlugin;
use bevy::camera::RenderTarget;
use tokio::sync::mpsc;

use stam_mod_runtimes::api::graphic::{
    FrameSnapshot, GamepadState, GraphicCommand, GraphicEngine, GraphicEngines, GraphicEvent,
    KeyModifiers, MouseButton, MouseButtonState, WindowConfig, WindowPositionMode,
};

/// Bevy engine implementation
pub struct BevyEngine;

impl BevyEngine {
    /// Create a new Bevy engine instance
    pub fn new() -> Self {
        Self
    }
}

impl Default for BevyEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl GraphicEngine for BevyEngine {
    fn run(
        &mut self,
        command_rx: mpsc::Receiver<GraphicCommand>,
        event_tx: mpsc::Sender<GraphicEvent>,
    ) {
        // Wrap channels in Arc<Mutex> for sharing with Bevy systems
        let command_rx = Arc::new(Mutex::new(command_rx));
        let event_tx = Arc::new(Mutex::new(event_tx));

        // Create Bevy app
        let mut app = App::new();

        // Configure WinitPlugin
        // Note: We no longer need run_on_any_thread because require_main_thread()
        // ensures this engine runs on the main thread
        let winit_plugin = WinitPlugin::<bevy::winit::WakeUp>::default();

        // Create a primary window at startup.
        // Note: Due to a Bevy bug (since v0.13), windows spawned dynamically when
        // primary_window: None don't appear. See: https://github.com/bevyengine/bevy/issues/12237
        // As a workaround, we create a hidden primary window that will be configured
        // via the first CreateWindow command.
        app.add_plugins(
            DefaultPlugins
                .set(WindowPlugin {
                    primary_window: Some(Window {
                        title: "Staminal".into(),
                        resolution: WindowResolution::new(1280, 720),
                        visible: false,  // Hidden until configured via command
                        ..default()
                    }),
                    // Don't auto-exit when windows close - we handle this manually
                    exit_condition: bevy::window::ExitCondition::DontExit,
                    // Don't auto-close windows - we need to handle the event first
                    close_when_requested: false,
                    ..default()
                })
                .set(winit_plugin)
                // Disable Bevy's LogPlugin because we already have our own tracing setup
                // This prevents duplicate log output and ensures our STAM_GFXENGINELOG filter works
                .disable::<bevy::log::LogPlugin>()
                .build(),
        );

        // Spawn a Camera2d for the primary window at startup
        app.add_systems(Startup, setup_primary_window);

        // Add our resources
        app.insert_resource(CommandReceiver(command_rx));
        app.insert_resource(EventSender(event_tx));
        app.insert_resource(WindowRegistry::default());
        app.insert_resource(InputState::default());
        app.insert_resource(FrameState::default());
        app.insert_resource(PrimaryWindowEntity::default());

        // Add our systems
        // IMPORTANT: process_commands runs first and spawns windows via Commands.
        // Using chain() automatically inserts apply_deferred between systems that use Commands,
        // ensuring the window entity exists before other systems run.
        app.add_systems(
            Update,
            (
                process_commands,
                collect_keyboard_input,
                collect_mouse_input,
                send_frame_events,
                handle_window_events,
                check_window_destroyed,
            ).chain(),
        );

        // Send engine ready event
        if let Ok(tx) = app.world().resource::<EventSender>().0.lock() {
            let _ = tx.blocking_send(GraphicEvent::EngineReady);
        }

        tracing::info!("Bevy app configured, starting main loop...");

        // Run Bevy's main loop (blocks until exit)
        app.run();

        tracing::info!("Bevy main loop exited");

        // Send shutdown event
        // Note: This may not execute if app.run() panics
    }

    fn engine_type(&self) -> GraphicEngines {
        GraphicEngines::Bevy
    }

    fn require_main_thread(&self) -> bool {
        // Bevy with winit requires the main thread on Linux/Wayland
        // for proper window event handling (close, resize, etc.)
        true
    }
}

// === Bevy Resources ===

/// Resource holding the command receiver channel
#[derive(Resource)]
struct CommandReceiver(Arc<Mutex<mpsc::Receiver<GraphicCommand>>>);

/// Resource holding the event sender channel
#[derive(Resource)]
struct EventSender(Arc<Mutex<mpsc::Sender<GraphicEvent>>>);

/// Registry mapping our window IDs to Bevy Entity IDs
#[derive(Resource, Default)]
struct WindowRegistry {
    /// Map from our window ID to Bevy window entity
    windows: HashMap<u64, Entity>,
    /// Map from Bevy entity to our window ID
    entities: HashMap<Entity, u64>,
}

/// Current input state for frame snapshots
#[derive(Resource, Default)]
struct InputState {
    /// Mouse position
    mouse_x: f32,
    mouse_y: f32,
    /// Mouse buttons
    mouse_buttons: MouseButtonState,
    /// Currently pressed keys
    pressed_keys: Vec<String>,
    /// Gamepad states
    gamepads: [GamepadState; 4],
    /// Number of connected gamepads
    gamepad_count: u8,
}

/// Frame timing state
#[derive(Resource, Default)]
struct FrameState {
    /// Current frame number
    frame_number: u64,
    /// Primary window ID for frame events
    primary_window_id: Option<u64>,
}

/// Track the primary window entity so we can reuse it for the first CreateWindow command
#[derive(Resource, Default)]
struct PrimaryWindowEntity(Option<Entity>);

// === Bevy Systems ===

/// Setup system that runs at Startup to configure the primary window
fn setup_primary_window(
    mut commands: Commands,
    mut primary_window_entity: ResMut<PrimaryWindowEntity>,
    primary_window_query: Query<Entity, With<bevy::window::PrimaryWindow>>,
) {
    // Find and store the primary window entity
    if let Ok(entity) = primary_window_query.single() {
        primary_window_entity.0 = Some(entity);
        tracing::debug!("Primary window entity stored: {:?}", entity);

        // Spawn a Camera2d targeting the primary window
        commands.spawn((
            Camera2d,
            Camera {
                target: RenderTarget::Window(WindowRef::Entity(entity)),
                ..default()
            },
        ));
        tracing::debug!("Camera2d spawned for primary window {:?}", entity);
    }
}

/// Process incoming commands from the main thread
fn process_commands(
    mut commands: Commands,
    command_rx: Res<CommandReceiver>,
    event_tx: Res<EventSender>,
    mut window_registry: ResMut<WindowRegistry>,
    mut frame_state: ResMut<FrameState>,
    mut primary_window_entity: ResMut<PrimaryWindowEntity>,
    mut app_exit: EventWriter<AppExit>,
    mut windows: Query<&mut Window>,
) {
    // Try to receive commands (non-blocking)
    let mut rx = match command_rx.0.lock() {
        Ok(rx) => rx,
        Err(_) => return,
    };

    while let Ok(cmd) = rx.try_recv() {
        match cmd {
            GraphicCommand::CreateWindow {
                id,
                config,
                response_tx,
            } => {
                tracing::info!("CreateWindow command received: id={}, title={}, size={}x{}, visible={}",
                    id, config.title, config.width, config.height, config.visible);

                // For the first window, reuse the primary window entity created at startup.
                // This works around a Bevy bug where dynamically spawned windows don't appear
                // when primary_window: None is set.
                let entity = if let Some(primary_entity) = primary_window_entity.0.take() {
                    // Reuse the existing primary window - just update its properties
                    if let Ok(mut window) = windows.get_mut(primary_entity) {
                        window.title = config.title.clone();
                        window.resolution.set(config.width as f32, config.height as f32);
                        window.mode = if config.fullscreen {
                            WindowMode::BorderlessFullscreen(MonitorSelection::Current)
                        } else {
                            WindowMode::Windowed
                        };
                        window.resizable = config.resizable;
                        window.visible = config.visible;
                        tracing::info!("Primary window reconfigured: {:?}", primary_entity);
                    }
                    // Camera was already spawned in setup_primary_window
                    primary_entity
                } else {
                    // Spawn a new window for subsequent CreateWindow commands
                    let window = Window {
                        title: config.title.clone(),
                        resolution: WindowResolution::new(config.width, config.height),
                        present_mode: PresentMode::AutoVsync,
                        mode: if config.fullscreen {
                            WindowMode::BorderlessFullscreen(MonitorSelection::Current)
                        } else {
                            WindowMode::Windowed
                        },
                        resizable: config.resizable,
                        visible: config.visible,
                        ..default()
                    };

                    let new_entity = commands.spawn(window).id();
                    tracing::info!("New window entity spawned: {:?}", new_entity);

                    // Spawn a Camera2d that targets this specific window.
                    commands.spawn((
                        Camera2d,
                        Camera {
                            target: RenderTarget::Window(WindowRef::Entity(new_entity)),
                            ..default()
                        },
                    ));
                    tracing::debug!("Camera2d spawned for window {:?}", new_entity);
                    new_entity
                };

                window_registry.windows.insert(id, entity);
                window_registry.entities.insert(entity, id);

                // Set as primary window if first
                if frame_state.primary_window_id.is_none() {
                    frame_state.primary_window_id = Some(id);
                }

                let _ = response_tx.send(Ok(()));

                // Send window created event
                if let Ok(tx) = event_tx.0.lock() {
                    let _ = tx.blocking_send(GraphicEvent::WindowCreated { window_id: id });
                }
            }

            GraphicCommand::CloseWindow { id, response_tx } => {
                if let Some(entity) = window_registry.windows.remove(&id) {
                    window_registry.entities.remove(&entity);
                    commands.entity(entity).despawn();
                    let _ = response_tx.send(Ok(()));

                    if let Ok(tx) = event_tx.0.lock() {
                        let _ = tx.blocking_send(GraphicEvent::WindowClosed { window_id: id });
                    }
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
                if let Some(&entity) = window_registry.windows.get(&id) {
                    if let Ok(mut window) = windows.get_mut(entity) {
                        window.resolution.set(width as f32, height as f32);
                        let _ = response_tx.send(Ok(()));
                    } else {
                        let _ = response_tx.send(Err(format!("Window entity not found for id {}", id)));
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
                if let Some(&entity) = window_registry.windows.get(&id) {
                    if let Ok(mut window) = windows.get_mut(entity) {
                        window.title = title;
                        let _ = response_tx.send(Ok(()));
                    } else {
                        let _ = response_tx.send(Err(format!("Window entity not found for id {}", id)));
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
                if let Some(&entity) = window_registry.windows.get(&id) {
                    if let Ok(mut window) = windows.get_mut(entity) {
                        window.mode = if fullscreen {
                            WindowMode::BorderlessFullscreen(MonitorSelection::Current)
                        } else {
                            WindowMode::Windowed
                        };
                        let _ = response_tx.send(Ok(()));
                    } else {
                        let _ = response_tx.send(Err(format!("Window entity not found for id {}", id)));
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
                if let Some(&entity) = window_registry.windows.get(&id) {
                    if let Ok(mut window) = windows.get_mut(entity) {
                        window.visible = visible;
                        let _ = response_tx.send(Ok(()));
                    } else {
                        let _ = response_tx.send(Err(format!("Window entity not found for id {}", id)));
                    }
                } else {
                    let _ = response_tx.send(Err(format!("Window {} not found", id)));
                }
            }

            GraphicCommand::SetWindowPosition {
                id,
                x,
                y,
                response_tx,
            } => {
                if let Some(&entity) = window_registry.windows.get(&id) {
                    if let Ok(mut window) = windows.get_mut(entity) {
                        window.position = WindowPosition::At(IVec2::new(x, y));
                        let _ = response_tx.send(Ok(()));
                    } else {
                        let _ = response_tx.send(Err(format!("Window entity not found for id {}", id)));
                    }
                } else {
                    let _ = response_tx.send(Err(format!("Window {} not found", id)));
                }
            }

            GraphicCommand::SetWindowPositionMode { id, mode, response_tx } => {
                if let Some(&entity) = window_registry.windows.get(&id) {
                    if let Ok(mut window) = windows.get_mut(entity) {
                        match mode {
                            WindowPositionMode::Centered => {
                                window.position = WindowPosition::Centered(MonitorSelection::Current);
                            }
                            WindowPositionMode::Default => {
                                window.position = WindowPosition::Automatic;
                            }
                            WindowPositionMode::Manual => {
                                // Keep current position, just mark as manual
                            }
                        }
                        let _ = response_tx.send(Ok(()));
                    } else {
                        let _ = response_tx.send(Err(format!("Window entity not found for id {}", id)));
                    }
                } else {
                    let _ = response_tx.send(Err(format!("Window {} not found", id)));
                }
            }

            GraphicCommand::SetWindowResizable { id, resizable, response_tx } => {
                if let Some(&entity) = window_registry.windows.get(&id) {
                    if let Ok(mut window) = windows.get_mut(entity) {
                        window.resizable = resizable;
                        let _ = response_tx.send(Ok(()));
                    } else {
                        let _ = response_tx.send(Err(format!("Window entity not found for id {}", id)));
                    }
                } else {
                    let _ = response_tx.send(Err(format!("Window {} not found", id)));
                }
            }

            GraphicCommand::GetMousePosition {
                window_id,
                response_tx,
            } => {
                // Return current mouse position from InputState
                // This is accessed synchronously, so we need another approach
                let _ = response_tx.send(Ok((0.0, 0.0)));
            }

            GraphicCommand::IsKeyPressed { key, response_tx } => {
                let _ = response_tx.send(false);
            }

            GraphicCommand::GetPressedKeys { response_tx } => {
                let _ = response_tx.send(Vec::new());
            }

            GraphicCommand::Shutdown { response_tx } => {
                if let Ok(tx) = event_tx.0.lock() {
                    let _ = tx.blocking_send(GraphicEvent::EngineShuttingDown);
                }
                let _ = response_tx.send(Ok(()));
                app_exit.write(AppExit::Success);
            }
        }
    }
}

/// Collect keyboard input events
fn collect_keyboard_input(
    mut keyboard_events: EventReader<KeyboardInput>,
    keyboard_input: Res<ButtonInput<KeyCode>>,
    mut input_state: ResMut<InputState>,
    event_tx: Res<EventSender>,
    window_registry: Res<WindowRegistry>,
) {
    // Update pressed keys list
    input_state.pressed_keys.clear();
    for key in keyboard_input.get_pressed() {
        input_state.pressed_keys.push(format!("{:?}", key));
    }

    // Send keyboard events
    for event in keyboard_events.read() {
        let window_id = window_registry
            .entities
            .get(&event.window)
            .copied()
            .unwrap_or(0);

        let key_name = format!("{:?}", event.key_code);
        let modifiers = KeyModifiers {
            shift: keyboard_input.pressed(KeyCode::ShiftLeft)
                || keyboard_input.pressed(KeyCode::ShiftRight),
            ctrl: keyboard_input.pressed(KeyCode::ControlLeft)
                || keyboard_input.pressed(KeyCode::ControlRight),
            alt: keyboard_input.pressed(KeyCode::AltLeft)
                || keyboard_input.pressed(KeyCode::AltRight),
            meta: keyboard_input.pressed(KeyCode::SuperLeft)
                || keyboard_input.pressed(KeyCode::SuperRight),
        };

        let graphic_event = match event.state {
            ButtonState::Pressed => GraphicEvent::KeyPressed {
                window_id,
                key: key_name,
                modifiers,
            },
            ButtonState::Released => GraphicEvent::KeyReleased {
                window_id,
                key: key_name,
                modifiers,
            },
        };

        if let Ok(tx) = event_tx.0.lock() {
            let _ = tx.blocking_send(graphic_event);
        }
    }
}

/// Collect mouse input events
fn collect_mouse_input(
    mut mouse_button_events: EventReader<MouseButtonInput>,
    mut mouse_motion_events: EventReader<MouseMotion>,
    mut mouse_wheel_events: EventReader<MouseWheel>,
    mouse_input: Res<ButtonInput<bevy::input::mouse::MouseButton>>,
    windows: Query<&Window>,
    mut input_state: ResMut<InputState>,
    event_tx: Res<EventSender>,
    window_registry: Res<WindowRegistry>,
) {
    // Update mouse button state
    input_state.mouse_buttons.left = mouse_input.pressed(bevy::input::mouse::MouseButton::Left);
    input_state.mouse_buttons.right = mouse_input.pressed(bevy::input::mouse::MouseButton::Right);
    input_state.mouse_buttons.middle =
        mouse_input.pressed(bevy::input::mouse::MouseButton::Middle);

    // Get mouse position from primary window
    for window in windows.iter() {
        if let Some(pos) = window.cursor_position() {
            input_state.mouse_x = pos.x;
            input_state.mouse_y = pos.y;
        }
    }

    // Send mouse button events
    for event in mouse_button_events.read() {
        let window_id = window_registry
            .entities
            .get(&event.window)
            .copied()
            .unwrap_or(0);

        let button = match event.button {
            bevy::input::mouse::MouseButton::Left => MouseButton::Left,
            bevy::input::mouse::MouseButton::Right => MouseButton::Right,
            bevy::input::mouse::MouseButton::Middle => MouseButton::Middle,
            bevy::input::mouse::MouseButton::Back => MouseButton::Other(3),
            bevy::input::mouse::MouseButton::Forward => MouseButton::Other(4),
            bevy::input::mouse::MouseButton::Other(n) => MouseButton::Other(n as u8),
        };

        let graphic_event = match event.state {
            ButtonState::Pressed => GraphicEvent::MouseButtonPressed {
                window_id,
                button,
                x: input_state.mouse_x,
                y: input_state.mouse_y,
            },
            ButtonState::Released => GraphicEvent::MouseButtonReleased {
                window_id,
                button,
                x: input_state.mouse_x,
                y: input_state.mouse_y,
            },
        };

        if let Ok(tx) = event_tx.0.lock() {
            let _ = tx.blocking_send(graphic_event);
        }
    }

    // Send mouse wheel events
    for event in mouse_wheel_events.read() {
        if let Ok(tx) = event_tx.0.lock() {
            let _ = tx.blocking_send(GraphicEvent::MouseWheel {
                window_id: 0, // TODO: Get proper window ID
                delta_x: event.x,
                delta_y: event.y,
            });
        }
    }
}

/// Send frame events with input snapshot
fn send_frame_events(
    time: Res<Time>,
    input_state: Res<InputState>,
    mut frame_state: ResMut<FrameState>,
    event_tx: Res<EventSender>,
    windows: Query<(Entity, &Window)>,
) {
    frame_state.frame_number += 1;

    // Debug: Log window count every 60 frames
    if frame_state.frame_number % 60 == 1 {
        let window_count = windows.iter().count();
        tracing::debug!("Frame {}: {} window(s) exist", frame_state.frame_number, window_count);
        for (entity, window) in windows.iter() {
            tracing::debug!("  Window {:?}: title='{}', visible={}, size={}x{}",
                entity, window.title, window.visible,
                window.resolution.width(), window.resolution.height());
        }
    }

    let snapshot = FrameSnapshot {
        delta: time.delta_secs_f64(),
        frame_number: frame_state.frame_number,
        window_id: frame_state.primary_window_id.unwrap_or(0),
        mouse_x: input_state.mouse_x,
        mouse_y: input_state.mouse_y,
        mouse_buttons: input_state.mouse_buttons,
        pressed_keys: input_state.pressed_keys.clone(),
        gamepads: input_state.gamepads,
        gamepad_count: input_state.gamepad_count,
    };

    if let Ok(tx) = event_tx.0.lock() {
        let _ = tx.blocking_send(GraphicEvent::FrameStart { snapshot });
    }
}

/// Handle window events (resize, move, focus, close)
fn handle_window_events(
    mut commands: Commands,
    mut window_resized: EventReader<bevy::window::WindowResized>,
    mut window_moved: EventReader<bevy::window::WindowMoved>,
    mut window_focused: EventReader<bevy::window::WindowFocused>,
    mut window_close_requested: EventReader<bevy::window::WindowCloseRequested>,
    mut app_exit: EventWriter<AppExit>,
    event_tx: Res<EventSender>,
    mut window_registry: ResMut<WindowRegistry>,
    windows: Query<Entity, With<Window>>,
) {
    for event in window_resized.read() {
        if let Some(&window_id) = window_registry.entities.get(&event.window) {
            if let Ok(tx) = event_tx.0.lock() {
                let _ = tx.blocking_send(GraphicEvent::WindowResized {
                    window_id,
                    width: event.width as u32,
                    height: event.height as u32,
                });
            }
        }
    }

    for event in window_moved.read() {
        if let Some(&window_id) = window_registry.entities.get(&event.window) {
            if let Ok(tx) = event_tx.0.lock() {
                let _ = tx.blocking_send(GraphicEvent::WindowMoved {
                    window_id,
                    x: event.position.x,
                    y: event.position.y,
                });
            }
        }
    }

    for event in window_focused.read() {
        if let Some(&window_id) = window_registry.entities.get(&event.window) {
            if let Ok(tx) = event_tx.0.lock() {
                let _ = tx.blocking_send(GraphicEvent::WindowFocused {
                    window_id,
                    focused: event.focused,
                });
            }
        }
    }

    // Handle window close requests - actually close the window and notify
    for event in window_close_requested.read() {
        tracing::info!("Window close requested: {:?}", event.window);

        // Send close event to JS
        if let Some(&window_id) = window_registry.entities.get(&event.window) {
            if let Ok(tx) = event_tx.0.lock() {
                let _ = tx.blocking_send(GraphicEvent::WindowClosed { window_id });
            }
            // Remove from registry
            window_registry.windows.remove(&window_id);
        }
        window_registry.entities.remove(&event.window);

        // Despawn the window entity
        commands.entity(event.window).despawn();

        // Check if this was the last window - if so, send shutdown and exit
        let remaining_windows = windows.iter().count();
        tracing::debug!("Remaining windows after close: {}", remaining_windows);

        // Note: remaining_windows includes the window we just despawned (command not yet applied)
        // So if count is 1, we're closing the last window
        if remaining_windows <= 1 {
            tracing::info!("Last window closed, shutting down engine");
            if let Ok(tx) = event_tx.0.lock() {
                let _ = tx.blocking_send(GraphicEvent::EngineShuttingDown);
            }
            app_exit.write(AppExit::Success);
        }
    }
}

/// Check if windows have been destroyed by the window manager
/// This is a fallback for when WindowCloseRequested events are not delivered
/// (which can happen on secondary threads or with some window managers)
fn check_window_destroyed(
    mut app_exit: EventWriter<AppExit>,
    event_tx: Res<EventSender>,
    mut window_registry: ResMut<WindowRegistry>,
    windows: Query<Entity, With<Window>>,
) {
    // Get all window entities from the query
    let existing_entities: std::collections::HashSet<Entity> = windows.iter().collect();

    // Check if any registered windows no longer exist
    let mut destroyed_entities = Vec::new();
    for (&entity, &window_id) in window_registry.entities.iter() {
        if !existing_entities.contains(&entity) {
            tracing::info!("Window {:?} (id={}) was destroyed by window manager", entity, window_id);
            destroyed_entities.push((entity, window_id));
        }
    }

    // Process destroyed windows
    for (entity, window_id) in destroyed_entities {
        // Notify JS
        if let Ok(tx) = event_tx.0.lock() {
            let _ = tx.blocking_send(GraphicEvent::WindowClosed { window_id });
        }
        // Remove from registry
        window_registry.windows.remove(&window_id);
        window_registry.entities.remove(&entity);
    }

    // If no windows left, shutdown
    if window_registry.windows.is_empty() && window_registry.entities.is_empty() {
        // Only shutdown if we had windows before (don't shutdown on first frame)
        // We check this by seeing if frame_state.primary_window_id is set
        // But we don't have access to frame_state here, so we rely on the window count
        if existing_entities.is_empty() {
            tracing::info!("All windows destroyed, shutting down engine");
            if let Ok(tx) = event_tx.0.lock() {
                let _ = tx.blocking_send(GraphicEvent::EngineShuttingDown);
            }
            app_exit.write(AppExit::Success);
        }
    }
}
