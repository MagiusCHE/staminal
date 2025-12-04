# Graphic Engine: Windows and Widgets

This document describes the architecture of the Staminal graphic system, focusing on window management and widget creation.

## Overview

The graphic system provides a **language-agnostic API** for creating windows and UI widgets. All operations are **client-only** - on the server, all graphic methods return descriptive errors.

## Architecture

### Threading Model

```
┌─────────────────────────────────────────────────────┐
│              Main Thread (Bevy)                     │
│  ┌──────────────────────────────────────────────┐  │
│  │ BevyEngine                                    │  │
│  │  • Window management (winit)                  │  │
│  │  • UI rendering (bevy_ui)                     │  │
│  │  • Widget entity creation/updates             │  │
│  └──────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────┘
           ↕ channels (std::sync::mpsc / tokio::sync::mpsc)
┌─────────────────────────────────────────────────────┐
│            Worker Thread (tokio runtime)            │
│  ┌──────────────────────────────────────────────┐  │
│  │ GraphicProxy                                  │  │
│  │  • Routes commands to engine                  │  │
│  │  • Receives events from engine                │  │
│  │  • Manages window/widget registries           │  │
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
| [proxy.rs](../apps/shared/stam_mod_runtimes/src/api/graphic/proxy.rs) | `GraphicProxy` - Central coordinator |
| [commands.rs](../apps/shared/stam_mod_runtimes/src/api/graphic/commands.rs) | Command definitions (worker → engine) |
| [events.rs](../apps/shared/stam_mod_runtimes/src/api/graphic/events.rs) | Event definitions (engine → worker) |
| [window.rs](../apps/shared/stam_mod_runtimes/src/api/graphic/window.rs) | Window configuration types |
| [widget.rs](../apps/shared/stam_mod_runtimes/src/api/graphic/widget.rs) | Widget types and configuration |
| [engines.rs](../apps/shared/stam_mod_runtimes/src/api/graphic/engines.rs) | Supported engine definitions |
| [bevy.rs](../apps/stam_client/src/engines/bevy.rs) | Bevy engine implementation |

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
- `widgets`: Registry of widget IDs → `WidgetInfo`
- `widget_subscriptions`: Event subscriptions per widget
- `loaded_fonts`: Loaded font aliases
- `next_window_id` / `next_widget_id`: Atomic counters

## Window Management

### Window Lifecycle

1. **Enable Engine** → Creates main window (ID 1)
2. **Create Window** → Creates additional windows (ID 2+)
3. **Modify Window** → Set title, size, fullscreen, visibility
4. **Close Window** → Destroys window and its widgets

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
| `SetWindowFullscreen` | Toggle fullscreen mode |
| `SetWindowVisible` | Show/hide window |
| `SetWindowFont` | Set default font for window widgets |

### JavaScript API

```javascript
// Enable graphic engine (creates main window)
await graphic.enableEngine("bevy", {
    title: "My Game",
    width: 1920,
    height: 1080,
    resizable: true,
    fullscreen: false,
    position: 1  // 0=default, 1=centered
});

// Get main window
const mainWindow = await graphic.getMainWindow();

// Create additional window
const secondWindow = await graphic.createWindow({
    title: "Debug Window",
    width: 800,
    height: 600
});

// Modify window
await mainWindow.setTitle("New Title");
await mainWindow.setSize(1280, 720);
await mainWindow.setFullscreen(true);

// Close window
await secondWindow.close();
```

## Widget System

### Widget Types

| Type | Description | Key Properties |
|------|-------------|----------------|
| `Container` | Flexbox/grid layout | `direction`, `justifyContent`, `alignItems`, `gap` |
| `Text` | Static/dynamic text | `content`, `font`, `fontColor`, `textAlign` |
| `Button` | Clickable button | `label`, `hoverColor`, `pressedColor`, `disabled` |
| `Image` | Image display | `image.path`, `image.scaleMode`, `image.tint` |
| `Panel` | Container with background | `backgroundColor`, `backgroundImage` |

### Widget Hierarchy

```
Window (Bevy Entity)
 └── RootNode (Node, TargetCamera)
      └── Container (Node, Layout)
           ├── Text (Node, Text, TextColor)
           ├── Button (Node, Button, BackgroundColor)
           │    └── ButtonLabel (Text)
           └── Panel (Node, BackgroundColor)
                └── ... (nested widgets)
```

### Widget Configuration

```rust
pub struct WidgetConfig {
    // Hierarchy
    pub parent_id: Option<u64>,     // Parent widget (None = window root)

    // Layout
    pub layout: Option<LayoutType>,
    pub direction: Option<FlexDirection>,
    pub justify_content: Option<JustifyContent>,
    pub align_items: Option<AlignItems>,
    pub gap: Option<f32>,

    // Dimensions
    pub width: Option<SizeValue>,   // Px(f32), Percent(f32), Auto
    pub height: Option<SizeValue>,
    pub min_width: Option<SizeValue>,
    pub max_width: Option<SizeValue>,

    // Spacing
    pub margin: Option<EdgeInsets>,
    pub padding: Option<EdgeInsets>,

    // Appearance
    pub background_color: Option<ColorValue>,
    pub border_color: Option<ColorValue>,
    pub border_width: Option<EdgeInsets>,
    pub border_radius: Option<f32>,
    pub opacity: Option<f32>,

    // Text
    pub content: Option<String>,
    pub font: Option<FontConfig>,
    pub font_color: Option<ColorValue>,
    pub text_align: Option<TextAlign>,

    // Button
    pub label: Option<String>,
    pub hover_color: Option<ColorValue>,
    pub pressed_color: Option<ColorValue>,
    pub disabled: Option<bool>,

    // Image
    pub image: Option<ImageConfig>,
    pub background_image: Option<ImageConfig>,
}
```

### Color Values

Colors support multiple formats:

```rust
// Rust
ColorValue::rgba(1.0, 0.0, 0.0, 1.0)
ColorValue::from_hex("#FF0000")
ColorValue::from_hex("#FF0000FF")
ColorValue::from_hex("rgba(255, 0, 0, 0.5)")
```

```javascript
// JavaScript
backgroundColor: "#FF0000"           // Hex RGB
backgroundColor: "#FF0000FF"         // Hex RGBA
backgroundColor: "rgba(255,0,0,0.5)" // RGBA function
backgroundColor: "rgb(255,0,0)"      // RGB function
```

### Widget Commands

| Command | Description |
|---------|-------------|
| `CreateWidget` | Create widget in window |
| `UpdateWidgetProperty` | Update single property |
| `UpdateWidgetConfig` | Update multiple properties |
| `DestroyWidget` | Destroy widget and children |
| `ReparentWidget` | Move to new parent |
| `ClearWindowWidgets` | Destroy all widgets in window |
| `SubscribeWidgetEvents` | Register for click/hover/focus |
| `UnsubscribeWidgetEvents` | Unregister events |

### JavaScript API

```javascript
// Create panel
const panel = await window.createWidget("panel", {
    width: "100%",
    height: "100%",
    backgroundColor: "rgba(0, 0, 0, 0.7)"
});

// Create text with custom font
await graphic.loadFont("fonts/Roboto-Bold.ttf", "roboto-bold");
const title = await window.createWidget("text", {
    parent: panel.id,
    content: "Hello World",
    font: { family: "roboto-bold", size: 32 },
    fontColor: "#FFFFFF"
});

// Create button with events
const button = await window.createWidget("button", {
    parent: panel.id,
    label: "Click Me",
    backgroundColor: "#4A90D9",
    hoverColor: "#5BA0E9",
    pressedColor: "#3A80C9"
});

button.onClick((event) => {
    console.log(`Clicked at ${event.x}, ${event.y}`);
});

// Update widget
await button.setProperty("label", "Clicked!");

// Destroy widget
await button.destroy();

// Clear all widgets
await window.clearWidgets();
```

## Event System

### Event Flow

```
┌─────────────────────────────────────────────────────┐
│                   Bevy Main Thread                  │
│  • Detects Interaction changes                      │
│  • Sends GraphicEvent via channel                   │
└─────────────────────────────────────────────────────┘
           ↓ event_tx (tokio::sync::mpsc)
┌─────────────────────────────────────────────────────┐
│                  Worker Thread                      │
│  • Main event loop receives GraphicEvent            │
│  • Calls RuntimeAdapter.dispatch_widget_callback()  │
│  • Handler executes in mod's JS context             │
└─────────────────────────────────────────────────────┘
```

### Widget Events

| Event | Description |
|-------|-------------|
| `WidgetCreated` | Widget was created |
| `WidgetDestroyed` | Widget was destroyed |
| `WidgetClicked` | Mouse click on widget |
| `WidgetHovered` | Mouse enter/leave widget |
| `WidgetFocused` | Focus gained/lost |

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
await graphic.loadFont("mods/my-mod/assets/fonts/Custom.ttf", "custom");

// Use in widget
const text = await window.createWidget("text", {
    content: "Hello",
    font: { family: "custom", size: 24 }
});

// Set window default font
await window.setFont("custom", 16);

// Unload when done
await graphic.unloadFont("custom");

// List loaded fonts
const fonts = graphic.getLoadedFonts();
```

### Image Loading

Images are loaded on-demand, but can be preloaded:

```javascript
// Preload for faster first use
await graphic.preloadImage("mods/my-mod/assets/images/background.png");

// Use in widget
const img = await window.createWidget("image", {
    image: {
        path: "mods/my-mod/assets/images/background.png",
        scaleMode: "fit",
        tint: "rgba(255,255,255,0.9)"
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
    await graphic.enableEngine("bevy");
} catch (e) {
    // "graphic.enableEngine() is not available on the server.
    //  This method is client-only."
}
```

### Common Errors

| Error | Cause |
|-------|-------|
| "No graphic engine enabled" | Call `enableEngine()` first |
| "A graphic engine is already enabled" | Engine already running |
| "Graphic engine '...' is not yet supported" | Unknown engine type |
| "Widget not found" | Invalid widget ID |
| "Window not found" | Invalid window ID |

## Thread Safety

`GraphicProxy` is designed to be shared via `Arc`:

- `RwLock` protects window/widget registries
- `Mutex` protects event receiver
- `AtomicU64` for ID counters
- Commands use `std::sync::mpsc` (sync channel for Bevy)
- Events use `tokio::sync::mpsc` (async channel for worker)

## Supported Engines

Currently only **Bevy** is supported:

```rust
pub enum GraphicEngines {
    Bevy,      // ✅ Supported
    SDL,       // ❌ Planned
    OpenGL,    // ❌ Planned
    Vulkan,    // ❌ Planned
}
```

## Best Practices

1. **Always check engine availability** before graphic operations
2. **Load fonts before creating widgets** that use them
3. **Use widget IDs** to track and update widgets
4. **Clean up widgets** when changing screens/scenes
5. **Handle window close events** for multi-window apps
6. **Use percentage sizes** for responsive layouts
7. **Preload large images** to avoid frame drops
