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

### 2. Architecture Layers

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
    visible: true
});

// Window methods
await win.setSize(800, 600);
await win.setTitle("New Title");
await win.setFullscreen(true);
await win.setVisible(false);
await win.close();

// Window properties (read-only)
console.log(win.id);        // Unique window ID
console.log(win.title);     // Current title
console.log(win.width);     // Current width
console.log(win.height);    // Current height
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
|  +-----------------------------------+-------------------------------+  |
|                                      |                                  |
+--------------------------------------+----------------------------------+
                                       |
                             mpsc channel (commands)
                                       |
                                       v
+-------------------------------------------------------------------------+
|                    Graphic Engine Thread                                |
|                                                                         |
|  +-------------------------------------------------------------------+  |
|  |                        Bevy App                                   |  |
|  |  +---------------+  +---------------+  +-------------------+      |  |
|  |  | Window Mgmt   |  |  Rendering    |  | Command Receiver  |      |  |
|  |  |   (winit)     |  |    (wgpu)     |  |    System         |      |  |
|  |  +---------------+  +---------------+  +-------------------+      |  |
|  +-------------------------------------------------------------------+  |
|                                                                         |
+-------------------------------------------------------------------------+
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

## Future Extensions

This architecture supports future additions:

1. **More engines**: WGPU for custom rendering, Terminal for TUI
2. **More languages**: Lua, C#, and others - all sharing the same GraphicProxy
3. **Rendering primitives**: Sprites, meshes, cameras, lights
4. **Input handling**: Keyboard, mouse, gamepad events from the graphic window
5. **Audio**: Could use similar proxy pattern for audio engines
6. **Scene management**: ECS-like API for managing game objects

## Notes for Implementation

1. **Thread Safety**: All communication between threads uses `tokio::sync` primitives
2. **Async/Await**: Window operations return Promises/Futures that resolve when the engine confirms completion
3. **Cleanup**: Engine shutdown must be graceful - close all windows, wait for thread join
4. **No Busy Loops**: Use proper async patterns as per Golden Rules in CLAUDE.md
5. **Client-Only**: This entire system is client-only; server should throw appropriate errors
6. **Language-Agnostic Core**: `GraphicProxy` and `GraphicEngine` know nothing about the calling language
7. **Consistent API**: All language bindings should expose the same capabilities with language-appropriate syntax
