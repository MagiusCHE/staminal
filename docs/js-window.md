# Window API Reference

The Window API provides methods for creating and controlling application windows from JavaScript mods.

## Overview

The Window API consists of two main components:
- **`window`** - Global factory object for getting/creating windows
- **`WindowHandle`** - Object returned by factory methods with integrated control methods

## Quick Start

```javascript
// Get the main window (created hidden at startup)
const main = window.get_main_window();

// Configure the window
main.set_title("My Game");
main.set_size(1920, 1080);
main.set_resizable(true);

// Show the window
main.show();

// Create an additional window
const settings = window.create("Settings", 400, 300, true);
settings.show();
```

---

## Global `window` Object

The `window` global object provides factory methods for obtaining window handles.

### Methods

#### `window.get_main_window()`

Returns a handle to the main application window.

The main window is created hidden at startup. Use `.show()` to make it visible.

**Returns:** `WindowHandle` - Handle object with methods to control the window

**Example:**
```javascript
const main = window.get_main_window();
main.set_title("Staminal: My Game");
main.show();
```

---

#### `window.create(title, width, height, resizable?)`

Creates a new additional window.

**Parameters:**
| Name | Type | Required | Default | Description |
|------|------|----------|---------|-------------|
| `title` | `string` | Yes | - | Window title |
| `width` | `number` | Yes | - | Window width in pixels |
| `height` | `number` | Yes | - | Window height in pixels |
| `resizable` | `boolean` | No | `true` | Whether the window is resizable |

**Returns:** `WindowHandle` - Handle object with methods to control the window

**Example:**
```javascript
// Create a resizable settings window
const settings = window.create("Settings", 400, 300, true);

// Create a fixed-size popup
const popup = window.create("Alert", 200, 100, false);
```

---

## `WindowHandle` Object

A `WindowHandle` is returned by `window.get_main_window()` and `window.create()`. It provides methods to control the associated window.

### Properties

#### `id`

The unique identifier of the window.

**Type:** `number`

- Main window always has `id = 0`
- Additional windows have `id >= 1`

**Example:**
```javascript
const main = window.get_main_window();
console.log(main.id); // 0

const popup = window.create("Popup", 400, 300);
console.log(popup.id); // 1, 2, 3, ...
```

---

### Methods

#### `set_title(title)`

Sets the window title.

**Parameters:**
| Name | Type | Description |
|------|------|-------------|
| `title` | `string` | The new window title |

**Returns:** `void`

**Example:**
```javascript
const main = window.get_main_window();
main.set_title("My Game - Level 1");
```

---

#### `set_size(width, height)`

Sets the window size in pixels.

**Parameters:**
| Name | Type | Description |
|------|------|-------------|
| `width` | `number` | Window width in pixels |
| `height` | `number` | Window height in pixels |

**Returns:** `void`

**Example:**
```javascript
const main = window.get_main_window();
main.set_size(1920, 1080);
```

---

#### `get_size()`

Gets the current window size.

> **Note:** Currently only accurate for the main window.

**Returns:** `object` - Object with `width` and `height` properties

**Example:**
```javascript
const main = window.get_main_window();
const size = main.get_size();
console.log(`Window size: ${size.width}x${size.height}`);
```

---

#### `set_resizable(resizable)`

Sets whether the window can be resized by the user.

**Parameters:**
| Name | Type | Description |
|------|------|-------------|
| `resizable` | `boolean` | `true` to allow resizing, `false` to prevent it |

**Returns:** `void`

**Example:**
```javascript
const main = window.get_main_window();
main.set_resizable(false); // Lock window size
```

---

#### `set_fullscreen(fullscreen)`

Sets fullscreen mode.

**Parameters:**
| Name | Type | Description |
|------|------|-------------|
| `fullscreen` | `boolean` | `true` for fullscreen, `false` for windowed |

**Returns:** `void`

**Example:**
```javascript
const main = window.get_main_window();
main.set_fullscreen(true);

// Toggle fullscreen
// main.set_fullscreen(!isFullscreen);
```

---

#### `show()`

Makes the window visible.

**Returns:** `void`

**Example:**
```javascript
const main = window.get_main_window();
main.set_title("My Game");
main.set_size(1280, 720);
main.show(); // Window appears on screen
```

---

#### `hide()`

Hides the window (makes it invisible).

**Returns:** `void`

**Example:**
```javascript
const settings = window.create("Settings", 400, 300);
settings.show();

// Later, hide it
settings.hide();
```

---

#### `close()`

Requests the window to close.

**Returns:** `void`

**Example:**
```javascript
const popup = window.create("Popup", 200, 100);
popup.show();

// Close the popup after 3 seconds
setTimeout(() => {
    popup.close();
}, 3000);
```

---

## Complete Example

```javascript
// Bootstrap mod that initializes the game window
export function onBootstrap() {
    console.log("Initializing game window...");

    // Get and configure the main window
    const main = window.get_main_window();
    main.set_title("Staminal: My Awesome Game");
    main.set_size(1280, 720);
    main.set_resizable(true);
    main.show();

    console.log(`Main window created with id: ${main.id}`);

    // Create a debug console window (optional)
    if (DEBUG_MODE) {
        const debug = window.create("Debug Console", 600, 400, true);
        debug.show();
    }
}

// Handle settings menu
function openSettings() {
    const settings = window.create("Settings", 500, 400, false);
    settings.show();
    return settings;
}

function closeSettings(settingsWindow) {
    settingsWindow.close();
}
```

---

## Notes

- The main window (id=0) is always created hidden at startup
- You must call `.show()` on the main window to make it visible
- Additional windows created with `window.create()` are visible by default
- Window handles can be stored and reused throughout your mod's lifecycle
- Closing the main window will trigger application shutdown
