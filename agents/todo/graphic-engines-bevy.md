# Graphic Engines System - Implementation Guide

## Overview

This document describes the implementation of the advanced graphics system for the Staminal client. The system allows mods to enable and interact with different graphic engines (starting with Bevy) through a unified proxy API.

## Multi-Language Scripting Support

Staminal supports **multiple scripting languages** for mod development:

- **JavaScript** (currently implemented via QuickJS)
- **Lua** (planned)
- **C#** (planned)
- **Other languages** (future)

The GraphicProxy and all graphic APIs are designed to be **language-agnostic**. Each scripting runtime adapter (JS, Lua, C#, etc.) will have its own bindings that expose the same functionality through language-specific idioms.

**Important**: When implementing graphic bindings:

1. The core `GraphicProxy` and `GraphicEngine` trait live in `stam_mod_runtimes/src/api/graphic/`
2. Each adapter (JS, Lua, C#) implements bindings in its own adapter directory
3. All adapters call the same underlying `GraphicProxy` methods
4. The API surface should be consistent across all languages

## Core Principles

### 1. Main Loop Preservation

The client has its own main loop that handles:

- Network communication
- Mod lifecycle management
- Event dispatching
- **JavaScript runtime event loop**
- **Lua runtime event loop** (future)
- **C# runtime event loop** (future)
- Other scripting runtimes as they are added

**This main loop MUST be preserved.** The graphic engine runs in a separate thread and communicates with the main loop through message channels. All scripting runtimes share the same `GraphicProxy` instance.

### 2. Main Thread Reservation for Graphic Engines

**CRITICAL ARCHITECTURE RULE**: Some graphic engines (notably Bevy with winit on Linux/Wayland) **require execution on the main thread**. To support this:

1. **Main thread stays free at startup**: All async application logic (tokio runtime, JS event loop, terminal input, network) runs on a **spawned thread**, keeping the main thread empty and waiting.

2. **Engine activation via channel**: When a mod calls `graphic.enableEngine()`, the engine is created but stored as "pending". The pending engine is sent to the main thread via an `mpsc` channel.

3. **Main thread receives and runs engine**: The main thread blocks on the channel receiver. When an engine arrives, it runs on the main thread (blocking call to `engine.run()`).

4. **Async loop continues**: The spawned thread's async loop does **NOT exit** when sending the engine. It continues running the JS event loop, terminal input, network I/O, etc. in parallel with the graphic engine.

5. **Engine-agnostic design**: The `GraphicEngine` trait has a `require_main_thread()` method (default `false`). Only engines that return `true` (like Bevy) are handled via the pending-engine-to-main-thread flow. Future engines that don't require the main thread can run on any thread.

**Why this matters**: On Linux with Wayland, closing a Bevy window from a secondary thread causes a deadlock in winit's event loop. Running Bevy on the main thread solves this architectural limitation.

```
Application Startup:
+--------------------+     +------------------------------------+
|    Main Thread     |     |       Spawned Thread (tokio)       |
|                    |     |                                    |
|  1. Create tokio   |     |                                    |
|     runtime        |     |                                    |
|                    |     |                                    |
|  2. Create engine  |     |                                    |
|     channel        |     |                                    |
|                    |     |                                    |
|  3. Spawn async    |---->|  4. Run async_main()               |
|     logic          |     |     - Network connection           |
|                    |     |     - Mod loading                  |
|  5. Block on       |     |     - JS event loop                |
|     engine_rx      |     |     - Terminal input               |
|     .recv()        |     |                                    |
|                    |     |  6. Mod calls enableEngine()       |
|                    |     |     -> PendingEngine sent          |
|                    |<----|        via channel                 |
|                    |     |                                    |
|  7. Receive        |     |  8. Loop CONTINUES running         |
|     PendingEngine  |     |     (JS, terminal, network)        |
|                    |     |                                    |
|  8. engine.run()   |     |                                    |
|     (blocking)     |     |                                    |
|                    |     |                                    |
|  Window visible,   |     |  Input events dispatched,          |
|  rendering active  |     |  mods receive callbacks            |
+--------------------+     +------------------------------------+
```

### 3. Graceful Shutdown

When the client main loop terminates (user exit, disconnect, error), a graceful shutdown sequence MUST occur:

1. **Client initiates shutdown**: The main loop sends a shutdown signal
2. **GraphicProxy receives shutdown**: Forwards `Shutdown` command to the engine thread
3. **Engine thread cleanup**: The graphic engine must:
   - Stop accepting new commands
   - Close all open windows gracefully
   - Release all GPU/rendering resources
   - Exit its main loop within a reasonable timeout (e.g., 5 seconds)
4. **Thread join with timeout**: The main thread waits for the engine thread to finish
   - If the engine does not respond within the timeout, the thread is forcibly terminated
5. **Non-blocking guarantee**: The shutdown MUST NOT block the client from exiting

**Critical**: If the graphic engine hangs or fails to shutdown, the client MUST still be able to exit cleanly. Use timeouts and fallback mechanisms.

```rust
impl GraphicProxy {
    /// Shutdown the graphic engine gracefully
    /// Returns Ok(()) if shutdown completed, Err if timeout or failure
    pub async fn shutdown(&self, timeout: Duration) -> Result<(), String> {
        if let Some(tx) = self.command_tx.read().unwrap().as_ref() {
            let (response_tx, response_rx) = oneshot::channel();

            // Send shutdown command
            if tx.send(GraphicCommand::Shutdown { response_tx }).await.is_err() {
                // Channel closed, engine already dead
                return Ok(());
            }

            // Wait for response with timeout
            match tokio::time::timeout(timeout, response_rx).await {
                Ok(Ok(result)) => result,
                Ok(Err(_)) => Ok(()), // Channel closed, engine exited
                Err(_) => {
                    // Timeout - engine did not respond
                    // The thread handle can be used to forcibly terminate if needed
                    Err("Graphic engine shutdown timed out".to_string())
                }
            }
        } else {
            Ok(()) // No engine running
        }
    }
}
```

### 4. Event System (Engine -> Scripts)

The graphic engine generates events that must be dispatched to all registered mod scripts. This enables mods to react to user input, window events, and rendering callbacks.

**Event Flow**:

```
+---------------------+     +------------------+     +----------------------+
|   Graphic Engine    | --> |   GraphicProxy   | --> |   EventDispatcher    |
|   (Bevy Thread)     |     |   (Main Thread)  |     |   (to all mods)      |
+---------------------+     +------------------+     +----------------------+
         |                           |                         |
   Generates events           Receives via            Dispatches to all
   (input, window,            mpsc channel            registered handlers
    render callbacks)                                 in JS/Lua/C#/etc.
```

**Event Types** (from engine to scripts):

```rust
/// Events generated by the graphic engine
pub enum GraphicEvent {
    // Window events
    WindowCreated { window_id: u64 },
    WindowClosed { window_id: u64 },
    WindowResized { window_id: u64, width: u32, height: u32 },
    WindowFocused { window_id: u64, focused: bool },
    WindowMoved { window_id: u64, x: i32, y: i32 },

    // Input events (keyboard)
    KeyPressed { window_id: u64, key: String, modifiers: KeyModifiers },
    KeyReleased { window_id: u64, key: String, modifiers: KeyModifiers },
    CharacterInput { window_id: u64, character: char },

    // Input events (mouse)
    MouseMoved { window_id: u64, x: f32, y: f32 },
    MouseButtonPressed { window_id: u64, button: MouseButton, x: f32, y: f32 },
    MouseButtonReleased { window_id: u64, button: MouseButton, x: f32, y: f32 },
    MouseWheel { window_id: u64, delta_x: f32, delta_y: f32 },

    // Render events
    FrameStart { window_id: u64, delta_time: f32 },
    FrameEnd { window_id: u64, frame_time: f32 },

    // Engine lifecycle
    EngineReady,
    EngineError { message: String },
    EngineShuttingDown,
}

#[derive(Clone, Debug)]
pub struct KeyModifiers {
    pub shift: bool,
    pub ctrl: bool,
    pub alt: bool,
    pub meta: bool,  // Cmd on macOS, Win on Windows
}

#[derive(Clone, Debug)]
pub enum MouseButton {
    Left,
    Right,
    Middle,
    Other(u8),
}
```

**GraphicProxy Event Channel**:

```rust
pub struct GraphicProxy {
    // ... existing fields ...

    /// Channel to receive events FROM the graphic engine thread
    event_rx: Arc<tokio::sync::Mutex<Option<mpsc::Receiver<GraphicEvent>>>>,

    /// Reference to the system event dispatcher for forwarding to mods
    event_dispatcher: Arc<EventDispatcher>,
}

impl GraphicProxy {
    /// Poll for events from the graphic engine and dispatch to mods
    /// This should be called from the main event loop
    pub async fn poll_events(&self) {
        let mut rx_guard = self.event_rx.lock().await;
        if let Some(rx) = rx_guard.as_mut() {
            while let Ok(event) = rx.try_recv() {
                self.dispatch_event(event).await;
            }
        }
    }

    async fn dispatch_event(&self, event: GraphicEvent) {
        // Convert to system event format and dispatch to all mods
        let event_name = match &event {
            GraphicEvent::WindowCreated { .. } => "graphic:window:created",
            GraphicEvent::WindowClosed { .. } => "graphic:window:closed",
            GraphicEvent::KeyPressed { .. } => "graphic:input:key_pressed",
            GraphicEvent::MouseMoved { .. } => "graphic:input:mouse_moved",
            GraphicEvent::FrameStart { .. } => "graphic:render:frame_start",
            // ... etc
        };

        self.event_dispatcher.dispatch(event_name, event.to_args()).await;
    }
}
```

**Script Event Registration** (JavaScript example):

```javascript
// Register frame update callback - this is the main game loop (on graphic object)
graphic.onFrameUpdate((delta) => {
  // All graphic.* methods are automatically optimized inside this callback
  const mouse = graphic.get_mouse_position(); // Uses cached snapshot (O(1))
  const keys = graphic.get_pressed_keys(); // Uses cached snapshot (O(1))

  // Check for escape key to close window
  if (graphic.is_key_pressed("Escape")) {
    graphic.shutdown();
  }

  // Update game logic at ~60fps
  updateGameState(delta);
});

// Create a window and register events on the window object
const win = await graphic.create_window({
  title: "My Game",
  width: 1280,
  height: 720,
});

// Window events are registered on the window instance, not on graphic
win.onResized((width, height, state) => {
  console.log(`Window resized to ${width}x${height} on state ${state}`);
});

win.onRequestClose(async (event) => {
  if (event.reasonId == WindowRequestReasons.UserRequest) {
    // await graprhic.confirm("Are you sure?")
    event.canClose = false;
  } else {
    // defulat
    event.canClose = true; // window can be closed.
  }
});

win.onClosed(() => {
  console.log("Window was closed");
});

win.onFocused((focused) => {
  console.log(`Window focus: ${focused}`);
});

win.onMoved((x, y) => {
  console.log(`Window moved to ${x}, ${y}`);
});
```

**Note**: The graphic event system does NOT use `system.register_event()`. Instead:

1. **Frame updates**: Use `graphic.onFrameUpdate()` on the global `graphic` object
2. **Window events**: Use `win.onResized()`, `win.onClosed()`, etc. on the window instance returned by `graphic.create_window()`

This design allows for:

1. Optimized per-frame callbacks with delta time
2. Automatic input state caching during frame updates
3. Per-window event handling without needing to filter by window ID

### 5. Architecture Layers

```
+-------------------------------------------------------------------------+
|                        Scripting Mods                                   |
|  +-------------------+  +-------------------+  +-------------------+    |
|  |   JavaScript      |  |      Lua          |  |      C#           |    |
|  | await system.     |  | system:enable_    |  | await System.     |    |
|  | enable_graphic_   |  | graphic_engine    |  | EnableGraphic     |    |
|  | engine(...)       |  | (...)             |  | Engine(...)       |    |
|  +---------+---------+  +---------+---------+  +---------+---------+    |
+------------|----------------------|----------------------|--------------+
             |                      |                      |
             v                      v                      v
+-------------------------------------------------------------------------+
|                    Runtime-Specific Bindings                            |
|  +-------------------+  +-------------------+  +-------------------+    |
|  |  JS Bindings      |  |  Lua Bindings     |  |  C# Bindings      |    |
|  |  (adapters/js/)   |  | (adapters/lua/)   |  | (adapters/cs/)    |    |
|  +---------+---------+  +---------+---------+  +---------+---------+    |
+------------|----------------------|----------------------|--------------+
             |                      |                      |
             +----------------------+----------------------+
                                    |
                                    v
+-------------------------------------------------------------------------+
|                         GraphicProxy                                    |
|   - Language-agnostic API layer                                         |
|   - Routes commands to the active graphic engine                        |
|   - Abstracts engine-specific implementations                           |
|   - Thread-safe message passing                                         |
|   - Shared by ALL scripting runtimes                                    |
+-----------------------------------+-------------------------------------+
                                    |
                                    v
+-------------------------------------------------------------------------+
|                      GraphicEngine Trait                                |
|   - Common interface for all engines                                    |
|   - Window management, rendering commands                               |
+-----------------------------------+-------------------------------------+
                                    |
              +---------------------+---------------------+
              |                     |                     |
              v                     v                     v
+---------------------+ +---------------------+ +---------------------+
|    BevyEngine       | |   WgpuEngine        | |   Future Engines    |
|  (Separate Thread)  | |    (Future)         | |        ...          |
+---------------------+ +---------------------+ +---------------------+
```

## Component Details

### GraphicEngines Enum

Exposed to all scripting languages as a global constant object/enum.

```rust
/// Available graphic engines
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum GraphicEngines {
    /// Bevy game engine - full-featured, ECS-based
    Bevy = 0,
    /// Raw WGPU - for custom rendering (future)
    Wgpu = 1,
    /// Terminal/TUI mode - for text-based interfaces (future)
    Terminal = 2,
}
```

### GraphicProxy

The `GraphicProxy` is the central coordinator that:

1. **Manages engine lifecycle**: Starts/stops graphic engines in separate threads
2. **Routes commands**: Forwards window and rendering commands to the active engine
3. **Handles responses**: Receives results from the engine thread and returns them to callers
4. **Thread-safe communication**: Uses `tokio::sync::mpsc` channels for async message passing
5. **Language-agnostic**: Same instance is shared across JS, Lua, C# and all other runtimes

```rust
/// Central proxy for graphic engine operations
///
/// This struct is shared across all mod contexts and ALL scripting runtimes
/// (JavaScript, Lua, C#, etc.). It provides a unified interface for graphic
/// operations regardless of the underlying engine or calling language.
pub struct GraphicProxy {
    /// Currently active engine type (None if no engine enabled)
    active_engine: Arc<RwLock<Option<GraphicEngines>>>,

    /// Channel to send commands to the graphic engine thread
    command_tx: Arc<RwLock<Option<mpsc::Sender<GraphicCommand>>>>,

    /// Channel to receive responses from the graphic engine thread
    response_rx: Arc<tokio::sync::Mutex<Option<mpsc::Receiver<GraphicResponse>>>>,

    /// Handle to the engine thread (for cleanup)
    engine_thread: Arc<RwLock<Option<std::thread::JoinHandle<()>>>>,

    /// Window registry - maps window IDs to their state
    windows: Arc<RwLock<HashMap<u64, WindowInfo>>>,

    /// Next window ID counter
    next_window_id: Arc<AtomicU64>,
}
```

### GraphicCommand Enum

Commands sent from the main thread to the engine thread:

```rust
/// Commands that can be sent to the graphic engine
pub enum GraphicCommand {
    /// Create a new window
    CreateWindow {
        id: u64,
        config: WindowConfig,
        response_tx: oneshot::Sender<Result<(), String>>,
    },

    /// Close a window
    CloseWindow {
        id: u64,
        response_tx: oneshot::Sender<Result<(), String>>,
    },

    /// Update window properties
    SetWindowSize {
        id: u64,
        width: u32,
        height: u32,
        response_tx: oneshot::Sender<Result<(), String>>,
    },

    SetWindowTitle {
        id: u64,
        title: String,
        response_tx: oneshot::Sender<Result<(), String>>,
    },

    SetWindowFullscreen {
        id: u64,
        fullscreen: bool,
        response_tx: oneshot::Sender<Result<(), String>>,
    },

    SetWindowVisible {
        id: u64,
        visible: bool,
        response_tx: oneshot::Sender<Result<(), String>>,
    },

    /// Shutdown the graphic engine
    Shutdown {
        response_tx: oneshot::Sender<Result<(), String>>,
    },
}
```

### GraphicEngine Trait

Common interface that all engine implementations must satisfy:

```rust
/// Trait for graphic engine implementations
///
/// Each engine (Bevy, WGPU, etc.) implements this trait to provide
/// a consistent interface for the GraphicProxy.
pub trait GraphicEngine: Send + 'static {
    /// Initialize the engine and start its main loop
    /// This method runs on the engine thread and should not return
    /// until shutdown is requested.
    fn run(&mut self, command_rx: mpsc::Receiver<GraphicCommand>);

    /// Get the engine type
    fn engine_type(&self) -> GraphicEngines;
}
```

### BevyEngine Implementation

Bevy runs in its own thread with its own event loop:

```rust
/// Bevy engine implementation
pub struct BevyEngine {
    /// Bevy App instance (created when run() is called)
    app: Option<bevy::prelude::App>,
}

impl BevyEngine {
    pub fn new() -> Self {
        Self { app: None }
    }
}

impl GraphicEngine for BevyEngine {
    fn run(&mut self, command_rx: mpsc::Receiver<GraphicCommand>) {
        // Create Bevy app with custom plugin to receive commands
        let mut app = App::new();

        // Add default plugins (windowing, rendering, etc.)
        app.add_plugins(DefaultPlugins);

        // Add our custom command receiver system
        app.insert_resource(CommandReceiver::new(command_rx));
        app.add_systems(Update, process_graphic_commands);

        // Run Bevy's main loop (blocks until exit)
        app.run();
    }

    fn engine_type(&self) -> GraphicEngines {
        GraphicEngines::Bevy
    }
}
```

### WindowConfig & WindowInfo

```rust
/// Configuration for creating a new window
#[derive(Clone, Debug)]
pub struct WindowConfig {
    pub title: String,
    pub width: u32,
    pub height: u32,
    pub fullscreen: bool,
    pub resizable: bool,
    pub visible: bool,
}

impl Default for WindowConfig {
    fn default() -> Self {
        Self {
            title: "Staminal".to_string(),
            width: 1280,
            height: 720,
            fullscreen: false,
            resizable: true,
            visible: true,
        }
    }
}

/// Runtime information about a window
#[derive(Clone, Debug)]
pub struct WindowInfo {
    pub id: u64,
    pub config: WindowConfig,
    pub created: bool,
}
```

## Scripting API Examples

### JavaScript

```javascript
// Enable a graphic engine
await system.enable_graphic_engine(GraphicEngines.Bevy);

// Check if an engine is enabled
const isEnabled = system.is_graphic_engine_enabled();

// Get the current engine type
const engineType = system.get_graphic_engine(); // Returns GraphicEngines.Bevy or null

// Create a new window
const win = await window.create({
  title: "My Game Window",
  width: 1280,
  height: 720,
  fullscreen: false,
  resizable: true,
  visible: true,
});

// Window methods
await win.setSize(800, 600);
await win.setTitle("New Title");
await win.setFullscreen(true);
await win.setVisible(false);
await win.close();

// Window properties (read-only)
console.log(win.id); // Unique window ID
console.log(win.title); // Current title
console.log(win.width); // Current width
console.log(win.height); // Current height
```

### Lua (Future - Example Syntax)

```lua
-- Enable a graphic engine
system:enable_graphic_engine(GraphicEngines.Bevy)

-- Create a new window
local win = window:create({
    title = "My Game Window",
    width = 1280,
    height = 720,
    fullscreen = false,
    resizable = true,
    visible = true
})

-- Window methods
win:set_size(800, 600)
win:set_title("New Title")
win:close()
```

### C# (Future - Example Syntax)

```csharp
// Enable a graphic engine
await System.EnableGraphicEngine(GraphicEngines.Bevy);

// Create a new window
var win = await Window.Create(new WindowConfig {
    Title = "My Game Window",
    Width = 1280,
    Height = 720,
    Fullscreen = false,
    Resizable = true,
    Visible = true
});

// Window methods
await win.SetSize(800, 600);
await win.SetTitle("New Title");
await win.Close();
```

## File Structure

```
apps/shared/stam_mod_runtimes/
   src/
      api/
         mod.rs              # Add: graphic module export
         graphic/
             mod.rs          # GraphicProxy, GraphicCommand, etc.
             engines/
                mod.rs      # GraphicEngine trait, GraphicEngines enum
                bevy.rs     # BevyEngine implementation
             window.rs       # WindowConfig, WindowInfo
      adapters/
          js/
              bindings.rs     # Add: GraphicJS, WindowJS classes for JavaScript
              glue/
                  main.js     # JavaScript glue code (if needed)
          lua/                # Future: Lua adapter
              bindings.rs     # Lua-specific bindings
          cs/                 # Future: C# adapter
              bindings.rs     # C#-specific bindings

apps/stam_client/
   Cargo.toml                  # Add: bevy dependency
   src/
       main.rs                 # Initialize GraphicProxy, pass to ALL runtime adapters
```

## Threading Model

```
+-------------------------------------------------------------------------+
|                          Main Thread                                    |
|                                                                         |
|  +---------------+  +---------------+  +---------------+                |
|  |  Network I/O  |  | JS Runtime    |  | Lua Runtime   |                |
|  |   (tokio)     |  | Event Loop    |  | Event Loop    |  ...           |
|  +---------------+  +-------+-------+  +-------+-------+                |
|                             |                  |                        |
|                     +-------+------------------+-------+                |
|                     |                                  |                |
|                     v                                  |                |
|  +------------------+----------------------------------+-------------+  |
|  |                    GraphicProxy                                   |  |
|  |    (Shared by ALL runtime adapters)                               |  |
|  +--------+--------------------------+-------------------------------+  |
|           |                          ^                                  |
+-----------|--------------------------+----------------------------------+
            |                          |
   mpsc channel (commands)    mpsc channel (events)
            |                          |
            v                          |
+-------------------------------------------------------------------------+
|                    Graphic Engine Thread                                |
|                                                                         |
|  +-------------------------------------------------------------------+  |
|  |                        Bevy App                                   |  |
|  |  +---------------+  +---------------+  +-------------------+      |  |
|  |  | Window Mgmt   |  |  Rendering    |  | Command Receiver  |      |  |
|  |  |   (winit)     |  |    (wgpu)     |  |    System         |      |  |
|  |  +---------------+  +---------------+  +-------------------+      |  |
|  |                                        +-------------------+      |  |
|  |                                        | Event Sender      |      |  |
|  |                                        |    System         |      |  |
|  |                                        +-------------------+      |  |
|  +-------------------------------------------------------------------+  |
|                                                                         |
+-------------------------------------------------------------------------+

Bidirectional Communication:
- Commands: Main Thread -> Engine Thread (window ops, shutdown, etc.)
- Events: Engine Thread -> Main Thread (input, window events, frame ticks)
```

## Shutdown Sequence

```
+------------------+     +------------------+     +------------------+
|   Main Thread    |     |  GraphicProxy    |     |  Engine Thread   |
+--------+---------+     +--------+---------+     +--------+---------+
         |                        |                        |
         | 1. Client exit         |                        |
         |----------------------->|                        |
         |                        | 2. Send Shutdown cmd   |
         |                        |----------------------->|
         |                        |                        | 3. Stop accepting
         |                        |                        |    new commands
         |                        |                        |
         |                        |                        | 4. Close windows
         |                        |                        |
         |                        |                        | 5. Release resources
         |                        |                        |
         |                        | 6. Shutdown response   |
         |                        |<-----------------------|
         |                        |                        | 7. Exit loop
         |                        |                        |
         | 8. Thread join         |                        |
         |<-----------------------|                        X
         |                        |
         | 9. Continue exit       |
         X                        X

Timeout handling:
- If step 6 does not arrive within timeout (e.g., 5 seconds)
- Main thread logs warning and continues exit
- Engine thread may be forcibly terminated
```

## Implementation Steps

### Phase 1: Core Infrastructure

1. Create `graphic` module in `stam_mod_runtimes/src/api/`
2. Implement `GraphicEngines` enum
3. Implement `GraphicProxy` with command/response channels
4. Implement `GraphicEngine` trait
5. Add JavaScript bindings for `system.enable_graphic_engine()`

### Phase 2: Bevy Integration

1. Add `bevy` dependency to `stam_client/Cargo.toml`
2. Implement `BevyEngine` struct
3. Implement Bevy command receiver system
4. Test engine initialization in separate thread

### Phase 3: Window Management

1. Implement `WindowConfig` and `WindowInfo`
2. Add window-related `GraphicCommand` variants
3. Implement JavaScript `window` object and `WindowJS` class
4. Implement Bevy window creation/management

### Phase 4: Testing & Polish

1. Create test mod that creates a window
2. Test window property changes
3. Test multiple windows
4. Test engine shutdown and cleanup

### Phase 5: Additional Language Bindings (Future)

1. Implement Lua bindings when Lua runtime is added
2. Implement C# bindings when C# runtime is added
3. Ensure API consistency across all languages

## Error Handling

All graphic operations should return descriptive errors:

```javascript
try {
  await system.enable_graphic_engine(GraphicEngines.Bevy);
} catch (e) {
  console.error("Failed to enable graphics:", e.message);
  // e.g., "Graphic engine already enabled" or "Failed to spawn engine thread"
}

try {
  const win = await window.create({ title: "Test" });
} catch (e) {
  console.error("Failed to create window:", e.message);
  // e.g., "No graphic engine enabled" or "Window creation failed: ..."
}
```

## UI Elements and User Interaction Events (Draft)

This section describes how scripts can create UI elements (buttons, text fields, etc.) and receive user interaction events from them.

### Design Goals

1. **Secure**: No direct memory access between threads, all communication via channels
2. **Efficient**: Minimize channel overhead, batch events when possible
3. **Performant**: Low latency for user interactions (< 16ms for 60fps responsiveness)
4. **Simple API**: Scripts create elements and register callbacks naturally

### Element ID System

Every UI element created by a script receives a unique ID. This ID is used to:

- Track the element in the engine
- Route events back to the correct script callback
- Allow scripts to modify or remove elements

```rust
/// Unique identifier for UI elements
/// Format: window_id (32 bits) | element_counter (32 bits)
pub type ElementId = u64;

/// Generate a unique element ID within a window
fn generate_element_id(window_id: u64, counter: &AtomicU32) -> ElementId {
    let element_num = counter.fetch_add(1, Ordering::SeqCst);
    (window_id << 32) | (element_num as u64)
}
```

### UI Element Types

```rust
/// Types of UI elements that can be created
pub enum UiElementType {
    Button,
    Label,
    TextInput,
    Checkbox,
    Slider,
    Image,
    Panel,
    // ... more as needed
}

/// Configuration for creating a UI element
pub struct UiElementConfig {
    pub element_type: UiElementType,
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub text: Option<String>,
    pub style: Option<UiStyle>,
}
```

### UI Events from Engine to Scripts

When a user interacts with a UI element, the engine generates a `UiEvent`:

```rust
/// Events generated by UI element interactions
pub enum UiEvent {
    /// Button was clicked
    ButtonClicked {
        window_id: u64,
        element_id: ElementId,
    },

    /// Text input value changed
    TextChanged {
        window_id: u64,
        element_id: ElementId,
        new_value: String,
    },

    /// Text input submitted (Enter pressed)
    TextSubmitted {
        window_id: u64,
        element_id: ElementId,
        value: String,
    },

    /// Checkbox toggled
    CheckboxToggled {
        window_id: u64,
        element_id: ElementId,
        checked: bool,
    },

    /// Slider value changed
    SliderChanged {
        window_id: u64,
        element_id: ElementId,
        value: f32,
    },

    /// Element gained focus
    FocusGained {
        window_id: u64,
        element_id: ElementId,
    },

    /// Element lost focus
    FocusLost {
        window_id: u64,
        element_id: ElementId,
    },

    /// Mouse entered element bounds
    MouseEnter {
        window_id: u64,
        element_id: ElementId,
    },

    /// Mouse left element bounds
    MouseLeave {
        window_id: u64,
        element_id: ElementId,
    },
}
```

### Event Routing Architecture

The challenge: scripts register callbacks for specific elements, but events come from the engine thread. We need an efficient way to route events to the correct callbacks.

**Recommended Approach: Callback Registry with Element-Specific Handlers**

```
Script creates button          GraphicProxy              Engine Thread
        |                           |                         |
        | 1. createButton(config)   |                         |
        |-------------------------->|                         |
        |                           | 2. CreateElement cmd    |
        |                           |------------------------>|
        |                           |                         | 3. Create button
        |                           | 4. ElementCreated       |    in Bevy UI
        |                           |<------------------------|
        | 5. Returns ElementId      |                         |
        |<--------------------------|                         |
        |                           |                         |
        | 6. onClick(elementId, fn) |                         |
        |-------------------------->|                         |
        | (registers callback       |                         |
        |  in local registry)       |                         |
        |                           |                         |
        |                           |                         | User clicks button
        |                           | 7. UiEvent::ButtonClicked
        |                           |<------------------------|
        |                           |                         |
        | 8. Dispatch to callback   |                         |
        |<--------------------------|                         |
        | (lookup by elementId)     |                         |
```

### Callback Registry (Script-Side)

Each scripting runtime maintains a local registry of callbacks:

```rust
/// Registry for UI element callbacks (per-runtime)
pub struct UiCallbackRegistry {
    /// Map: ElementId -> list of (event_type, callback)
    callbacks: HashMap<ElementId, Vec<(UiEventType, CallbackHandle)>>,
}

impl UiCallbackRegistry {
    /// Register a callback for an element event
    pub fn register(&mut self, element_id: ElementId, event_type: UiEventType, callback: CallbackHandle) {
        self.callbacks
            .entry(element_id)
            .or_default()
            .push((event_type, callback));
    }

    /// Unregister all callbacks for an element (when element is destroyed)
    pub fn unregister_element(&mut self, element_id: ElementId) {
        self.callbacks.remove(&element_id);
    }

    /// Get callbacks for a specific element and event type
    pub fn get_callbacks(&self, element_id: ElementId, event_type: UiEventType) -> Vec<&CallbackHandle> {
        self.callbacks
            .get(&element_id)
            .map(|cbs| cbs.iter()
                .filter(|(et, _)| *et == event_type)
                .map(|(_, cb)| cb)
                .collect())
            .unwrap_or_default()
    }
}
```

### JavaScript API for UI Elements

```javascript
// Create a window
const win = await window.create({ title: "My App", width: 800, height: 600 });

// Create a button
const btn = await win.createButton({
  x: 100,
  y: 100,
  width: 200,
  height: 50,
  text: "Click Me!",
});

// Register click handler - callback is stored locally, not sent to engine
btn.onClick(async () => {
  console.log("Button clicked!");
  await btn.setText("Clicked!");
});

// Alternative: inline event registration during creation
const btn2 = await win.createButton({
  x: 100,
  y: 200,
  width: 200,
  height: 50,
  text: "Another Button",
  onClick: async () => {
    console.log("Button 2 clicked!");
  },
});

// Create a text input
const input = await win.createTextInput({
  x: 100,
  y: 300,
  width: 300,
  height: 40,
  placeholder: "Enter your name...",
});

// Register text change handler
input.onTextChanged(async (newValue) => {
  console.log(`Text changed to: ${newValue}`);
});

// Register submit handler (Enter key)
input.onSubmit(async (value) => {
  console.log(`Submitted: ${value}`);
});

// Create a checkbox
const checkbox = await win.createCheckbox({
  x: 100,
  y: 400,
  text: "Enable feature",
  checked: false,
});

checkbox.onToggle(async (checked) => {
  console.log(`Checkbox is now: ${checked ? "checked" : "unchecked"}`);
});

// Remove an element
await btn.remove();
// This also automatically unregisters all callbacks for btn
```

### Event Batching for Performance

To minimize overhead, the engine should batch UI events:

```rust
/// Batched UI events sent from engine to main thread
pub struct UiEventBatch {
    /// Timestamp when batch was created
    pub timestamp: f64,
    /// All events in this batch
    pub events: Vec<UiEvent>,
}

// In the engine thread (Bevy system):
fn collect_ui_events(
    mut event_writer: EventWriter<UiEventBatch>,
    // ... UI event sources
) {
    let mut batch = UiEventBatch {
        timestamp: time.elapsed_seconds_f64(),
        events: Vec::new(),
    };

    // Collect all UI events that occurred this frame
    for event in button_events.iter() {
        batch.events.push(UiEvent::ButtonClicked {
            window_id: event.window_id,
            element_id: event.element_id,
        });
    }
    // ... collect other event types

    // Only send if there are events
    if !batch.events.is_empty() {
        event_tx.send(batch).ok();
    }
}
```

### Event Dispatch in Main Thread

```rust
impl GraphicProxy {
    /// Process UI events and dispatch to script callbacks
    pub async fn poll_ui_events(&self, callback_registry: &UiCallbackRegistry) {
        let mut rx_guard = self.ui_event_rx.lock().await;
        if let Some(rx) = rx_guard.as_mut() {
            while let Ok(batch) = rx.try_recv() {
                for event in batch.events {
                    self.dispatch_ui_event(event, callback_registry).await;
                }
            }
        }
    }

    async fn dispatch_ui_event(&self, event: UiEvent, registry: &UiCallbackRegistry) {
        let (element_id, event_type) = match &event {
            UiEvent::ButtonClicked { element_id, .. } => (*element_id, UiEventType::Click),
            UiEvent::TextChanged { element_id, .. } => (*element_id, UiEventType::TextChange),
            // ... etc
        };

        // Get callbacks registered for this element and event type
        let callbacks = registry.get_callbacks(element_id, event_type);

        // Execute each callback
        for callback in callbacks {
            callback.invoke(event.to_args()).await;
        }
    }
}
```

### Event Subscription Model (Lazy Event Emission)

**Critical Optimization**: The GraphicEngine MUST NOT emit events that no one is listening to.

When a script creates a UI element (e.g., a button) but does not register any event handler
(e.g., `onClick`), the engine should NOT send click events for that element. This prevents
unnecessary channel traffic and processing overhead.

**How it works**:

1. **Subscription Registry**: The GraphicProxy maintains a registry of which events are
   subscribed for each element.

2. **Subscription Notification**: When a script registers a callback (e.g., `btn.onClick(...)`),
   the GraphicProxy sends a `SubscribeEvent` command to the engine.

3. **Conditional Emission**: The engine only emits events for elements/event-types that have
   at least one subscriber.

4. **Unsubscription**: When all callbacks for an event type are removed, a `UnsubscribeEvent`
   command is sent to stop emissions.

```rust
/// Event subscription management
pub enum GraphicCommand {
    // ... existing commands ...

    /// Subscribe to events for an element
    SubscribeEvent {
        element_id: ElementId,
        event_type: UiEventType,
    },

    /// Unsubscribe from events for an element
    UnsubscribeEvent {
        element_id: ElementId,
        event_type: UiEventType,
    },
}

/// In the GraphicProxy (main thread side)
pub struct EventSubscriptionRegistry {
    /// Map: ElementId -> Set of subscribed event types
    subscriptions: HashMap<ElementId, HashSet<UiEventType>>,
}

impl EventSubscriptionRegistry {
    /// Register a subscription and notify engine if this is the first subscriber
    pub fn subscribe(&mut self, element_id: ElementId, event_type: UiEventType, command_tx: &mpsc::Sender<GraphicCommand>) {
        let entry = self.subscriptions.entry(element_id).or_default();
        let is_new = entry.insert(event_type);

        if is_new {
            // First subscriber for this element/event - notify engine
            let _ = command_tx.try_send(GraphicCommand::SubscribeEvent {
                element_id,
                event_type,
            });
        }
    }

    /// Unregister a subscription and notify engine if no more subscribers
    pub fn unsubscribe(&mut self, element_id: ElementId, event_type: UiEventType, command_tx: &mpsc::Sender<GraphicCommand>) {
        if let Some(entry) = self.subscriptions.get_mut(&element_id) {
            entry.remove(&event_type);

            if !entry.contains(&event_type) {
                // No more subscribers - notify engine to stop emitting
                let _ = command_tx.try_send(GraphicCommand::UnsubscribeEvent {
                    element_id,
                    event_type,
                });
            }
        }
    }
}
```

**In the Engine Thread (Bevy side)**:

```rust
/// Resource tracking which events to emit
#[derive(Resource, Default)]
pub struct EventSubscriptions {
    /// Map: ElementId -> Set of subscribed event types
    subscriptions: HashMap<ElementId, HashSet<UiEventType>>,
}

impl EventSubscriptions {
    pub fn is_subscribed(&self, element_id: ElementId, event_type: UiEventType) -> bool {
        self.subscriptions
            .get(&element_id)
            .map(|s| s.contains(&event_type))
            .unwrap_or(false)
    }
}

/// System that processes button clicks - only emits if subscribed
fn handle_button_interactions(
    subscriptions: Res<EventSubscriptions>,
    interaction_query: Query<(Entity, &Interaction, &UiElementId), Changed<Interaction>>,
    event_tx: Res<EventSender>,
) {
    for (entity, interaction, ui_id) in interaction_query.iter() {
        if *interaction == Interaction::Pressed {
            // ONLY emit if someone is listening
            if subscriptions.is_subscribed(ui_id.0, UiEventType::Click) {
                event_tx.send(UiEvent::ButtonClicked {
                    window_id: ui_id.window_id(),
                    element_id: ui_id.0,
                });
            }
        }
    }
}
```

**Benefits**:

1. **Reduced Channel Traffic**: No events sent for unsubscribed elements
2. **Lower CPU Usage**: Engine skips event processing for unsubscribed elements
3. **Scalability**: UI with many elements but few handlers remains efficient
4. **Memory Efficiency**: No event objects created for unsubscribed events

**Example Flow**:

```
Script                      GraphicProxy                    Engine
  |                              |                            |
  | createButton()               |                            |
  |----------------------------->| CreateElement              |
  |                              |--------------------------->|
  |                              |                            | (button created,
  |                              |                            |  no subscriptions)
  |                              |                            |
  |                              |                            | User clicks button
  |                              |                            | -> NO EVENT SENT
  |                              |                            |    (no subscribers)
  |                              |                            |
  | btn.onClick(handler)         |                            |
  |----------------------------->| SubscribeEvent(Click)      |
  |                              |--------------------------->|
  |                              |                            | (now subscribed)
  |                              |                            |
  |                              |                            | User clicks button
  |                              | UiEvent::ButtonClicked     |
  |                              |<---------------------------|
  | handler() called             |                            |
  |<-----------------------------|                            |
```

### Performance Considerations

1. **Callback Storage**: Callbacks are stored in the scripting runtime, NOT sent to the engine.
   This avoids serialization overhead and keeps the channel payload small.

2. **Element ID Lookup**: Use HashMap for O(1) callback lookup by ElementId.

3. **Event Batching**: Group all UI events per frame into a single channel message.

4. **Non-blocking Dispatch**: Event dispatch should not block the main loop.
   If a callback is slow, consider queueing it for async execution.

5. **Memory Cleanup**: When an element is removed, its callbacks must be unregistered
   to prevent memory leaks.

6. **Lazy Event Emission**: Events are only sent from engine to main thread if at least
   one callback is registered for that element/event-type combination.

### Security Considerations

1. **ElementId Validation**: Before dispatching an event, verify the ElementId
   belongs to the requesting script's window. Prevent cross-mod event injection.

2. **Callback Isolation**: Each mod's callbacks run in its own scripting context.
   One mod cannot access another mod's callbacks.

3. **Rate Limiting**: Consider rate-limiting high-frequency events (like MouseMove)
   to prevent script overload.

## Input Handling and Frame Loop Optimization

This section describes how scripts can efficiently handle high-frequency input (mouse position,
keyboard state, gamepad) during the game loop, with automatic optimization.

### Design Principles

1. **Single API**: Scripts use the same methods everywhere (e.g., `graphic.getMousePosition()`)
2. **Automatic Optimization**: The system detects execution context and chooses the fast path when possible
3. **Zero Allocation in Hot Path**: Pre-allocated structures are reused every frame
4. **Snapshot Consistency**: All input data within a frame callback represents the same instant

### Frame Update Callback

Scripts register a frame update callback that receives only the delta time:

```javascript
// Register frame update handler
graphic.onFrameUpdate((delta) => {
  // All graphic.* methods are automatically optimized here
  const mouse = graphic.getMousePosition(); // Uses cached snapshot (O(1))
  const keys = graphic.getPressedKeys(); // Uses cached snapshot (O(1))
  const gamepads = graphic.getGamepadState(); // Uses cached snapshot (O(1))

  // Game logic
  player.x += velocity.x * delta;
  player.y += velocity.y * delta;
});

// Outside frame loop, same API works but slower
document.onClick(() => {
  const mouse = graphic.getMousePosition(); // Sync request to engine (round-trip)
});
```

### Context-Aware Method Dispatch

Every `graphic.*` method automatically detects its execution context:

```
graphic.getMousePosition() called
              |
              v
    +---------------------+
    | In frame callback?  |
    +---------------------+
         |           |
        YES          NO
         |           |
         v           v
    +-----------+  +-------------------+
    | Return    |  | Sync request to   |
    | cached    |  | GraphicEngine     |
    | snapshot  |  | (channel round-   |
    | (O(1))    |  | trip)             |
    +-----------+  +-------------------+
```

### Frame Snapshot Structure

The GraphicProxy maintains a pre-allocated snapshot that is updated once per frame:

```rust
/// Pre-allocated snapshot of input state for the current frame
/// Allocated once, reused every frame to avoid allocations in hot path
pub struct FrameSnapshot {
    /// Time since last frame in seconds
    pub delta: f64,

    /// Frame number (monotonically increasing)
    pub frame_number: u64,

    /// Mouse position in window coordinates
    pub mouse_position: (f32, f32),

    /// Mouse button states
    pub mouse_buttons: MouseButtonState,

    /// Currently pressed keyboard keys
    /// Vec is reused - cleared and refilled each frame
    pub pressed_keys: Vec<KeyCode>,

    /// Gamepad states (up to 4 gamepads)
    /// Fixed-size array, no allocation
    pub gamepads: [GamepadState; 4],

    /// Number of connected gamepads
    pub gamepad_count: u8,
}

impl FrameSnapshot {
    /// Create a new snapshot with pre-allocated capacity
    pub fn new() -> Self {
        Self {
            delta: 0.0,
            frame_number: 0,
            mouse_position: (0.0, 0.0),
            mouse_buttons: MouseButtonState::default(),
            pressed_keys: Vec::with_capacity(16), // Pre-allocate for typical use
            gamepads: [GamepadState::default(); 4],
            gamepad_count: 0,
        }
    }

    /// Update snapshot in-place from engine data (no allocation)
    pub fn update_from(&mut self, data: &FrameData) {
        self.delta = data.delta;
        self.frame_number = data.frame_number;
        self.mouse_position = data.mouse_position;
        self.mouse_buttons = data.mouse_buttons;

        // Reuse Vec capacity - clear and extend
        self.pressed_keys.clear();
        self.pressed_keys.extend_from_slice(&data.pressed_keys);

        // Copy gamepad data
        self.gamepads = data.gamepads;
        self.gamepad_count = data.gamepad_count;
    }
}

#[derive(Clone, Copy, Default)]
pub struct MouseButtonState {
    pub left: bool,
    pub right: bool,
    pub middle: bool,
}

#[derive(Clone, Copy, Default)]
pub struct GamepadState {
    pub connected: bool,
    pub left_stick: (f32, f32),   // -1.0 to 1.0
    pub right_stick: (f32, f32),  // -1.0 to 1.0
    pub left_trigger: f32,        // 0.0 to 1.0
    pub right_trigger: f32,       // 0.0 to 1.0
    pub buttons: u32,             // Bitmask of pressed buttons
}
```

### GraphicProxy Implementation

```rust
use std::cell::{Cell, RefCell};

pub struct GraphicProxy {
    // ... existing fields ...

    /// Pre-allocated frame snapshot, reused every frame
    frame_snapshot: RefCell<FrameSnapshot>,

    /// Flag indicating we're inside a frame callback
    in_frame_callback: Cell<bool>,

    /// Channel to send commands to engine
    command_tx: mpsc::Sender<GraphicCommand>,

    /// Channel to receive responses from engine
    response_rx: mpsc::Receiver<GraphicResponse>,
}

impl GraphicProxy {
    pub fn new(command_tx: mpsc::Sender<GraphicCommand>,
               response_rx: mpsc::Receiver<GraphicResponse>) -> Self {
        Self {
            frame_snapshot: RefCell::new(FrameSnapshot::new()),
            in_frame_callback: Cell::new(false),
            command_tx,
            response_rx,
        }
    }

    /// Called by the main loop when engine sends frame data
    pub fn process_frame(&self, frame_data: FrameData, callbacks: &[FrameCallback]) {
        // 1. Update snapshot in-place (no allocation)
        self.frame_snapshot.borrow_mut().update_from(&frame_data);

        // 2. Set context flag
        self.in_frame_callback.set(true);

        // 3. Call all registered frame callbacks with just delta
        for callback in callbacks {
            callback.invoke(frame_data.delta);
        }

        // 4. Clear context flag
        self.in_frame_callback.set(false);
    }

    /// Get mouse position - automatically optimized based on context
    pub fn get_mouse_position(&self) -> (f32, f32) {
        if self.in_frame_callback.get() {
            // Fast path: use cached snapshot
            self.frame_snapshot.borrow().mouse_position
        } else {
            // Slow path: sync request to engine
            self.sync_request(GraphicCommand::GetMousePosition)
                .map(|r| r.into_mouse_position())
                .unwrap_or((0.0, 0.0))
        }
    }

    /// Get pressed keys - automatically optimized based on context
    pub fn get_pressed_keys(&self) -> Vec<KeyCode> {
        if self.in_frame_callback.get() {
            // Fast path: clone from snapshot (small vec, usually < 5 keys)
            self.frame_snapshot.borrow().pressed_keys.clone()
        } else {
            // Slow path: sync request to engine
            self.sync_request(GraphicCommand::GetPressedKeys)
                .map(|r| r.into_pressed_keys())
                .unwrap_or_default()
        }
    }

    /// Check if a specific key is pressed - optimized
    pub fn is_key_pressed(&self, key: KeyCode) -> bool {
        if self.in_frame_callback.get() {
            // Fast path: check snapshot
            self.frame_snapshot.borrow().pressed_keys.contains(&key)
        } else {
            // Slow path: sync request
            self.sync_request(GraphicCommand::IsKeyPressed(key))
                .map(|r| r.into_bool())
                .unwrap_or(false)
        }
    }

    /// Get gamepad state - automatically optimized based on context
    pub fn get_gamepad_state(&self, index: u8) -> Option<GamepadState> {
        if self.in_frame_callback.get() {
            // Fast path: use snapshot
            let snapshot = self.frame_snapshot.borrow();
            if index < snapshot.gamepad_count {
                Some(snapshot.gamepads[index as usize])
            } else {
                None
            }
        } else {
            // Slow path: sync request
            self.sync_request(GraphicCommand::GetGamepadState(index))
                .and_then(|r| r.into_gamepad_state())
        }
    }

    /// Synchronous request to engine (used outside frame loop)
    fn sync_request(&self, cmd: GraphicCommand) -> Option<GraphicResponse> {
        self.command_tx.blocking_send(cmd).ok()?;
        self.response_rx.blocking_recv().ok()
    }
}
```

### JavaScript Bindings

```javascript
// All these methods work the same way everywhere
// but are automatically optimized in the frame loop

graphic.onFrameUpdate((delta) => {
  // === OPTIMIZED (cached snapshot) ===

  // Mouse
  const [mx, my] = graphic.getMousePosition();
  const mouseButtons = graphic.getMouseButtons();
  // mouseButtons = { left: bool, right: bool, middle: bool }

  // Keyboard
  const keys = graphic.getPressedKeys(); // Array of key codes
  if (graphic.isKeyPressed("KeyW")) {
    player.moveForward(delta);
  }
  if (graphic.isKeyPressed("Space")) {
    player.jump();
  }

  // Gamepad
  const gamepad = graphic.getGamepad(0);
  if (gamepad) {
    const [lx, ly] = gamepad.leftStick; // -1.0 to 1.0
    const [rx, ry] = gamepad.rightStick; // -1.0 to 1.0
    player.move(lx * speed * delta, ly * speed * delta);
    camera.rotate(rx * sensitivity * delta, ry * sensitivity * delta);

    if (gamepad.isButtonPressed("A")) {
      player.jump();
    }
  }

  // Frame info
  const frameNum = graphic.getFrameNumber();
});
```

### Lua Bindings

```lua
graphic.onFrameUpdate(function(delta)
    -- Same optimizations apply in Lua

    local mx, my = graphic.getMousePosition()

    if graphic.isKeyPressed("W") then
        player:moveForward(delta)
    end

    local gamepad = graphic.getGamepad(0)
    if gamepad then
        local lx, ly = gamepad:getLeftStick()
        player:move(lx * speed * delta, ly * speed * delta)
    end
end)
```

### C# Bindings

```csharp
Graphic.OnFrameUpdate((delta) => {
    // Same optimizations apply in C#

    var (mx, my) = Graphic.GetMousePosition();

    if (Graphic.IsKeyPressed(KeyCode.W)) {
        player.MoveForward(delta);
    }

    var gamepad = Graphic.GetGamepad(0);
    if (gamepad != null) {
        var (lx, ly) = gamepad.LeftStick;
        player.Move(lx * speed * delta, ly * speed * delta);
    }
});
```

### Data Flow

```
Engine Thread                          Main Thread (GraphicProxy)
     |                                        |
     | [Every frame]                          |
     |                                        |
     | 1. Collect input state                 |
     |    - Mouse position                    |
     |    - Keyboard state                    |
     |    - Gamepad state                     |
     |                                        |
     | 2. Send FrameData via channel          |
     |--------------------------------------->|
     |                                        | 3. Update FrameSnapshot
     |                                        |    in-place (no alloc)
     |                                        |
     |                                        | 4. Set in_frame_callback = true
     |                                        |
     |                                        | 5. Call script callbacks
     |                                        |    with delta
     |                                        |    |
     |                                        |    | script calls
     |                                        |    | graphic.getMousePosition()
     |                                        |    |
     |                                        |    +-> Returns cached value
     |                                        |        (no channel access)
     |                                        |
     |                                        | 6. Set in_frame_callback = false
     |                                        |
     | [Continue to next frame]               |
```

### Important Behaviors

1. **Snapshot Consistency**: All `graphic.*` calls within the same frame callback
   return data from the same instant. Calling `getMousePosition()` twice gives
   the same value (the snapshot), not the "live" position.

2. **Capacity Stabilization**: The `pressed_keys` Vec will stabilize its capacity
   after a few frames (typically 8-16 keys max), eliminating allocations.

3. **Outside Frame Loop**: Methods still work outside `onFrameUpdate`, but incur
   a channel round-trip. This is fine for infrequent operations (e.g., menu clicks).

4. **Thread Safety**: `RefCell` is safe here because GraphicProxy is single-threaded
   on the script side. The `in_frame_callback` flag ensures no concurrent access.

### Performance Characteristics

| Context                | Method Call Cost | Notes                |
| ---------------------- | ---------------- | -------------------- |
| Inside `onFrameUpdate` | O(1), ~10ns      | Direct memory access |
| Outside frame loop     | ~1-10ms          | Channel round-trip   |

### Memory Layout

```
GraphicProxy
+---------------------------+
| frame_snapshot: RefCell   |
|   +---------------------+ |
|   | delta: f64          | |  8 bytes
|   | frame_number: u64   | |  8 bytes
|   | mouse_position: 2xf32| | 8 bytes
|   | mouse_buttons: 3xu8 | |  3 bytes (+ padding)
|   | pressed_keys: Vec   | |  24 bytes (ptr+len+cap) + heap
|   | gamepads: [4]       | |  4 * ~48 bytes = 192 bytes
|   | gamepad_count: u8   | |  1 byte
|   +---------------------+ |
|                           |
| in_frame_callback: Cell   |  1 byte (+ padding)
+---------------------------+

Total stack: ~256 bytes (fixed)
Heap: pressed_keys buffer (~64 bytes typical)
```

## Future Extensions

This architecture supports future additions:

1. **More engines**: WGPU for custom rendering, Terminal for TUI
2. **More languages**: Lua, C#, and others - all sharing the same GraphicProxy
3. **Rendering primitives**: Sprites, meshes, cameras, lights
4. **Input handling**: Keyboard, mouse, gamepad events from the graphic window
5. **Audio**: Could use similar proxy pattern for audio engines
6. **Scene management**: ECS-like API for managing game objects
7. **UI Layouts**: Flexbox-like layout system for automatic element positioning
8. **UI Themes**: Centralized styling system for consistent UI appearance

## Implementation Phases

### Phase 1: Core Window System (Current Priority)

The initial implementation should focus on:

1. **Window Creation and Management**

   - `graphic.createWindow({ title, width, height, ... })`
   - `window.close()`
   - `window.setTitle(title)`
   - `window.setSize(width, height)`
   - `window.getSize()`
   - `window.setPosition(x, y)`
   - `window.getPosition()`
   - `window.setFullscreen(enabled)`
   - `window.isFullscreen()`

2. **Basic Window Properties**

   - Title, size, position
   - Fullscreen toggle
   - Visibility (show/hide)
   - Focus management

3. **Frame Loop with Input**
   - `graphic.onFrameUpdate((delta) => { ... })`
   - `graphic.getMousePosition()`
   - `graphic.getMouseButtons()`
   - `graphic.isKeyPressed(key)`
   - `graphic.getPressedKeys()`
   - `graphic.getGamepad(index)`

This provides the foundation for any graphical application: a window that can receive
input and update every frame.

### Phase 2: UI Widgets (Future)

After Phase 1 is complete and stable, the next phase will implement UI widgets:

- Buttons, Labels, Text Inputs
- Checkboxes, Sliders, Progress Bars
- Panels, Layouts, Themes

These are documented in the "UI Elements and User Interaction Events" section above
but will be implemented in a subsequent phase.

## Notes for Implementation

1. **Thread Safety**: All communication between threads uses `tokio::sync` primitives
2. **Async/Await**: Window operations return Promises/Futures that resolve when the engine confirms completion
3. **Cleanup**: Engine shutdown must be graceful - close all windows, wait for thread join
4. **No Busy Loops**: Use proper async patterns as per Golden Rules in CLAUDE.md
5. **Client-Only**: This entire system is client-only; server should throw appropriate errors
6. **Language-Agnostic Core**: `GraphicProxy` and `GraphicEngine` know nothing about the calling language
7. **Consistent API**: All language bindings should expose the same capabilities with language-appropriate syntax
