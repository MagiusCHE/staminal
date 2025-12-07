# Graphic Engine: Windows and ECS UI

This document describes the architecture of the Staminal graphic system, focusing on window management and ECS-based UI creation.

## Overview

The graphic system provides a **language-agnostic API** for creating windows and UI elements using the **ECS (Entity Component System)** paradigm. All operations are **client-only** - on the server, all graphic methods return descriptive errors.

> **Note**: The legacy widget system (`window.createWidget()`, `WidgetTypes`, etc.) has been removed. Use the ECS API (`World.spawn()`, component types) instead. See [ecs.md](ecs.md) for the complete ECS API documentation.

## Architecture

### Threading Model

```
┌─────────────────────────────────────────────────────┐
│              Main Thread (Bevy)                     │
│  ┌──────────────────────────────────────────────┐  │
│  │ BevyEngine                                    │  │
│  │  • Window management (winit)                  │  │
│  │  • UI rendering (bevy_ui)                     │  │
│  │  • ECS entity creation/updates                │  │
│  └──────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────┘
           ↕ channels (std::sync::mpsc / tokio::sync::mpsc)
┌─────────────────────────────────────────────────────┐
│            Worker Thread (tokio runtime)            │
│  ┌──────────────────────────────────────────────┐  │
│  │ GraphicProxy                                  │  │
│  │  • Routes commands to engine                  │  │
│  │  • Receives events from engine                │  │
│  │  • Manages window registries                  │  │
│  │  • Shared by ALL runtime adapters             │  │
│  └──────────────────────────────────────────────┘  │
│  ┌──────────────────────────────────────────────┐  │
│  │ Runtime Adapters                              │  │
│  │  ├── JavaScript (QuickJS) ← current           │  │
│  │  ├── Lua (mlua) ← future                      │  │
│  │  └── C#, Rust, C++ ← future                   │  │
│  └──────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────┘
```

### Key Files

| File | Description |
|------|-------------|
| [proxy.rs](../../apps/shared/stam_mod_runtimes/src/api/graphic/proxy.rs) | `GraphicProxy` - Central coordinator |
| [commands.rs](../../apps/shared/stam_mod_runtimes/src/api/graphic/commands.rs) | Command definitions (worker → engine) |
| [events.rs](../../apps/shared/stam_mod_runtimes/src/api/graphic/events.rs) | Event definitions (engine → worker) |
| [window.rs](../../apps/shared/stam_mod_runtimes/src/api/graphic/window.rs) | Window configuration types |
| [common_types.rs](../../apps/shared/stam_mod_runtimes/src/api/graphic/common_types.rs) | Shared types (colors, sizes, layout) |
| [ecs.rs](../../apps/shared/stam_mod_runtimes/src/api/graphic/ecs.rs) | ECS command and query types |
| [bevy.rs](../../apps/stam_client/src/engines/bevy.rs) | Bevy engine implementation |

## GraphicProxy

The `GraphicProxy` is the central API for all graphic operations. It's created once per runtime and shared across all mods.

### Initialization

```rust
// Client: Create with enable request channel
let proxy = GraphicProxy::new_client(enable_request_tx, Some(asset_root));

// Server: Create stub (all operations return errors)
let proxy = GraphicProxy::new_server_stub();
```

### State Management

GraphicProxy maintains:

- `active_engine`: Currently enabled engine type
- `command_tx`: Channel to send commands to engine
- `event_rx`: Channel to receive events from engine
- `windows`: Registry of window IDs → `WindowInfo`
- `loaded_fonts`: Loaded font aliases
- `next_window_id`: Atomic counter for window IDs

## Window Management

### Window Lifecycle

1. **Enable Engine** → Creates main window (ID 1)
2. **Create Window** → Creates additional windows (ID 2+)
3. **Modify Window** → Set title, size, fullscreen, visibility
4. **Close Window** → Destroys window and all its UI entities

### Window Configuration

```rust
pub struct WindowConfig {
    pub title: String,          // Window title
    pub width: u32,             // Width in pixels
    pub height: u32,            // Height in pixels
    pub fullscreen: bool,       // Fullscreen mode
    pub resizable: bool,        // Allow resize (set at creation only)
    pub visible: bool,          // Visibility
    pub position_mode: WindowPositionMode,  // Positioning
}

pub enum WindowPositionMode {
    Default,           // OS default
    Centered,          // Center on screen
    At(i32, i32),      // Specific coordinates
}
```

### Window Commands

| Command | Description |
|---------|-------------|
| `CreateWindow` | Create a new window |
| `CloseWindow` | Close and destroy a window |
| `SetWindowSize` | Update window dimensions |
| `SetWindowTitle` | Update window title |
| `SetWindowMode` | Set window mode (Windowed, Fullscreen, BorderlessFullscreen) |
| `SetWindowVisible` | Show/hide window |
| `SetWindowFont` | Set default font for window |

### JavaScript API

```javascript
// Enable graphic engine (creates main window)
await Graphic.enableEngine(GraphicEngines.Bevy, {
    title: "My Game",
    width: 1920,
    height: 1080,
    resizable: true,
    fullscreen: false,
    position: WindowPositionModes.Centered
});

// Get main window
const mainWindow = await Graphic.getMainWindow();

// Create additional window
const secondWindow = await Graphic.createWindow({
    title: "Debug Window",
    width: 800,
    height: 600
});

// Modify window
await mainWindow.setTitle("New Title");
await mainWindow.setSize(1280, 720);
await mainWindow.setMode(WindowModes.BorderlessFullscreen);

// Close window
await secondWindow.close();
```

## UI Creation with ECS

UI elements are now created using the ECS API instead of the legacy widget system. See [ecs.md](ecs.md) for complete documentation.

### Basic Example

```javascript
// Create a container
const container = await World.spawn({
    Node: {
        width: "100%",
        height: "100%",
        flexDirection: FlexDirection.Column,
        justifyContent: JustifyContent.Center,
        alignItems: AlignItems.Center
    },
    BackgroundColor: "#1a1a2e"
});

// Create text
const text = await World.spawn({
    Text: "Hello World",
    TextColor: "#ffffff",
    TextFont: { size: 32 }
});
await text.setParent(container);

// Create interactive button
const button = await World.spawn({
    Button: true,
    Node: {
        padding: 20,
        width: 200,
        height: 50
    },
    BackgroundColor: "#4A90D9",
    HoverBackgroundColor: "#5BA0E9",
    PressedBackgroundColor: "#3A80C9"
});

// Register click callback
await button.on("click", (event) => {
    console.log(`Button clicked at ${event.x}, ${event.y}`);
});
```

### Component Types for UI

| Component | Description |
|-----------|-------------|
| `Node` | Layout properties (width, height, flex, padding, margin) |
| `Text` | Text content |
| `TextColor` | Text color |
| `TextFont` | Font size and family |
| `Button` | Makes entity interactive |
| `BackgroundColor` | Background color |
| `HoverBackgroundColor` | Color when hovered (pseudo-component) |
| `PressedBackgroundColor` | Color when pressed (pseudo-component) |
| `DisabledBackgroundColor` | Color when disabled (pseudo-component) |
| `ImageNode` | Image display |

## Event System

### Event Flow

```
┌─────────────────────────────────────────────────────┐
│                   Bevy Main Thread                  │
│  • Detects Interaction changes on Button entities   │
│  • Sends EntityEventCallback via channel            │
└─────────────────────────────────────────────────────┘
           ↓ event_tx (tokio::sync::mpsc)
┌─────────────────────────────────────────────────────┐
│                  Worker Thread                      │
│  • Main event loop receives GraphicEvent            │
│  • Calls RuntimeAdapter.dispatch_entity_event()     │
│  • Handler executes in mod's JS context             │
└─────────────────────────────────────────────────────┘
```

### Entity Event Callbacks

Instead of widget events, use ECS entity callbacks:

```javascript
// Register callback when entity is clicked
await entity.on("click", (event) => {
    console.log("Clicked!", event);
});

// Register hover callbacks
await entity.on("hover_enter", (event) => {
    console.log("Mouse entered");
});

await entity.on("hover_leave", (event) => {
    console.log("Mouse left");
});

// Unregister callback
await entity.off("click");
```

### Window Events

| Event | Description |
|-------|-------------|
| `WindowCreated` | Window was created |
| `WindowClosed` | Window was closed |
| `WindowResized` | Window size changed |
| `WindowFocused` | Window focus changed |
| `WindowMoved` | Window position changed |

## Asset Management

### Font Loading

Fonts must be loaded before use:

```javascript
// Load font with custom alias
await Graphic.loadFont("mods/my-mod/assets/fonts/Custom.ttf", "custom");

// Use in ECS entity
const text = await World.spawn({
    Text: "Hello",
    TextFont: { family: "custom", size: 24 }
});

// Set window default font
await window.setFont("custom", 16);

// Unload when done
await Graphic.unloadFont("custom");

// List loaded fonts
const fonts = Graphic.getLoadedFonts();
```

### Image Loading

Images are loaded via the Resource API:

```javascript
// Load image resource
Resource.load("mods/my-mod/assets/images/bg.png", "background");
await Resource.whenLoadedAll();

// Use in ECS entity
const image = await World.spawn({
    Node: { width: "100%", height: "100%" },
    ImageNode: {
        resource_id: "background",
        image_mode: NodeImageMode.Stretch
    }
});
```

### Path Security

All asset paths are validated against permitted directories (`data_dir`, `config_dir`). Path traversal attacks are blocked.

## Error Handling

### Server-Side Errors

All graphic operations return descriptive errors on the server:

```javascript
try {
    await Graphic.enableEngine(GraphicEngines.Bevy);
} catch (e) {
    // "Graphic.enableEngine() is not available on the server.
    //  This method is client-only."
}
```

### Common Errors

| Error | Cause |
|-------|-------|
| "No graphic engine enabled" | Call `enableEngine()` first |
| "A graphic engine is already enabled" | Engine already running |
| "Graphic engine '...' is not yet supported" | Unknown engine type |
| "Entity not found" | Invalid entity ID |
| "Window not found" | Invalid window ID |

## Thread Safety

`GraphicProxy` is designed to be shared via `Arc`:

- `RwLock` protects window registries
- `Mutex` protects event receiver
- `AtomicU64` for ID counters
- Commands use `std::sync::mpsc` (sync channel for Bevy)
- Events use `tokio::sync::mpsc` (async channel for worker)

## Supported Engines

Currently only **Bevy** is supported:

```rust
pub enum GraphicEngines {
    Bevy,      // ✅ Supported
    Wgpu,      // ❌ Planned
    Terminal,  // ❌ Planned
}
```

## Per-Window Camera System

The graphic engine implements a **camera creation** system that automatically manages UI cameras for each window.

### How It Works

When the engine is enabled, a **Camera2D** is created for the main window. Each window has its own camera for the UI to be visible:

1. **Engine startup**: Camera2D and root UI node are created for the main window
2. **Additional windows**: Each new window gets its own camera and root node
3. **Window close cleanup**: When a window is closed, its camera and root UI node are automatically despawned

### Technical Details

The camera system uses these Bevy components:

```rust
// Camera targeting a specific window
Camera {
    target: RenderTarget::Window(WindowRef::Entity(window_entity)),
    ..default()
}

// Root UI node targeting the camera
Node { width: Val::Percent(100.0), height: Val::Percent(100.0), .. }
UiTargetCamera(camera_entity)
```

The `WindowUIRegistry` tracks:
- `window_cameras: HashMap<u64, Entity>` - Camera entity per window
- `window_roots: HashMap<u64, Entity>` - Root UI node per window

## Migration from Widget API

If you have code using the legacy widget API, here's how to migrate:

### Before (Widget API - REMOVED)

```javascript
// OLD - No longer works
const panel = await window.createWidget(WidgetTypes.Container, {
    width: "100%",
    height: "100%",
    backgroundColor: "#1a1a2e"
});

const button = await panel.createChild(WidgetTypes.Button, {
    label: "Click Me",
    backgroundColor: "#4A90D9"
});

button.on("click", () => console.log("Clicked!"));
```

### After (ECS API)

```javascript
// NEW - Use ECS API
const panel = await World.spawn({
    Node: { width: "100%", height: "100%" },
    BackgroundColor: "#1a1a2e"
});

const button = await World.spawn({
    Button: true,
    Node: { padding: 10 },
    BackgroundColor: "#4A90D9",
    Text: "Click Me"
});
await button.setParent(panel);

await button.on("click", () => console.log("Clicked!"));
```

## Best Practices

1. **Always check engine availability** before graphic operations
2. **Load fonts before creating entities** that use them
3. **Use entity IDs** to track and update UI elements
4. **Clean up entities** when changing screens/scenes using `entity.destroy()`
5. **Handle window close events** for multi-window apps
6. **Use percentage sizes** for responsive layouts
7. **Preload large images** via Resource.load() to avoid frame drops
8. **Use the ECS API** (`World.spawn()`) instead of the removed widget API
