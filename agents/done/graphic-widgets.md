# Implementation Plan: UI Widgets for GraphicEngine

## Executive Summary

This document describes the implementation of the UI widget system for Staminal, allowing mod scripts to create, modify, and manage graphical widgets within GraphicEngine Bevy windows.

**Key principle**: The system is **language-agnostic**. The core API is defined in Rust and each runtime (JavaScript, Lua, C#, Rust, C++) implements its own bindings to this common API.

## Table of Contents

1. [Current Architecture Analysis](#1-current-architecture-analysis)
2. [UI Strategy Choice](#2-ui-strategy-choice)
3. [Widget System Design](#3-widget-system-design)
4. [Core API (Language-Agnostic)](#4-core-api-language-agnostic)
5. [Runtime-Specific Bindings](#5-runtime-specific-bindings)
6. [Rust Core Implementation](#6-rust-core-implementation)
7. [Event and Callback System](#7-event-and-callback-system)
8. [Implementation Plan](#8-implementation-plan)

---

## 1. Current Architecture Analysis

### 1.1 Threading Model

```
┌─────────────────────────────────────────────────────┐
│              Main Thread (main.rs)                  │
│  ┌──────────────────────────────────────────────┐  │
│  │ BevyEngine                                    │  │
│  │  • Window management                         │  │
│  │  • Rendering pipeline                        │  │
│  │  • UI rendering (bevy_ui)                    │  │
│  └──────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────┘
           ↕ channels (mpsc)
┌─────────────────────────────────────────────────────┐
│            Worker Thread (tokio runtime)            │
│  ┌──────────────────────────────────────────────┐  │
│  │ GraphicProxy (Language-Agnostic Core API)    │  │
│  │  • Sends commands to Bevy                    │  │
│  │  • Receives events from Bevy                 │  │
│  │  • Shared by ALL runtime adapters            │  │
│  └──────────────────────────────────────────────┘  │
│  ┌──────────────────────────────────────────────┐  │
│  │ Runtime Adapters (one per language)          │  │
│  │  ├── JavaScript (QuickJS) ← current          │  │
│  │  ├── Lua (mlua) ← future                     │  │
│  │  ├── C# (dotnet) ← future                    │  │
│  │  ├── Rust (native) ← future                  │  │
│  │  └── C++ (FFI) ← future                      │  │
│  └──────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────┘
```

### 1.2 Existing Command Flow

1. **Script (any language)** calls the widget API (e.g., `window.createWidget()`)
2. **Runtime Adapter** translates the call to `GraphicProxy`
3. **GraphicProxy** generates an ID, sends `GraphicCommand::CreateWidget`
4. **Bevy** receives the command, creates the widget entity, responds
5. **GraphicProxy** stores `WidgetInfo`, returns ID to Runtime Adapter
6. **Runtime Adapter** creates the Widget object in the specific language

### 1.3 Key Files

| File | Description |
|------|-------------|
| `apps/stam_client/src/engines/bevy.rs` | BevyEngine implementation |
| `apps/shared/stam_mod_runtimes/src/api/graphic/proxy.rs` | GraphicProxy |
| `apps/shared/stam_mod_runtimes/src/api/graphic/commands.rs` | Commands |
| `apps/shared/stam_mod_runtimes/src/api/graphic/events.rs` | Events |
| `apps/shared/stam_mod_runtimes/src/adapters/js/bindings.rs` | JS Bindings |

---

## 2. UI Strategy Choice

### 2.1 Analyzed Options

| Option | Pros | Cons |
|--------|------|------|
| **native bevy_ui** | Perfect ECS integration, no extra dependencies, Bevy's future | Verbose, evolving API |
| **bevy_egui** | Immediate mode API, easy to use, documentation | Extra dependency, different style from Bevy |
| **Sickle UI** | Ergonomic, reduces boilerplate | Extra dependency, less mature |

### 2.2 Decision: Native bevy_ui

**Motivation:**
1. **Bevy Compatibility**: Follows the engine's official direction
2. **ECS Integration**: Widgets are entities, native queries
3. **No extra dependencies**: Reduces complexity and version conflicts
4. **Future-proof**: Bevy is actively improving the UI system

**Accepted trade-off:**
- More verbosity on the Rust side (not visible to scripts)
- Need to build abstraction for scripts

---

## 3. Widget System Design

### 3.1 Widget Hierarchy

```
Window (Bevy Entity)
 └── RootNode (Node, TargetCamera)
      └── Container (Node, Layout)
           ├── Text (Node, Text, TextColor)
           ├── Button (Node, Button, BackgroundColor)
           │    └── ButtonLabel (Text)
           ├── Image (Node, UiImage)
           └── Panel (Node, BackgroundColor)
                └── ... (nested widgets)
```

### 3.2 Supported Widgets (Phase 1)

| Widget | Bevy Components | Description |
|--------|-----------------|-------------|
| `Container` | `Node` | Flexbox/grid layout |
| `Text` | `Node`, `Text`, `TextColor` | Static or dynamic text |
| `Button` | `Node`, `Button`, `BackgroundColor`, `BorderColor` | Clickable button |
| `Image` | `Node`, `UiImage` | Image from asset |
| `Panel` | `Node`, `BackgroundColor` | Container with background |

### 3.3 Widget ID System

```rust
// Each widget has a unique ID generated by Staminal
pub struct WidgetId(u64);

// Registry in BevyEngine
pub struct WidgetRegistry {
    widgets: HashMap<u64, Entity>,
    next_id: AtomicU64,
}
```

### 3.4 Marker Component

```rust
/// Marks an entity as a Staminal widget
#[derive(Component)]
pub struct StamWidget {
    pub id: u64,
    pub window_id: u64,
    pub widget_type: WidgetType,
}

#[derive(Clone, Copy, PartialEq)]
pub enum WidgetType {
    Container,
    Text,
    Button,
    Image,
    Panel,
}
```

---

## 4. Core API (Language-Agnostic)

The core API is defined in Rust in the `stam_mod_runtimes::api::graphic` module. Each runtime adapter translates these structures into its own language.

### 4.1 Design Principles

1. **Simple data structures**: Only primitive types and serializable structs
2. **ID-based references**: Widgets referenced via `u64` ID, not pointers
3. **Async by default**: All operations communicating with Bevy are async
4. **Events via channel**: Callbacks implemented as events, not as function pointers
5. **No language-specific types**: No `Function`, `Closure`, or language-specific types

### 4.2 Core Rust Types

```rust
// In apps/shared/stam_mod_runtimes/src/api/graphic/widget.rs

/// Supported widget types
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum WidgetType {
    Container,
    Text,
    Button,
    Image,
    Panel,
}

/// Widget configuration (serializable for all runtimes)
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct WidgetConfig {
    /// Parent widget ID (None = window root)
    pub parent_id: Option<u64>,

    // === Layout ===
    pub layout: Option<LayoutType>,
    pub direction: Option<FlexDirection>,
    pub justify_content: Option<JustifyContent>,
    pub align_items: Option<AlignItems>,
    pub gap: Option<f32>,

    // === Dimensions ===
    pub width: Option<SizeValue>,
    pub height: Option<SizeValue>,
    pub min_width: Option<SizeValue>,
    pub max_width: Option<SizeValue>,
    pub min_height: Option<SizeValue>,
    pub max_height: Option<SizeValue>,

    // === Spacing ===
    pub margin: Option<EdgeInsets>,
    pub padding: Option<EdgeInsets>,

    // === Appearance and Transparency ===
    pub background_color: Option<ColorValue>,    // RGBA with alpha
    pub border_color: Option<ColorValue>,        // RGBA with alpha
    pub border_width: Option<EdgeInsets>,
    pub border_radius: Option<f32>,
    pub opacity: Option<f32>,                    // 0.0-1.0, global widget opacity
    pub blend_mode: Option<BlendMode>,           // Blend mode

    // === Background Image ===
    pub background_image: Option<ImageConfig>,   // Background image (alternative to background_color)

    // === Text and Font ===
    pub content: Option<String>,
    pub font: Option<FontConfig>,                // Complete font configuration
    pub font_color: Option<ColorValue>,          // RGBA with alpha
    pub text_align: Option<TextAlign>,
    pub text_shadow: Option<ShadowConfig>,       // Text shadow

    // === Button ===
    pub label: Option<String>,
    pub hover_color: Option<ColorValue>,         // RGBA with alpha
    pub pressed_color: Option<ColorValue>,       // RGBA with alpha
    pub disabled: Option<bool>,
    pub disabled_color: Option<ColorValue>,      // Color when disabled

    // === Image Widget ===
    pub image: Option<ImageConfig>,              // For Image type widgets
}

/// Image configuration (for background or Image widget)
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ImageConfig {
    /// Image asset path (relative to mod or asset folder)
    pub path: String,
    /// Scale mode
    pub scale_mode: Option<ImageScaleMode>,
    /// Tint color (multiplied with image pixels)
    pub tint: Option<ColorValue>,
    /// Image opacity (0.0-1.0)
    pub opacity: Option<f32>,
    /// Horizontal flip
    pub flip_x: Option<bool>,
    /// Vertical flip
    pub flip_y: Option<bool>,
    /// Image region to show (for sprite sheets)
    pub source_rect: Option<RectValue>,
}

/// Font configuration
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FontConfig {
    /// Font name or path (e.g., "Roboto", "fonts/custom.ttf")
    pub family: String,
    /// Size in pixels
    pub size: f32,
    /// Font weight
    pub weight: Option<FontWeight>,
    /// Style (normal, italic)
    pub style: Option<FontStyle>,
    /// Letter spacing
    pub letter_spacing: Option<f32>,
    /// Line height (multiplier)
    pub line_height: Option<f32>,
}

/// Shadow configuration (for text or widgets)
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ShadowConfig {
    pub color: ColorValue,           // RGBA with alpha
    pub offset_x: f32,
    pub offset_y: f32,
    pub blur_radius: Option<f32>,
}

/// Rectangle (for image source_rect)
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RectValue {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

/// Size value (supports px, %, auto)
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum SizeValue {
    Px(f32),
    Percent(f32),
    Auto,
}

/// Insets for margin/padding/border (top, right, bottom, left)
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EdgeInsets {
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
    pub left: f32,
}

/// Color (RGBA 0.0-1.0) with full transparency support
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ColorValue {
    pub r: f32,    // 0.0-1.0
    pub g: f32,    // 0.0-1.0
    pub b: f32,    // 0.0-1.0
    pub a: f32,    // 0.0-1.0 (0 = transparent, 1 = opaque)
}

impl ColorValue {
    /// Create color from hex string (e.g., "#FF0000", "#FF0000FF", "rgba(255,0,0,0.5)")
    pub fn from_hex(hex: &str) -> Result<Self, ColorParseError>;

    /// Create color with specific alpha
    pub fn with_alpha(self, alpha: f32) -> Self;

    /// Create fully transparent color
    pub fn transparent() -> Self { Self { r: 0.0, g: 0.0, b: 0.0, a: 0.0 } }

    /// Predefined colors
    pub fn white() -> Self { Self { r: 1.0, g: 1.0, b: 1.0, a: 1.0 } }
    pub fn black() -> Self { Self { r: 0.0, g: 0.0, b: 0.0, a: 1.0 } }
}

/// Blend mode for advanced graphic effects
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum BlendMode {
    /// Normal (default) - standard alpha blending
    Normal,
    /// Multiply colors (darkens)
    Multiply,
    /// Screen (lightens)
    Screen,
    /// Overlay (combination of multiply and screen)
    Overlay,
    /// Additive (adds brightness)
    Add,
    /// Subtract color
    Subtract,
}

/// Scale mode for images
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ImageScaleMode {
    /// Scale to fill maintaining aspect ratio (may crop)
    Fill,
    /// Scale to contain maintaining aspect ratio (may leave gaps)
    Fit,
    /// Scale to fill ignoring aspect ratio
    Stretch,
    /// No scaling, original size
    None,
    /// Repeat image as pattern (tile)
    Tile,
    /// 9-slice scaling for UI (preserves borders)
    NineSlice {
        top: f32,
        right: f32,
        bottom: f32,
        left: f32,
    },
}

/// Font weight
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum FontWeight {
    Thin,       // 100
    Light,      // 300
    Regular,    // 400
    Medium,     // 500
    SemiBold,   // 600
    Bold,       // 700
    ExtraBold,  // 800
    Black,      // 900
}

/// Font style
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum FontStyle {
    Normal,
    Italic,
    Oblique,
}

/// Widget information (returned from queries)
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WidgetInfo {
    pub id: u64,
    pub window_id: u64,
    pub widget_type: WidgetType,
    pub parent_id: Option<u64>,
    pub children_ids: Vec<u64>,
}

/// Dynamic property value (for updates)
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum PropertyValue {
    String(String),
    Number(f64),
    Bool(bool),
    Color(ColorValue),
    Size(SizeValue),
}
```

### 4.3 GraphicProxy API (Extended for Widgets)

```rust
// In apps/shared/stam_mod_runtimes/src/api/graphic/proxy.rs

impl GraphicProxy {
    // === Widget Creation ===

    /// Create a new widget in the specified window
    pub async fn create_widget(
        &self,
        window_id: u64,
        widget_type: WidgetType,
        config: WidgetConfig,
    ) -> Result<u64, GraphicError>;

    // === Widget Modification ===

    /// Update a widget property
    pub async fn update_widget_property(
        &self,
        widget_id: u64,
        property: &str,
        value: PropertyValue,
    ) -> Result<(), GraphicError>;

    /// Update multiple properties in a single call
    pub async fn update_widget_config(
        &self,
        widget_id: u64,
        config: WidgetConfig,
    ) -> Result<(), GraphicError>;

    // === Widget Hierarchy ===

    /// Move a widget under a new parent
    pub async fn reparent_widget(
        &self,
        widget_id: u64,
        new_parent_id: Option<u64>,
    ) -> Result<(), GraphicError>;

    /// Destroy a widget and all its children
    pub async fn destroy_widget(&self, widget_id: u64) -> Result<(), GraphicError>;

    /// Destroy all widgets in a window
    pub async fn clear_window_widgets(&self, window_id: u64) -> Result<(), GraphicError>;

    // === Widget Query ===

    /// Get information about a widget
    pub fn get_widget_info(&self, widget_id: u64) -> Option<WidgetInfo>;

    /// Get all widgets in a window
    pub fn get_window_widgets(&self, window_id: u64) -> Vec<WidgetInfo>;

    /// Get the root widget of a window
    pub fn get_window_root_widget(&self, window_id: u64) -> Option<u64>;

    // === Event Subscription ===

    /// Register interest in widget events (click, hover, etc.)
    pub async fn subscribe_widget_events(
        &self,
        widget_id: u64,
        event_types: Vec<WidgetEventType>,
    ) -> Result<(), GraphicError>;

    /// Remove interest in widget events
    pub async fn unsubscribe_widget_events(
        &self,
        widget_id: u64,
        event_types: Vec<WidgetEventType>,
    ) -> Result<(), GraphicError>;

    // === Asset Management (Font & Images) ===

    /// Load a custom font from file
    /// Returns a handle that can be used in FontConfig.family
    pub async fn load_font(
        &self,
        path: &str,           // Path relative to mod/assets folder
        alias: Option<&str>,  // Name to use for referencing the font (default: filename)
    ) -> Result<String, GraphicError>;

    /// Preload an image (optional, to avoid lag on first use)
    pub async fn preload_image(
        &self,
        path: &str,
    ) -> Result<(), GraphicError>;

    /// Get list of loaded fonts
    pub fn get_loaded_fonts(&self) -> Vec<FontInfo>;

    /// Unload a font from memory
    pub async fn unload_font(&self, alias: &str) -> Result<(), GraphicError>;
}

/// Information about a loaded font
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FontInfo {
    pub alias: String,
    pub path: String,
    pub family_name: Option<String>,  // Internal font name if available
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum WidgetEventType {
    Click,
    Hover,
    Focus,
}
```

### 4.4 Widget Events

```rust
// In apps/shared/stam_mod_runtimes/src/api/graphic/events.rs

/// Widget events (sent from Bevy to worker thread)
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum WidgetEvent {
    Created {
        window_id: u64,
        widget_id: u64,
        widget_type: WidgetType,
    },
    Destroyed {
        window_id: u64,
        widget_id: u64,
    },
    Clicked {
        window_id: u64,
        widget_id: u64,
        x: f32,
        y: f32,
        button: MouseButton,
    },
    Hovered {
        window_id: u64,
        widget_id: u64,
        entered: bool,
        x: f32,
        y: f32,
    },
    Focused {
        window_id: u64,
        widget_id: u64,
        focused: bool,
    },
}
```

---

## 5. Runtime-Specific Bindings

Each runtime adapter implements bindings to the core API. The core logic remains in Rust, bindings only translate types.

### 5.1 Binding Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                     GraphicProxy (Rust Core)                     │
│  • create_widget(), update_widget_property(), destroy_widget()  │
│  • No dependency on specific languages                          │
└─────────────────────────────────────────────────────────────────┘
                              ↑
        ┌─────────────────────┼─────────────────────┐
        ↓                     ↓                     ↓
┌───────────────┐     ┌───────────────┐     ┌───────────────┐
│ JS Adapter    │     │ Lua Adapter   │     │ C# Adapter    │
│ (rquickjs)    │     │ (mlua)        │     │ (dotnet)      │
│               │     │               │     │               │
│ WidgetJS      │     │ WidgetLua     │     │ WidgetCS      │
│ .onClick(fn)  │     │ :onClick(fn)  │     │ .OnClick(fn)  │
└───────────────┘     └───────────────┘     └───────────────┘
```

### 5.2 JavaScript Binding (Current Example)

```javascript
// === Custom Font Loading ===
await graphic.loadFont("fonts/Roboto-Bold.ttf", "roboto-bold");
await graphic.loadFont("fonts/GameFont.otf", "game-font");

// === UI Creation with transparency and images ===
const mainPanel = await window.createWidget("panel", {
    width: "100%",
    height: "100%",
    // Semi-transparent background
    backgroundColor: "rgba(0, 0, 0, 0.7)",  // Black with 70% opacity
    // Or background image
    backgroundImage: {
        path: "textures/background.png",
        scaleMode: "fill",
        opacity: 0.8
    }
});

// === Text with custom font and shadow ===
const title = await window.createWidget("text", {
    parent: mainPanel.id,
    content: "Game Title",
    font: {
        family: "game-font",  // Font loaded above
        size: 48,
        weight: "bold"
    },
    fontColor: "rgba(255, 255, 255, 0.9)",  // Almost opaque white
    textShadow: {
        color: "rgba(0, 0, 0, 0.5)",
        offsetX: 2,
        offsetY: 2,
        blurRadius: 4
    },
    textAlign: "center"
});

// === Button with transparent states ===
const button = await window.createWidget("button", {
    parent: mainPanel.id,
    label: "Start Game",
    font: { family: "roboto-bold", size: 18 },
    backgroundColor: "rgba(74, 144, 217, 0.8)",    // Semi-transparent blue
    hoverColor: "rgba(91, 160, 233, 0.9)",         // Brighter on hover
    pressedColor: "rgba(58, 128, 201, 1.0)",       // Opaque when pressed
    borderRadius: 8,
    padding: [12, 24, 12, 24]
});

// === Image widget for icons/sprites ===
const icon = await window.createWidget("image", {
    parent: button.id,
    image: {
        path: "icons/play.png",
        tint: "rgba(255, 255, 255, 0.9)",  // White tint
        scaleMode: "fit"
    },
    width: 24,
    height: 24
});

// === Panel with 9-slice for borders ===
const dialogBox = await window.createWidget("panel", {
    backgroundImage: {
        path: "ui/dialog-frame.png",
        scaleMode: {
            type: "nineSlice",
            top: 16, right: 16, bottom: 16, left: 16
        }
    },
    opacity: 0.95  // Global widget opacity
});

button.onClick((event) => {
    console.log(`Clicked at ${event.x}, ${event.y}`);
});

await button.setProperty("label", "Clicked!");
await button.destroy();
```

### 5.3 Lua Binding (Future Example)

```lua
-- Usage in Lua
local button = window:createWidget("button", {
    label = "Click Me",
    backgroundColor = "#4A90D9"
})

button:onClick(function(event)
    print("Clicked at " .. event.x .. ", " .. event.y)
end)

button:setProperty("label", "Clicked!")
button:destroy()
```

### 5.4 C# Binding (Future Example)

```csharp
// Usage in C#
var button = await window.CreateWidget("button", new WidgetConfig {
    Label = "Click Me",
    BackgroundColor = "#4A90D9"
});

button.OnClick += (sender, e) => {
    Console.WriteLine($"Clicked at {e.X}, {e.Y}");
};

await button.SetProperty("label", "Clicked!");
await button.Destroy();
```

### 5.5 Rust Native Mod (Future Example)

```rust
// Usage in Rust (native mod)
let button = window.create_widget(WidgetType::Button, WidgetConfig {
    label: Some("Click Me".into()),
    background_color: Some(ColorValue::from_hex("#4A90D9")),
    ..Default::default()
}).await?;

// Callback via event
system.on_widget_event(button.id, |event| {
    if let WidgetEvent::Clicked { x, y, .. } = event {
        println!("Clicked at {}, {}", x, y);
    }
});

button.set_property("label", PropertyValue::String("Clicked!".into())).await?;
button.destroy().await?;
```

### 5.6 Cross-Language Callback Handling

Callbacks are handled through the existing event system, not as function pointers:

```rust
// In runtime adapter (e.g., JS)
impl WidgetJS {
    pub fn on_click(&self, ctx: Ctx, handler: Function) -> Result<()> {
        // 1. Register interest with GraphicProxy
        self.graphic_proxy.subscribe_widget_events(
            self.widget_id,
            vec![WidgetEventType::Click]
        );

        // 2. Store handler in the runtime's local registry
        self.callback_registry.register(
            self.widget_id,
            "click",
            handler.into_persistent()
        );

        Ok(())
    }
}

// When event arrives from Bevy:
fn dispatch_widget_event(event: WidgetEvent, runtime: &mut JsRuntime) {
    match event {
        WidgetEvent::Clicked { widget_id, x, y, button } => {
            if let Some(handler) = runtime.callback_registry.get(widget_id, "click") {
                let event_obj = create_js_click_event(x, y, button);
                handler.call((event_obj,));
            }
        }
        // ...
    }
}
```

---

## 6. Rust Core Implementation

### 6.1 New GraphicCommand Types

```rust
// In apps/shared/stam_mod_runtimes/src/api/graphic/commands.rs

pub enum GraphicCommand {
    // ... existing commands ...

    // Widget commands
    CreateWidget {
        window_id: u64,
        widget_id: u64,
        parent_id: Option<u64>,
        widget_type: WidgetType,
        config: WidgetConfig,
        response_tx: oneshot::Sender<Result<(), String>>,
    },
    UpdateWidget {
        widget_id: u64,
        property: String,
        value: PropertyValue,
        response_tx: oneshot::Sender<Result<(), String>>,
    },
    DestroyWidget {
        widget_id: u64,
        response_tx: oneshot::Sender<Result<(), String>>,
    },
    ReparentWidget {
        widget_id: u64,
        new_parent_id: Option<u64>,
        response_tx: oneshot::Sender<Result<(), String>>,
    },
    QueryWidgets {
        window_id: u64,
        filter: WidgetFilter,
        response_tx: oneshot::Sender<Result<Vec<WidgetInfo>, String>>,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WidgetConfig {
    // Layout
    pub layout: Option<LayoutType>,
    pub direction: Option<FlexDirection>,
    pub justify_content: Option<JustifyContent>,
    pub align_items: Option<AlignItems>,
    pub gap: Option<f32>,

    // Dimensions
    pub width: Option<Val>,
    pub height: Option<Val>,
    pub min_width: Option<Val>,
    pub max_width: Option<Val>,
    pub min_height: Option<Val>,
    pub max_height: Option<Val>,

    // Spacing
    pub margin: Option<UiRect>,
    pub padding: Option<UiRect>,

    // Appearance
    pub background_color: Option<Color>,
    pub border_color: Option<Color>,
    pub border_width: Option<UiRect>,
    pub border_radius: Option<BorderRadius>,

    // Text
    pub content: Option<String>,
    pub font_size: Option<f32>,
    pub font_color: Option<Color>,
    pub text_align: Option<JustifyText>,

    // Button
    pub label: Option<String>,
    pub hover_color: Option<Color>,
    pub pressed_color: Option<Color>,
    pub disabled: Option<bool>,

    // Image
    pub image_path: Option<String>,
    pub scale_mode: Option<ImageScaleMode>,
}

#[derive(Clone, Debug)]
pub enum PropertyValue {
    String(String),
    Number(f64),
    Bool(bool),
    Color(Color),
    Val(Val),
}
```

### 6.2 New GraphicEvent Types

```rust
// In apps/shared/stam_mod_runtimes/src/api/graphic/events.rs

pub enum GraphicEvent {
    // ... existing events ...

    // Widget events
    WidgetCreated {
        window_id: u64,
        widget_id: u64,
        widget_type: WidgetType,
    },
    WidgetDestroyed {
        window_id: u64,
        widget_id: u64,
    },
    WidgetClicked {
        window_id: u64,
        widget_id: u64,
        x: f32,
        y: f32,
        button: MouseButton,
    },
    WidgetHovered {
        window_id: u64,
        widget_id: u64,
        entered: bool,
        x: f32,
        y: f32,
    },
    WidgetFocused {
        window_id: u64,
        widget_id: u64,
        focused: bool,
    },
    WidgetInteractionChanged {
        window_id: u64,
        widget_id: u64,
        interaction: Interaction,
    },
}
```

### 6.3 Bevy Widget System

```rust
// In apps/stam_client/src/engines/bevy.rs

/// Widget registry per window
#[derive(Resource, Default)]
pub struct WidgetRegistry {
    widgets: HashMap<u64, Entity>,
    widget_to_window: HashMap<u64, u64>,
    window_root_nodes: HashMap<u64, Entity>,
}

/// Marker component for Staminal widgets
#[derive(Component)]
pub struct StamWidget {
    pub id: u64,
    pub window_id: u64,
    pub widget_type: WidgetType,
}

/// Component for tracking registered callbacks
#[derive(Component, Default)]
pub struct WidgetCallbacks {
    pub on_click: bool,
    pub on_hover: bool,
    pub on_focus: bool,
}

/// Component for button hover/pressed colors
#[derive(Component)]
pub struct ButtonColors {
    pub normal: Color,
    pub hovered: Color,
    pub pressed: Color,
}

/// System for processing widget commands
fn process_widget_commands(
    mut commands: Commands,
    mut widget_registry: ResMut<WidgetRegistry>,
    window_registry: Res<WindowRegistry>,
    cmd_rx: Res<CommandReceiverRes>,
    event_tx: Res<EventSenderRes>,
    mut query: Query<&mut Node>,
    // ... other necessary queries
) {
    while let Ok(cmd) = cmd_rx.0.try_recv() {
        match cmd {
            GraphicCommand::CreateWidget {
                window_id, widget_id, parent_id, widget_type, config, response_tx
            } => {
                // Create the appropriate widget entity
                let entity = create_widget_entity(
                    &mut commands,
                    &widget_registry,
                    &window_registry,
                    window_id,
                    widget_id,
                    parent_id,
                    widget_type,
                    config,
                );

                widget_registry.widgets.insert(widget_id, entity);
                widget_registry.widget_to_window.insert(widget_id, window_id);

                let _ = response_tx.send(Ok(()));
                let _ = event_tx.0.try_send(GraphicEvent::WidgetCreated {
                    window_id,
                    widget_id,
                    widget_type,
                });
            }
            // ... other commands
        }
    }
}

/// System for handling widget interactions
fn handle_widget_interactions(
    mut interaction_query: Query<
        (&Interaction, &StamWidget, &WidgetCallbacks, Option<&ButtonColors>, &mut BackgroundColor),
        Changed<Interaction>
    >,
    event_tx: Res<EventSenderRes>,
) {
    for (interaction, stam_widget, callbacks, button_colors, mut bg_color) in interaction_query.iter_mut() {
        // Update color for button
        if let Some(colors) = button_colors {
            *bg_color = match *interaction {
                Interaction::Pressed => BackgroundColor(colors.pressed),
                Interaction::Hovered => BackgroundColor(colors.hovered),
                Interaction::None => BackgroundColor(colors.normal),
            };
        }

        // Send event to worker thread
        if callbacks.on_click && *interaction == Interaction::Pressed {
            let _ = event_tx.0.try_send(GraphicEvent::WidgetClicked {
                window_id: stam_widget.window_id,
                widget_id: stam_widget.id,
                x: 0.0, // TODO: get actual position
                y: 0.0,
                button: MouseButton::Left,
            });
        }

        if callbacks.on_hover {
            let entered = *interaction == Interaction::Hovered;
            let _ = event_tx.0.try_send(GraphicEvent::WidgetHovered {
                window_id: stam_widget.window_id,
                widget_id: stam_widget.id,
                entered,
                x: 0.0,
                y: 0.0,
            });
        }
    }
}

/// Helper function to create widget entities
fn create_widget_entity(
    commands: &mut Commands,
    widget_registry: &WidgetRegistry,
    window_registry: &WindowRegistry,
    window_id: u64,
    widget_id: u64,
    parent_id: Option<u64>,
    widget_type: WidgetType,
    config: WidgetConfig,
) -> Entity {
    // Determine parent entity
    let parent_entity = match parent_id {
        Some(pid) => widget_registry.widgets.get(&pid).copied(),
        None => widget_registry.window_root_nodes.get(&window_id).copied(),
    };

    // Build base Node
    let node = build_node_from_config(&config);

    // Create entity based on type
    match widget_type {
        WidgetType::Container => {
            let mut entity_commands = commands.spawn((
                node,
                StamWidget { id: widget_id, window_id, widget_type },
                WidgetCallbacks::default(),
            ));

            if let Some(color) = config.background_color {
                entity_commands.insert(BackgroundColor(color));
            }

            if let Some(parent) = parent_entity {
                entity_commands.set_parent(parent);
            }

            entity_commands.id()
        }
        WidgetType::Text => {
            let content = config.content.unwrap_or_default();
            let font_size = config.font_size.unwrap_or(16.0);
            let color = config.font_color.unwrap_or(Color::WHITE);

            let mut entity_commands = commands.spawn((
                node,
                Text::new(content),
                TextColor(color),
                TextFont { font_size, ..default() },
                StamWidget { id: widget_id, window_id, widget_type },
                WidgetCallbacks::default(),
            ));

            if let Some(parent) = parent_entity {
                entity_commands.set_parent(parent);
            }

            entity_commands.id()
        }
        WidgetType::Button => {
            let label = config.label.clone().unwrap_or_default();
            let normal = config.background_color.unwrap_or(Color::srgb(0.3, 0.3, 0.3));
            let hovered = config.hover_color.unwrap_or(Color::srgb(0.4, 0.4, 0.4));
            let pressed = config.pressed_color.unwrap_or(Color::srgb(0.2, 0.2, 0.2));

            let mut entity_commands = commands.spawn((
                node,
                Button,
                BackgroundColor(normal),
                ButtonColors { normal, hovered, pressed },
                StamWidget { id: widget_id, window_id, widget_type },
                WidgetCallbacks { on_click: true, on_hover: true, ..default() },
            ));

            // Add label as child
            entity_commands.with_children(|parent| {
                parent.spawn((
                    Text::new(label),
                    TextColor(Color::WHITE),
                    TextFont { font_size: config.font_size.unwrap_or(16.0), ..default() },
                ));
            });

            if let Some(parent) = parent_entity {
                entity_commands.set_parent(parent);
            }

            entity_commands.id()
        }
        // ... other types
    }
}
```

### 6.4 GraphicProxy Extension

```rust
// In apps/shared/stam_mod_runtimes/src/api/graphic/proxy.rs

impl GraphicProxy {
    // ... existing methods ...

    pub async fn create_widget(
        &self,
        window_id: u64,
        widget_type: WidgetType,
        parent_id: Option<u64>,
        config: WidgetConfig,
    ) -> Result<u64, GraphicError> {
        if !self.available {
            return Err(GraphicError::NotAvailable(
                "Widget creation is not available on the server".into()
            ));
        }

        let cmd_tx = self.command_tx.read().await;
        let cmd_tx = cmd_tx.as_ref().ok_or(GraphicError::NoEngineEnabled)?;

        let widget_id = self.next_widget_id.fetch_add(1, Ordering::SeqCst);
        let (response_tx, response_rx) = oneshot::channel();

        cmd_tx.send(GraphicCommand::CreateWidget {
            window_id,
            widget_id,
            parent_id,
            widget_type,
            config,
            response_tx,
        }).map_err(|_| GraphicError::ChannelClosed)?;

        response_rx.await
            .map_err(|_| GraphicError::ResponseTimeout)?
            .map_err(GraphicError::CommandFailed)?;

        Ok(widget_id)
    }

    pub async fn update_widget(
        &self,
        widget_id: u64,
        property: String,
        value: PropertyValue,
    ) -> Result<(), GraphicError> {
        // ... similar implementation ...
    }

    pub async fn destroy_widget(&self, widget_id: u64) -> Result<(), GraphicError> {
        // ... similar implementation ...
    }
}
```

### 6.5 JavaScript Widget Binding (Example)

```rust
// In apps/shared/stam_mod_runtimes/src/adapters/js/bindings.rs

/// Widget JavaScript class
#[derive(Clone, Trace)]
#[rquickjs::class]
pub struct WidgetJS {
    #[qjs(skip_trace)]
    widget_id: u64,
    #[qjs(skip_trace)]
    window_id: u64,
    #[qjs(skip_trace)]
    widget_type: WidgetType,
    #[qjs(skip_trace)]
    graphic_proxy: Arc<GraphicProxy>,
}

#[rquickjs::methods]
impl WidgetJS {
    #[qjs(get)]
    pub fn id(&self) -> u64 {
        self.widget_id
    }

    #[qjs(get, rename = "type")]
    pub fn widget_type(&self) -> String {
        self.widget_type.to_string()
    }

    #[qjs(get, rename = "windowId")]
    pub fn window_id(&self) -> u64 {
        self.window_id
    }

    #[qjs(rename = "setProperty")]
    pub async fn set_property<'js>(
        &self,
        ctx: Ctx<'js>,
        name: String,
        value: Value<'js>,
    ) -> rquickjs::Result<()> {
        let property_value = js_value_to_property_value(&ctx, value)?;

        self.graphic_proxy
            .update_widget(self.widget_id, name, property_value)
            .await
            .map_err(|e| ctx.throw(rquickjs::String::from_str(ctx.clone(), &e.to_string())?.into()))?;

        Ok(())
    }

    #[qjs(rename = "destroy")]
    pub async fn destroy<'js>(&self, ctx: Ctx<'js>) -> rquickjs::Result<()> {
        self.graphic_proxy
            .destroy_widget(self.widget_id)
            .await
            .map_err(|e| ctx.throw(rquickjs::String::from_str(ctx.clone(), &e.to_string())?.into()))?;

        Ok(())
    }

    // Callback methods - register interest in GraphicProxy
    #[qjs(rename = "onClick")]
    pub fn on_click<'js>(&self, ctx: Ctx<'js>, handler: Function<'js>) -> rquickjs::Result<()> {
        // Register callback in JavaScript event system
        let event_name = format!("widget:{}:click", self.widget_id);
        // ... handler registration ...
        Ok(())
    }
}

// WindowJS extension for widgets
#[rquickjs::methods]
impl WindowJS {
    // ... existing methods ...

    #[qjs(rename = "createWidget")]
    pub async fn create_widget<'js>(
        &self,
        ctx: Ctx<'js>,
        widget_type: String,
        config: Object<'js>,
    ) -> rquickjs::Result<WidgetJS> {
        let wtype = WidgetType::from_str(&widget_type)
            .map_err(|_| ctx.throw(
                rquickjs::String::from_str(ctx.clone(), &format!("Unknown widget type: {}", widget_type))?.into()
            ))?;

        let parent_id = config.get::<_, Option<u64>>("parent")?;
        let widget_config = parse_widget_config(&ctx, &config)?;

        let widget_id = self.graphic_proxy
            .create_widget(self.window_id, wtype, parent_id, widget_config)
            .await
            .map_err(|e| ctx.throw(rquickjs::String::from_str(ctx.clone(), &e.to_string())?.into()))?;

        Ok(WidgetJS {
            widget_id,
            window_id: self.window_id,
            widget_type: wtype,
            graphic_proxy: Arc::clone(&self.graphic_proxy),
        })
    }

    #[qjs(rename = "getWidget")]
    pub fn get_widget(&self, widget_id: u64) -> Option<WidgetJS> {
        // ... implementation ...
    }

    #[qjs(rename = "clearWidgets")]
    pub async fn clear_widgets<'js>(&self, ctx: Ctx<'js>) -> rquickjs::Result<()> {
        // ... implementation ...
    }
}
```

---

## 7. Event and Callback System

### 7.1 Widget Event Flow (Language-Agnostic)

```
┌─────────────────────────────────────────────────────┐
│                   Bevy Main Thread                  │
│  ┌──────────────────────────────────────────────┐  │
│  │ handle_widget_interactions()                 │  │
│  │  • Detects Interaction changes               │  │
│  │  • Sends WidgetEvent::Clicked                │  │
│  └──────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────┘
           ↓ event_tx (tokio::mpsc)
┌─────────────────────────────────────────────────────┐
│                  Worker Thread                      │
│  ┌──────────────────────────────────────────────┐  │
│  │ Main Event Loop                              │  │
│  │  • Receives WidgetEvent                      │  │
│  │  • Calls RuntimeManager::dispatch_event()   │  │
│  └──────────────────────────────────────────────┘  │
│           ↓                                         │
│  ┌──────────────────────────────────────────────┐  │
│  │ Runtime Adapter (JS, Lua, C#, etc.)          │  │
│  │  • Finds registered callbacks for widget_id  │  │
│  │  • Executes handler in specific language     │  │
│  └──────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────┘
```

### 7.2 Callback Registry Trait (Language-Agnostic)

Each runtime adapter implements its own callback registry, but all follow the same pattern:

```rust
// In stam_mod_runtimes/src/api/graphic/callbacks.rs

/// Trait that each runtime adapter must implement to handle widget callbacks
pub trait WidgetCallbackDispatcher: Send + Sync {
    /// Register interest in an event type for a widget
    fn subscribe(&mut self, widget_id: u64, event_type: WidgetEventType);

    /// Remove interest in an event type
    fn unsubscribe(&mut self, widget_id: u64, event_type: WidgetEventType);

    /// Dispatch a widget event - the specific runtime handles the callback
    fn dispatch(&self, event: &WidgetEvent);

    /// Clean up all callbacks for a widget (called when widget is destroyed)
    fn cleanup_widget(&mut self, widget_id: u64);
}

/// Shared base registry (only subscription tracking, no callbacks)
#[derive(Default)]
pub struct WidgetSubscriptionRegistry {
    subscriptions: HashMap<u64, HashSet<WidgetEventType>>,
}

impl WidgetSubscriptionRegistry {
    pub fn subscribe(&mut self, widget_id: u64, event_type: WidgetEventType) {
        self.subscriptions
            .entry(widget_id)
            .or_default()
            .insert(event_type);
    }

    pub fn is_subscribed(&self, widget_id: u64, event_type: WidgetEventType) -> bool {
        self.subscriptions
            .get(&widget_id)
            .map(|set| set.contains(&event_type))
            .unwrap_or(false)
    }
}
```

### 7.3 Event Dispatch in Main Loop

```rust
// In apps/stam_client/src/main.rs

fn handle_graphic_event(
    event: GraphicEvent,
    runtime_manager: &mut RuntimeManager,
    widget_callback_registry: &WidgetCallbackRegistry,
) {
    match event {
        GraphicEvent::WidgetClicked { window_id, widget_id, x, y, button } => {
            let click_event = ClickEvent { widget_id, window_id, x, y, button };

            // Dispatch to JavaScript runtime
            runtime_manager.dispatch_widget_event(
                "click",
                widget_id,
                click_event,
            );
        }
        GraphicEvent::WidgetHovered { window_id, widget_id, entered, x, y } => {
            let hover_event = HoverEvent { widget_id, window_id, entered, x, y };

            runtime_manager.dispatch_widget_event(
                "hover",
                widget_id,
                hover_event,
            );
        }
        // ... other events
    }
}
```

---

## 8. Implementation Plan

### Phase 1: Core API (Language-Agnostic) - High Priority

#### 1.1 Core Type Definitions
- [ ] Create `widget.rs` in `stam_mod_runtimes/src/api/graphic/`
  - [ ] `WidgetType` enum
  - [ ] `WidgetConfig` struct (serializable)
  - [ ] `SizeValue`, `EdgeInsets` (layout types)
  - [ ] `ColorValue` with RGBA support and hex/rgba() parsing
  - [ ] `BlendMode` enum for blend effects
  - [ ] `ImageConfig` struct (path, scaleMode, tint, opacity, flip, sourceRect)
  - [ ] `ImageScaleMode` enum (Fill, Fit, Stretch, None, Tile, NineSlice)
  - [ ] `FontConfig` struct (family, size, weight, style, letterSpacing, lineHeight)
  - [ ] `FontWeight`, `FontStyle` enums
  - [ ] `ShadowConfig` struct for text/widget shadows
  - [ ] `RectValue` for sprite sheet regions
  - [ ] `PropertyValue` enum for dynamic updates
  - [ ] `WidgetInfo` struct for queries
  - [ ] `WidgetEventType` enum

#### 1.2 Commands and Events
- [ ] Extend `GraphicCommand` in `commands.rs`
  - [ ] `CreateWidget`
  - [ ] `UpdateWidgetProperty`
  - [ ] `UpdateWidgetConfig`
  - [ ] `DestroyWidget`
  - [ ] `ReparentWidget`
  - [ ] `ClearWindowWidgets`
  - [ ] `SubscribeWidgetEvents`
  - [ ] `UnsubscribeWidgetEvents`
  - [ ] `LoadFont` (path, alias)
  - [ ] `UnloadFont` (alias)
  - [ ] `PreloadImage` (path)
- [ ] Create `WidgetEvent` enum in `events.rs`
  - [ ] `Created`, `Destroyed`
  - [ ] `Clicked`, `Hovered`, `Focused`

#### 1.3 GraphicProxy Extension
- [ ] Async widget methods in `proxy.rs`
  - [ ] `create_widget()`
  - [ ] `update_widget_property()`, `update_widget_config()`
  - [ ] `destroy_widget()`, `clear_window_widgets()`
  - [ ] `reparent_widget()`
  - [ ] `subscribe_widget_events()`, `unsubscribe_widget_events()`
- [ ] Asset management methods
  - [ ] `load_font()`, `unload_font()`, `get_loaded_fonts()`
  - [ ] `preload_image()`
- [ ] Sync query methods
  - [ ] `get_widget_info()`, `get_window_widgets()`
  - [ ] `get_window_root_widget()`
- [ ] Add `next_widget_id: AtomicU64`
- [ ] Add `widgets: Arc<RwLock<HashMap<u64, WidgetInfo>>>`
- [ ] Add `loaded_fonts: Arc<RwLock<HashMap<String, FontInfo>>>`

### Phase 2: Bevy Implementation - High Priority

#### 2.1 Widget Registry and Asset Registry
- [ ] Create `WidgetRegistry` resource
- [ ] Create `FontRegistry` resource (alias → Handle<Font>)
- [ ] Create `ImageCache` resource (path → Handle<Image>)
- [ ] Create `StamWidget` component (marker)
- [ ] Create `WidgetEventSubscriptions` component
- [ ] Create `ButtonColors` component (normal, hover, pressed, disabled)
- [ ] Create `WidgetOpacity` component (for hierarchical transparency management)

#### 2.2 Widget Command System
- [ ] Implement handling in `process_commands` or new system
- [ ] `create_widget_entity()` helper for each `WidgetType`
- [ ] Parent-child hierarchy management with Bevy relations
- [ ] Handling `LoadFont`, `UnloadFont`, `PreloadImage` commands

#### 2.3 Advanced Rendering System
- [ ] System for applying `opacity` to widgets and children
- [ ] System for applying `BlendMode` (requires custom shader or bevy_blend_modes)
- [ ] System for rendering `background_image` with all options
- [ ] System for 9-slice scaling

#### 2.4 Interaction System
- [ ] `handle_widget_interactions` system
- [ ] Query on `Changed<Interaction>` for widgets
- [ ] Send events only for widgets with active subscriptions
- [ ] Automatic button color management (including disabled)

### Phase 3: JavaScript Binding - High Priority

#### 3.1 JS-specific Callback Registry
- [ ] Create `JsWidgetCallbackRegistry` in `adapters/js/`
- [ ] Implement `WidgetCallbackDispatcher` trait
- [ ] Handle `PersistentFunction` for callbacks

#### 3.2 WidgetJS Class
- [ ] Properties: `id`, `type`, `windowId`
- [ ] Methods: `setProperty()`, `destroy()`
- [ ] Callbacks: `onClick()`, `onHover()`, `onFocus()`
- [ ] Removal: `removeOnClick()`, `removeOnHover()`, `removeOnFocus()`

#### 3.3 WindowJS Extension
- [ ] `createWidget(type, config)`
- [ ] `getWidget(id)`, `getWidgetsByType(type)`
- [ ] `getRootWidget()`, `clearWidgets()`

#### 3.4 GraphicJS Extension
- [ ] `loadFont(path, alias)` - load custom font
- [ ] `unloadFont(alias)` - unload font
- [ ] `getLoadedFonts()` - list loaded fonts
- [ ] `preloadImage(path)` - preload image

#### 3.5 Color and Config Parsing
- [ ] Parser for colors: "#RGB", "#RGBA", "#RRGGBB", "#RRGGBBAA", "rgba(r,g,b,a)"
- [ ] Parser for `FontConfig` from JS object
- [ ] Parser for `ImageConfig` from JS object
- [ ] Parser for `ShadowConfig` from JS object

### Phase 4: Specific Widgets - Medium Priority

#### 4.1 Container Widget
- [ ] Flex layout (direction, justify, align, gap)
- [ ] Basic grid layout (rows, columns)
- [ ] Background color with alpha
- [ ] Background image with all options
- [ ] Global opacity

#### 4.2 Text Widget
- [ ] Content, fontColor with alpha
- [ ] Complete FontConfig (family, size, weight, style)
- [ ] Letter spacing and line height
- [ ] TextAlign
- [ ] Text shadow with blur
- [ ] Dynamic content update

#### 4.3 Button Widget
- [ ] Label with FontConfig
- [ ] Colors with alpha: normal, hover, pressed, disabled
- [ ] Background image for states
- [ ] Disabled state
- [ ] Automatic interaction
- [ ] Border radius

#### 4.4 Panel Widget
- [ ] Background color with alpha
- [ ] Background image (fill, tile, 9-slice)
- [ ] Border (color with alpha, width, radius)
- [ ] Global opacity

#### 4.5 Image Widget
- [ ] Loading from asset path
- [ ] Scale modes (Fill, Fit, Stretch, None, Tile, NineSlice)
- [ ] Tint color with alpha
- [ ] Opacity
- [ ] Flip X/Y
- [ ] Source rect for sprite sheets

### Phase 5: Other Runtime Preparation - Low Priority

#### 5.1 Binding Documentation
- [ ] Document how to implement `WidgetCallbackDispatcher`
- [ ] Template for new runtime adapter

#### 5.2 Trait Bounds
- [ ] Verify all core types are `Serialize + Deserialize`
- [ ] Verify thread-safety (`Send + Sync`)

### Phase 6: Testing and Documentation - Medium Priority

#### 6.1 Manual Testing
- [ ] Create demo mod with complex UI
- [ ] Test widget creation/destruction
- [ ] Test callbacks on all types
- [ ] Test hierarchy (parent-child, reparent)

#### 6.2 Documentation
- [ ] API reference for core types
- [ ] Examples for JavaScript
- [ ] Guide for implementing bindings in other languages

---

## References

### Bevy UI Documentation
- [Bevy Window Struct](https://docs.rs/bevy/latest/bevy/window/struct.Window.html)
- [Bevy UI Overview](https://taintedcoders.com/bevy/ui)
- [Bevy Widgets Discussion](https://github.com/bevyengine/bevy/discussions/5604)

### Alternative Libraries (for future reference)
- [bevy_egui](https://docs.rs/bevy_egui/latest/bevy_egui/) - Egui Integration
- [Sickle UI](https://github.com/UmbraLuminworksai/sickle_ui) - Ergonomic UI for Bevy

### Architectural Patterns
- **Language-agnostic core**: API defined in Rust, bindings for each language
- **Entity-based widgets**: Widgets are ECS entities with marker components
- **Command-Event pattern**: Asynchronous communication between threads
- **Proxy pattern**: GraphicProxy mediates between all runtimes and Bevy
- **ID-based references**: Widgets referenced via ID, not pointers
- **Subscription-based events**: Callbacks registered as interest in events
- **Asset caching**: Fonts and images loaded once, referenced by alias/path

### Technical Notes on Transparency and Rendering

#### Alpha Blending
- All colors support alpha channel (0.0 = transparent, 1.0 = opaque)
- Widget `opacity` multiplies with child colors' alpha
- Bevy uses premultiplied alpha by default

#### BlendMode
- Requires custom shader or integration with `bevy_blend_modes`
- `Normal` is the only mode natively supported by Bevy UI
- Other modes might require render-to-texture

#### Font Loading
- Bevy supports .ttf and .otf
- Fonts loaded via AssetServer
- Alias allows simple reference in widgets

#### 9-Slice Scaling
- Bevy 0.15+ supports `ImageScaleMode::Sliced` for UI
- Preserves corners and borders during resizing
- Ideal for dialog boxes, panels, styled buttons
