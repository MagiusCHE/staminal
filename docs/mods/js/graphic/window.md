# JavaScript API: Graphic Windows and Widgets

This document provides the JavaScript API reference for window and widget management in Staminal mods.

> **Note**: All graphic APIs are **client-only**. Calling them on the server throws an error.

## Quick Start

```javascript
// In your mod's onBootstrap or GraphicEngineReady handler
System.registerEvent(System.GraphicEngineReady, async (req, res) => {
    // Get the main window (created by enableEngine)
    const info = await Graphic.getEngineInfo();
    const window = info.mainWindow;

    // Create a simple UI
    const container = await window.createWidget(WidgetTypes.Container, {
        width: "100%",
        height: "100%",
        backgroundColor: "#1a1a2e"
    });

    const title = await container.createChild(WidgetTypes.Text, {
        content: "Hello, Staminal!",
        fontColor: "#ffffff",
        font: { size: 32 }
    });
});

// Enable the graphic engine (triggers GraphicEngineReady)
await Graphic.enableEngine(GraphicEngines.Bevy, {
    window: {
        title: "My Game",
        width: 1280,
        height: 720
    }
});
```

---

## Global Objects

### `Graphic`

The main graphic API object, available globally in all mods.

### `GraphicEngines`

Enum for supported graphic engines:

| Constant | Value | Description |
|----------|-------|-------------|
| `GraphicEngines.Bevy` | `0` | Bevy game engine (supported) |
| `GraphicEngines.Wgpu` | `1` | WGPU renderer (planned) |
| `GraphicEngines.Terminal` | `2` | Terminal UI (planned) |

### `WidgetTypes`

Enum for widget types:

| Constant | Value | Description |
|----------|-------|-------------|
| `WidgetTypes.Container` | `0` | Flexbox/grid layout container |
| `WidgetTypes.Text` | `1` | Text display |
| `WidgetTypes.Button` | `2` | Clickable button |
| `WidgetTypes.Image` | `3` | Image display |
| `WidgetTypes.Panel` | `4` | Panel with background |

### `WindowPositionModes`

Enum for window positioning:

| Constant | Value | Description |
|----------|-------|-------------|
| `WindowPositionModes.Default` | `0` | OS default positioning |
| `WindowPositionModes.Centered` | `1` | Center on screen |

### `WindowModes`

Enum for window display modes:

| Constant | Value | Description |
|----------|-------|-------------|
| `WindowModes.Windowed` | `0` | Normal windowed mode |
| `WindowModes.Fullscreen` | `1` | Exclusive fullscreen mode |
| `WindowModes.BorderlessFullscreen` | `2` | Borderless fullscreen (covers entire screen) |

### `ImageScaleModes`

Enum for image scaling modes:

| Constant | Value | Description |
|----------|-------|-------------|
| `ImageScaleModes.Auto` | `0` | Image uses its natural dimensions (default) |
| `ImageScaleModes.Stretch` | `1` | Stretch to fill container (ignores aspect ratio) |
| `ImageScaleModes.Tiled` | `2` | Repeat image as pattern |
| `ImageScaleModes.Sliced` | `3` | 9-slice scaling (preserves corners for UI elements) |
| `ImageScaleModes.Contain` | `4` | Scale to fit within bounds, maintaining aspect ratio (may letterbox) |
| `ImageScaleModes.Cover` | `5` | Scale to cover entire area, maintaining aspect ratio (may crop) |

---

## `Graphic` Object

### `Graphic.enableEngine(engineType, config?)`

Enables a graphic engine and creates the main window.

**Parameters:**
- `engineType`: `GraphicEngines` - Engine to use
- `config?`: `object` - Optional configuration
  - `window`: `object` - Main window configuration
    - `title`: `string` (default: `"Staminal"`)
    - `width`: `number` (default: `1280`)
    - `height`: `number` (default: `720`)
    - `resizable`: `boolean` (default: `true`)
    - `fullscreen`: `boolean` (default: `false`)
    - `positionMode`: `WindowPositionModes` (default: `Centered`)

**Returns:** `Promise<void>`

**Example:**
```javascript
await Graphic.enableEngine(GraphicEngines.Bevy, {
    window: {
        title: "My Awesome Game",
        width: 1920,
        height: 1080,
        resizable: false,
        fullscreen: true
    }
});
```

---

### `Graphic.isEngineEnabled()`

Checks if a graphic engine is currently enabled.

**Returns:** `boolean`

```javascript
if (Graphic.isEngineEnabled()) {
    console.log("Engine is ready!");
}
```

---

### `Graphic.getEngine()`

Gets the currently active engine type.

**Returns:** `number | null` - `GraphicEngines` value or `null`

```javascript
const engine = Graphic.getEngine();
if (engine === GraphicEngines.Bevy) {
    console.log("Using Bevy engine");
}
```

---

### `Graphic.getEngineInfo()`

Gets detailed information about the active engine.

**Returns:** `Promise<object>`
- `engineType`: `string` - Engine name (e.g., `"Bevy"`)
- `engineTypeId`: `number` - `GraphicEngines` value
- `name`: `string` - Library name
- `version`: `string` - Library version
- `description`: `string` - Engine description
- `features`: `string[]` - Enabled features
- `backend`: `string` - Rendering backend (e.g., `"Vulkan"`)
- `supports2d`: `boolean`
- `supports3d`: `boolean`
- `supportsUi`: `boolean`
- `supportsAudio`: `boolean`
- `mainWindow`: `Window` - The main window object

**Example:**
```javascript
const info = await Graphic.getEngineInfo();
console.log(`Using ${info.name} v${info.version}`);
console.log(`Backend: ${info.backend}`);
console.log(`Features: ${info.features.join(", ")}`);

// Access main window
await info.mainWindow.setTitle("Updated Title");
```

---

### `Graphic.getWindows()`

Gets all windows managed by the engine.

**Returns:** `Window[]`

```javascript
const windows = Graphic.getWindows();
for (const win of windows) {
    console.log(`Window ${win.id}`);
}
```

---

### `Graphic.getPrimaryScreen()`

Gets the primary screen/monitor identifier.

**Returns:** `Promise<number>` - Screen ID (0 for primary, or hash-based ID)

```javascript
const screenId = await Graphic.getPrimaryScreen();
console.log(`Primary screen ID: ${screenId}`);
```

---

### `Graphic.getScreenResolution(screenId)`

Gets the resolution of a specific screen/monitor.

**Parameters:**
- `screenId`: `number` - Screen identifier (from `getPrimaryScreen()` or 0 for primary)

**Returns:** `Promise<object>`
- `width`: `number` - Screen width in pixels
- `height`: `number` - Screen height in pixels

**Example:**
```javascript
const screen = await Graphic.getPrimaryScreen();
const resolution = await Graphic.getScreenResolution(screen);
console.log(`Screen resolution: ${resolution.width}x${resolution.height}`);

// Or directly use 0 for primary screen
const primaryRes = await Graphic.getScreenResolution(0);
console.log(`Primary screen: ${primaryRes.width}x${primaryRes.height}`);
```

---

### `Graphic.createWindow(config?)`

Creates a new window.

**Parameters:**
- `config?`: `object` - Window configuration
  - `title`: `string` (default: `"Staminal"`)
  - `width`: `number` (default: `1280`)
  - `height`: `number` (default: `720`)
  - `fullscreen`: `boolean` (default: `false`)
  - `resizable`: `boolean` (default: `true`)
  - `visible`: `boolean` (default: `true`)
  - `positionMode`: `WindowPositionModes` (default: `Centered`)

**Returns:** `Promise<Window>`

**Example:**
```javascript
const debugWindow = await Graphic.createWindow({
    title: "Debug Console",
    width: 600,
    height: 400,
    resizable: true
});
```

---

### `Graphic.setMainWindow(window)`

Promotes a window to be the "main" window.

After this call:
- `Graphic.getEngineInfo().mainWindow` will return this window
- Window close events will reflect the new main window

This is useful when you create a new window to replace the initial loading/splash window and want to promote it as the main game window.

**Parameters:**
- `window`: `Window` - The window object to set as the main window

**Throws:** Error if called on server or if the window is invalid

**Example:**
```javascript
// Create a new game window
const gameWindow = await Graphic.createWindow({
    title: "My Game",
    width: 1920,
    height: 1080,
    resizable: true
});

// Promote it as the main window
Graphic.setMainWindow(gameWindow);

// Now getEngineInfo().mainWindow returns gameWindow
const info = await Graphic.getEngineInfo();
console.log(info.mainWindow.id === gameWindow.id); // true

// Close the old splash/loading window
await oldWindow.close();
```

---

### `Graphic.loadFont(alias, path)`

Loads a custom font from a file.

**Parameters:**
- `alias`: `string` - Name to reference this font
- `path`: `string` - Path to font file (use `System.getAssetsPath()`)

**Returns:** `Promise<string>` - The assigned alias

**Example:**
```javascript
const fontPath = System.getAssetsPath("fonts/Roboto-Bold.ttf");
await Graphic.loadFont("roboto-bold", fontPath);

// Use in widgets
const text = await window.createWidget(WidgetTypes.Text, {
    content: "Bold Text",
    font: { family: "roboto-bold", size: 24 }
});
```

---

### `Graphic.unloadFont(alias)`

Unloads a previously loaded font.

**Parameters:**
- `alias`: `string` - Font alias to unload

**Returns:** `Promise<void>`

```javascript
await Graphic.unloadFont("roboto-bold");
```

---

## `Window` Object

Returned by `Graphic.createWindow()` or `Graphic.getEngineInfo().mainWindow`.

### Properties

| Property | Type | Description |
|----------|------|-------------|
| `id` | `number` | Unique window ID (read-only) |

---

### `window.setSize(width, height)`

Sets the window size.

**Parameters:**
- `width`: `number` - Width in pixels
- `height`: `number` - Height in pixels

**Returns:** `Promise<void>`

```javascript
await window.setSize(1920, 1080);
```

---

### `window.setTitle(title)`

Sets the window title.

**Parameters:**
- `title`: `string` - New title

**Returns:** `Promise<void>`

```javascript
await window.setTitle("Level 2 - The Dark Forest");
```

---

### `window.setMode(mode)`

Sets the window display mode.

**Parameters:**
- `mode`: `WindowModes` - The display mode to set:
  - `WindowModes.Windowed` (0) - Normal windowed mode
  - `WindowModes.Fullscreen` (1) - Exclusive fullscreen mode
  - `WindowModes.BorderlessFullscreen` (2) - Borderless fullscreen

**Returns:** `Promise<void>`

```javascript
// Set to borderless fullscreen
await window.setMode(WindowModes.BorderlessFullscreen);

// Set to exclusive fullscreen
await window.setMode(WindowModes.Fullscreen);

// Set back to windowed mode
await window.setMode(WindowModes.Windowed);
```

---

### `window.setVisible(visible)`

Shows or hides the window.

**Parameters:**
- `visible`: `boolean` - `true` to show, `false` to hide

**Returns:** `Promise<void>`

```javascript
await window.setVisible(false); // Hide
await window.setVisible(true);  // Show
```

---

### `window.setFont(family, size)`

Sets the default font for all widgets in this window.

**Parameters:**
- `family`: `string` - Font alias (loaded via `Graphic.loadFont()`)
- `size`: `number` - Font size in pixels

**Returns:** `Promise<void>`

```javascript
await Graphic.loadFont("game-font", System.getAssetsPath("fonts/Game.ttf"));
await window.setFont("game-font", 16);
```

---

### `window.getTitle()`

Gets the current window title.

**Returns:** `string` - The window title

```javascript
const title = window.getTitle();
console.log(`Current title: ${title}`);
```

---

### `window.getSize()`

Gets the current window size.

**Returns:** `object`
- `width`: `number` - Width in pixels
- `height`: `number` - Height in pixels

```javascript
const size = window.getSize();
console.log(`Window size: ${size.width}x${size.height}`);
```

---

### `window.getMode()`

Gets the current window display mode.

**Returns:** `number` - `WindowModes` value (0=Windowed, 1=Fullscreen, 2=BorderlessFullscreen)

```javascript
const mode = window.getMode();
if (mode === WindowModes.Fullscreen) {
    console.log("Window is in fullscreen mode");
} else if (mode === WindowModes.BorderlessFullscreen) {
    console.log("Window is in borderless fullscreen mode");
} else {
    console.log("Window is in windowed mode");
}
```

---

### `window.close()`

Closes the window.

**Returns:** `Promise<void>`

```javascript
await debugWindow.close();
```

---

### `window.createWidget(widgetType, config?)`

Creates a widget in the window.

**Parameters:**
- `widgetType`: `WidgetTypes` - Type of widget
- `config?`: `object` - Widget configuration (see Widget Config below)

**Returns:** `Promise<Widget>`

```javascript
const panel = await window.createWidget(WidgetTypes.Panel, {
    width: "100%",
    height: "100%",
    backgroundColor: "#2d2d44"
});
```

---

### `window.clearWidgets()`

Destroys all widgets in the window.

**Returns:** `Promise<void>`

```javascript
await window.clearWidgets();
```

---

## `Widget` Object

Returned by `window.createWidget()` or `widget.createChild()`.

### Properties

| Property | Type | Description |
|----------|------|-------------|
| `id` | `number` | Unique widget ID (read-only) |
| `windowId` | `number` | Parent window ID (read-only) |

---

### `widget.createChild(widgetType, config?)`

Creates a child widget.

**Parameters:**
- `widgetType`: `WidgetTypes` - Type of widget
- `config?`: `object` - Widget configuration

**Returns:** `Promise<Widget>`

```javascript
const container = await window.createWidget(WidgetTypes.Container, {
    direction: "column",
    gap: 10
});

const title = await container.createChild(WidgetTypes.Text, {
    content: "Menu",
    font: { size: 24 }
});

const button = await container.createChild(WidgetTypes.Button, {
    label: "Start Game"
});
```

---

### `widget.setContent(content)`

Sets text content (for Text widgets) or label (for Button widgets).

**Parameters:**
- `content`: `string` - New text content

**Returns:** `Promise<void>`

```javascript
await textWidget.setContent("Score: 1000");
await buttonWidget.setContent("Continue");
```

---

### `widget.setBackgroundColor(color)`

Sets the background color.

**Parameters:**
- `color`: `string | object` - Color value

**Returns:** `Promise<void>`

```javascript
// String formats
await widget.setBackgroundColor("#ff0000");          // Hex RGB
await widget.setBackgroundColor("#ff0000aa");        // Hex RGBA
await widget.setBackgroundColor("rgba(255,0,0,0.5)"); // RGBA function

// Object format
await widget.setBackgroundColor({ r: 1.0, g: 0.0, b: 0.0, a: 0.5 });
```

---

### `widget.setProperty(property, value)`

Sets a widget property dynamically.

**Parameters:**
- `property`: `string` - Property name
- `value`: `any` - Property value

**Returns:** `Promise<void>`

**Supported properties:**
- `content`, `label` - Text content
- `width`, `height`, `minWidth`, `maxWidth`, `minHeight`, `maxHeight` - Dimensions
- `backgroundColor`, `borderColor`, `fontColor`, `hoverColor`, `pressedColor` - Colors
- `disabled` - Boolean state
- `opacity` - Number (0.0 - 1.0)

```javascript
await widget.setProperty("content", "Updated text");
await widget.setProperty("width", "50%");
await widget.setProperty("backgroundColor", "#00ff00");
await widget.setProperty("disabled", true);
await widget.setProperty("opacity", 0.8);
```

---

### `widget.on(eventType, callback)`

Subscribes to widget events.

**Parameters:**
- `eventType`: `string` - Event type: `"click"`, `"hover"`, `"focus"`
- `callback`: `function` - Event handler

**Returns:** `Promise<void>`

**Event data passed to callback:**
- `click`: `{ widgetId, x, y, button }`
- `hover`: `{ widgetId, entered, x, y }`
- `focus`: `{ widgetId, focused }`

```javascript
await button.on("click", (event) => {
    console.log(`Clicked at ${event.x}, ${event.y}`);
    console.log(`Button: ${event.button}`); // "left", "right", "middle"
});

await panel.on("hover", (event) => {
    if (event.entered) {
        console.log("Mouse entered");
    } else {
        console.log("Mouse left");
    }
});

await input.on("focus", (event) => {
    console.log(event.focused ? "Focused" : "Unfocused");
});
```

---

### `widget.off(eventType)`

Unsubscribes from widget events.

**Parameters:**
- `eventType`: `string` - Event type to unsubscribe

**Returns:** `Promise<void>`

```javascript
await button.off("click");
```

---

### `widget.destroy()`

Destroys the widget and all its children.

**Returns:** `Promise<void>`

```javascript
await widget.destroy();
```

---

## Widget Configuration

All widgets accept a configuration object with the following properties:

### Layout Properties

| Property | Type | Default | Description |
|----------|------|---------|-------------|
| `layout` | `"flex"` \| `"grid"` | `"flex"` | Layout mode |
| `direction` | `"row"` \| `"column"` \| `"rowReverse"` \| `"columnReverse"` | `"row"` | Flex direction |
| `justifyContent` | `"flexStart"` \| `"flexEnd"` \| `"center"` \| `"spaceBetween"` \| `"spaceAround"` \| `"spaceEvenly"` | `"flexStart"` | Main axis alignment |
| `alignItems` | `"stretch"` \| `"flexStart"` \| `"flexEnd"` \| `"center"` \| `"baseline"` | `"stretch"` | Cross axis alignment |
| `gap` | `number` | `0` | Gap between children (px) |

### Dimension Properties

| Property | Type | Default | Description |
|----------|------|---------|-------------|
| `width` | `number` \| `string` | `auto` | Width (px or %) |
| `height` | `number` \| `string` | `auto` | Height (px or %) |
| `minWidth` | `number` \| `string` | - | Minimum width |
| `maxWidth` | `number` \| `string` | - | Maximum width |
| `minHeight` | `number` \| `string` | - | Minimum height |
| `maxHeight` | `number` \| `string` | - | Maximum height |

### Spacing Properties

| Property | Type | Default | Description |
|----------|------|---------|-------------|
| `margin` | `number` \| `[number]` \| `object` | `0` | Outer margin |
| `padding` | `number` \| `[number]` \| `object` | `0` | Inner padding |

Margin/padding formats:
```javascript
// Single value (all sides)
margin: 10

// [vertical, horizontal]
margin: [10, 20]

// [top, right, bottom, left]
margin: [10, 20, 10, 20]

// Object
margin: { top: 10, right: 20, bottom: 10, left: 20 }
```

### Appearance Properties

| Property | Type | Default | Description |
|----------|------|---------|-------------|
| `backgroundColor` | `string` | - | Background color |
| `borderColor` | `string` | - | Border color |
| `borderWidth` | `number` \| `object` | `0` | Border width |
| `borderRadius` | `number` | `0` | Corner radius |
| `opacity` | `number` | `1.0` | Opacity (0.0 - 1.0) |

### Text Properties (Text widget)

| Property | Type | Default | Description |
|----------|------|---------|-------------|
| `content` | `string` | `""` | Text content |
| `font` | `object` | - | Font configuration |
| `fontColor` | `string` | `"#ffffff"` | Text color |
| `textAlign` | `"left"` \| `"center"` \| `"right"` | `"left"` | Text alignment |

Font configuration:
```javascript
font: {
    family: "roboto-bold",  // Font alias
    size: 16,               // Size in pixels
    weight: "regular",      // "thin", "light", "regular", "medium", "bold", etc.
    style: "normal"         // "normal", "italic", "oblique"
}
```

### Button Properties

| Property | Type | Default | Description |
|----------|------|---------|-------------|
| `label` | `string` | `""` | Button text |
| `hoverColor` | `string` | - | Color on hover |
| `pressedColor` | `string` | - | Color when pressed |
| `disabled` | `boolean` | `false` | Disable button |
| `disabledColor` | `string` | - | Color when disabled |

### Image Properties

| Property | Type | Default | Description |
|----------|------|---------|-------------|
| `image` | `object` | - | Image configuration |
| `backgroundImage` | `object` | - | Background image |

Image configuration:
```javascript
image: {
    // Source (one of these is required):
    resourceId: "my-image",          // Pre-loaded resource alias (recommended)
    path: "mods/my-mod/assets/sprite.png",  // Direct path (not yet supported)

    // Scale mode (using ImageScaleModes enum or string):
    scaleMode: ImageScaleModes.Cover, // or "auto", "stretch", "contain", "cover"

    // For Tiled mode:
    scaleMode: {
        type: "Tiled",
        tileX: true,              // Tile horizontally
        tileY: true,              // Tile vertically
        stretchValue: 1.0         // Stretch factor
    },

    // For 9-slice/Sliced mode:
    scaleMode: {
        type: "Sliced",
        top: 10,                  // Border from top edge (px)
        right: 10,                // Border from right edge (px)
        bottom: 10,               // Border from bottom edge (px)
        left: 10,                 // Border from left edge (px)
        center: true              // Whether to draw center portion
    },

    tint: "#ffffff",              // Multiply color
    opacity: 1.0,
    flipX: false,
    flipY: false
}
```

**Scale Mode Values:**

| Mode | Description |
|------|-------------|
| `ImageScaleModes.Auto` | Image uses its natural dimensions |
| `ImageScaleModes.Stretch` | Stretch to fill container (ignores aspect ratio) |
| `ImageScaleModes.Tiled` | Repeat image as pattern |
| `ImageScaleModes.Sliced` | 9-slice scaling for UI elements |
| `ImageScaleModes.Contain` | Fit within bounds, maintaining aspect ratio (may letterbox) |
| `ImageScaleModes.Cover` | Cover entire area, maintaining aspect ratio (may crop) |

**Example with pre-loaded resource:**
```javascript
// First, load the resource
await Resource.load("@bme-assets/background/title.jpg", "title-bg");

// Then create the Image widget
const background = await container.createChild(WidgetTypes.Image, {
    width: "100%",
    height: "100%",
    image: {
        resourceId: "title-bg",
        scaleMode: ImageScaleModes.Cover
    }
});
```

---

## Complete Examples

### Menu Screen

```javascript
System.registerEvent(System.GraphicEngineReady, async (req, res) => {
    const info = await Graphic.getEngineInfo();
    const window = info.mainWindow;

    // Load font
    await Graphic.loadFont("title", System.getAssetsPath("fonts/Title.ttf"));

    // Main container
    const main = await window.createWidget(WidgetTypes.Container, {
        width: "100%",
        height: "100%",
        direction: "column",
        justifyContent: "center",
        alignItems: "center",
        backgroundColor: "#1a1a2e",
        gap: 20
    });

    // Title
    await main.createChild(WidgetTypes.Text, {
        content: "My Awesome Game",
        font: { family: "title", size: 48 },
        fontColor: "#eeff88"
    });

    // Button container
    const buttons = await main.createChild(WidgetTypes.Container, {
        direction: "column",
        gap: 10
    });

    // Start button
    const startBtn = await buttons.createChild(WidgetTypes.Button, {
        label: "Start Game",
        width: 200,
        height: 50,
        backgroundColor: "#4a90d9",
        hoverColor: "#5ba0e9",
        pressedColor: "#3a80c9"
    });

    await startBtn.on("click", async () => {
        console.log("Starting game...");
        await window.clearWidgets();
        // Load game scene...
    });

    // Quit button
    const quitBtn = await buttons.createChild(WidgetTypes.Button, {
        label: "Quit",
        width: 200,
        height: 50,
        backgroundColor: "#d94a4a",
        hoverColor: "#e95b5b",
        pressedColor: "#c93a3a"
    });

    await quitBtn.on("click", async () => {
        await System.exit(0);
    });
});

await Graphic.enableEngine(GraphicEngines.Bevy, {
    window: { title: "My Awesome Game", width: 1280, height: 720 }
});
```

### HUD Overlay

```javascript
async function createHUD(window) {
    // Top bar
    const topBar = await window.createWidget(WidgetTypes.Panel, {
        width: "100%",
        height: 60,
        backgroundColor: "rgba(0,0,0,0.7)",
        direction: "row",
        justifyContent: "spaceBetween",
        alignItems: "center",
        padding: [0, 20]
    });

    // Score
    const scoreText = await topBar.createChild(WidgetTypes.Text, {
        content: "Score: 0",
        fontColor: "#ffffff",
        font: { size: 24 }
    });

    // Lives
    const livesContainer = await topBar.createChild(WidgetTypes.Container, {
        direction: "row",
        gap: 5
    });

    for (let i = 0; i < 3; i++) {
        await livesContainer.createChild(WidgetTypes.Image, {
            image: {
                path: System.getAssetsPath("sprites/heart.png"),
                scaleMode: "fit"
            },
            width: 32,
            height: 32
        });
    }

    return { scoreText };
}

// Usage
const hud = await createHUD(window);
await hud.scoreText.setContent("Score: 1000");
```

### Multi-Window Application

```javascript
// Main game window
await Graphic.enableEngine(GraphicEngines.Bevy, {
    window: { title: "Game", width: 1280, height: 720 }
});

// Create debug window
const debugWin = await Graphic.createWindow({
    title: "Debug Console",
    width: 400,
    height: 300
});

const debugPanel = await debugWin.createWidget(WidgetTypes.Panel, {
    width: "100%",
    height: "100%",
    backgroundColor: "#1e1e1e",
    padding: 10
});

const debugLog = await debugPanel.createChild(WidgetTypes.Text, {
    content: "Debug output...",
    fontColor: "#00ff00",
    font: { size: 12 }
});

// Log function
async function debugPrint(message) {
    const current = debugLog.getProperty("content") || "";
    await debugLog.setContent(current + "\n" + message);
}

// Close debug window when done
await debugWin.close();
```

---

## Error Handling

All async methods can throw errors. Always use try/catch:

```javascript
try {
    await Graphic.enableEngine(GraphicEngines.Bevy);
} catch (error) {
    console.error("Failed to enable engine:", error.message);
}
```

Common errors:
- `"Graphic.* is not available on the server. This method is client-only."`
- `"No graphic engine enabled. Call Graphic.enableEngine() first."`
- `"A graphic engine is already enabled"`
- `"Invalid widget type: X"`
- `"Invalid color: ..."`

---

## See Also

- [Graphic Engine Architecture](../graphic-window.md) - Internal architecture documentation
- [Event System](../event-System.md) - Event handling patterns
