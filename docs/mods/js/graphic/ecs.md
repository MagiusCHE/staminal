# JavaScript API: ECS (Entity-Component-System)

This document provides the JavaScript API reference for the ECS system in Staminal mods.

> **Note**: All ECS APIs are **client-only**. Calling them on the server throws an error.

## Naming Conventions

The ECS API uses two different naming conventions that reflect the underlying Rust/Bevy structure:

| Level | Convention | Reason | Examples |
|-------|------------|--------|----------|
| **Component names** (root keys) | `PascalCase` | Rust struct types use PascalCase | `Node`, `BackgroundColor`, `Text`, `Transform` |
| **Component fields** (nested keys) | `snake_case` | Rust struct fields use snake_case | `flex_direction`, `justify_content`, `font_size` |

**Example:**
```javascript
{
    Node: {                          // PascalCase = component type (Bevy struct)
        width: 200,                  // snake_case = field
        height: 50,
        flex_direction: "column",    // snake_case = field
        justify_content: "center",   // snake_case = field
        align_items: "center"        // snake_case = field
    },
    BackgroundColor: "#4a90d9",      // PascalCase = component type
    Text: {                          // PascalCase = component type
        value: "Hello",              // snake_case = field
        font_size: 24                // snake_case = field
    }
}
```

This convention mirrors Bevy's Rust API, so Bevy documentation is directly applicable when looking up component fields.

---

## Quick Start

```javascript
// In your mod's GraphicEngineReady handler
System.registerEvent(System.GraphicEngineReady, async (req, res) => {
    // Spawn a UI element using ECS
    const button = await World.spawn({
        Node: {
            width: 200,
            height: 50,
            justify_content: "center",
            align_items: "center"
        },
        Button: {},
        BackgroundColor: "#4a90d9",
        HoverBackgroundColor: "#5ba0e9",
        PressedBackgroundColor: "#3a80c9",
        BorderRadius: 8
    }, window);

    // Add text as child
    const label = await World.spawn({
        Node: { width: "auto", height: "auto" },
        Text: { value: "Click Me!", font_size: 18, color: "#ffffff" }
    }, button);
});
```

---

## Global Objects

### `World`

The main entry point for ECS operations.

### `SystemBehaviors`

Enum for predefined system behaviors:

| Constant | Description |
|----------|-------------|
| `SystemBehaviors.ApplyVelocity` | Add Velocity to Transform |
| `SystemBehaviors.ApplyGravity` | Apply gravity to Velocity |
| `SystemBehaviors.ApplyFriction` | Apply friction to Velocity |
| `SystemBehaviors.RegenerateOverTime` | Increment a field over time |
| `SystemBehaviors.DecayOverTime` | Decrement a field over time |
| `SystemBehaviors.DespawnWhenZero` | Remove entity when field reaches zero |

### `FieldTypes`

Enum for component schema field types:

| Constant | Description | Example |
|----------|-------------|---------|
| `FieldTypes.Number` | Numeric value | `42`, `3.14` |
| `FieldTypes.String` | Text value | `"Hello"` |
| `FieldTypes.Bool` | Boolean | `true`, `false` |
| `FieldTypes.Vec2` | 2D vector | `{ x: 0, y: 0 }` |
| `FieldTypes.Vec3` | 3D vector | `{ x: 0, y: 0, z: 0 }` |
| `FieldTypes.Color` | Color value | `"#FF0000"` |
| `FieldTypes.Entity` | Entity reference | Entity ID (number) |
| `FieldTypes.Any` | Any JSON value | Anything |

---

## `World` Object

### `World.spawn(components?, parent?)`

Spawn a new entity with optional initial components.

**Parameters:**
- `components?`: `object` - Component names as keys, component data as values
- `parent?`: `Entity` - Parent entity (optional)

**Returns:** `Promise<Entity>` - Handle to the spawned entity

```javascript
// Spawn empty entity
const entity = await World.spawn();

// Spawn with components
const player = await World.spawn({
    Transform: { translation: { x: 100, y: 200, z: 0 } },
    Health: { current: 100, max: 100 }
});

// Spawn as child of another entity
const child = await World.spawn({
    Node: { width: 50, height: 50 },
    BackgroundColor: "#ff0000"
}, player);

// Spawn as child of a container
const uiElement = await World.spawn({
    Node: { width: "100%", height: 50 },
    BackgroundColor: "#333333"
}, container);
```

---

### `World.despawn(entityId)`

Despawn an entity by ID.

**Parameters:**
- `entityId`: `number` - Entity ID to despawn

**Returns:** `Promise<void>`

```javascript
await World.despawn(entity.id);
// or
await entity.despawn();
```

---

### `World.query(options)`

Query entities matching criteria.

**Parameters:**
- `options.withComponents`: `string[]` - Required component names
- `options.withoutComponents?`: `string[]` - Excluded component names
- `options.limit?`: `number` - Maximum results

**Returns:** `Promise<QueryResult[]>` - Array of `{ id, components }`

```javascript
const enemies = await World.query({
    withComponents: ["Transform", "Enemy"],
    withoutComponents: ["Dead"],
    limit: 50
});

for (const enemy of enemies) {
    console.log(`Enemy ${enemy.id} at`, enemy.components.Transform.translation);
}
```

---

### `World.registerComponent(name, schema?)`

Register a custom component type.

**Parameters:**
- `name`: `string` - Component type name
- `schema?`: `object` - Field name to FieldType mapping

**Returns:** `Promise<void>`

```javascript
// Register without schema
await World.registerComponent("Tag");

// Register with schema
await World.registerComponent("CharacterStats", {
    health: FieldTypes.Number,
    mana: FieldTypes.Number,
    name: FieldTypes.String,
    position: FieldTypes.Vec2
});
```

---

### `World.declareSystem(config)`

Declare a system that runs automatically each frame.

**Parameters:**
- `config.name`: `string` - Unique system name
- `config.query`: `object` - Query options
- `config.behavior?`: `SystemBehaviors` - Predefined behavior
- `config.config?`: `object` - Behavior configuration
- `config.formulas?`: `string[]` - Custom formulas
- `config.order?`: `number` - Execution order (default: 0)
- `config.enabled?`: `boolean` - Initial enabled state (default: true)

**Returns:** `Promise<void>`

```javascript
// Predefined behavior
await World.declareSystem({
    name: "movement",
    query: { withComponents: ["Transform", "Velocity"] },
    behavior: SystemBehaviors.ApplyVelocity,
    order: 0
});

// Custom formulas
await World.declareSystem({
    name: "oscillate",
    query: { withComponents: ["Transform", "Wave"] },
    formulas: [
        "Transform.y = Wave.center + sin(time * Wave.speed) * Wave.amplitude"
    ],
    order: 5
});
```

---

### `World.setSystemEnabled(name, enabled)`

Enable or disable a system.

**Parameters:**
- `name`: `string` - System name
- `enabled`: `boolean` - Enabled state

**Returns:** `Promise<void>`

```javascript
await World.setSystemEnabled("gravity", false);  // Pause gravity
await World.setSystemEnabled("gravity", true);   // Resume
```

---

### `World.removeSystem(name)`

Remove a declared system.

**Parameters:**
- `name`: `string` - System name

**Returns:** `Promise<void>`

```javascript
await World.removeSystem("gravity");
```

---

## `Entity` Object

Returned by `World.spawn()`. Represents a handle to a specific entity.

### Properties

| Property | Type | Description |
|----------|------|-------------|
| `id` | `number` | Unique entity ID (read-only) |

---

### `entity.insert(componentName, data)` or `entity.insert(components)`

Insert or replace component(s) on this entity.

**Signatures:**
- `entity.insert(componentName, data)` - Insert a single component
- `entity.insert(components)` - Batch insert multiple components

**Parameters:**
- `componentName`: `string` - Component type name
- `data`: `any` - Component data
- `components`: `object` - Object with component names as keys and data as values

**Returns:** `Promise<void>`

```javascript
// Single component
await entity.insert("Health", { current: 100, max: 100 });
await entity.insert("BackgroundColor", "#ff0000");

// Batch insert multiple components at once
await entity.insert({
    Node: { width: 200, height: 50 },
    BackgroundColor: "#ff0000",
    BorderRadius: 8
});
```

---

### `entity.update(componentName, data)` or `entity.update(components)`

Update specific fields of component(s) (merge with existing).

Unlike `insert()` which replaces the entire component, `update()` merges the provided fields with existing values.

**Signatures:**
- `entity.update(componentName, data)` - Update a single component
- `entity.update(components)` - Batch update multiple components

**Parameters:**
- `componentName`: `string` - Component type name
- `data`: `any` - Partial component data to merge
- `components`: `object` - Object with component names as keys and partial data as values

**Returns:** `Promise<void>`

```javascript
// Single component update
// Assuming entity has Node: { width: 100, height: 50, padding: 10 }
await entity.update("Node", { width: 200 });
// Result: Node: { width: 200, height: 50, padding: 10 }

// Update only Text value, keeping other fields
await entity.update("Text", { value: "New text!" });

// Batch update multiple components at once
await entity.update({
    Node: { width: "50%" },
    Text: { value: "Updated!" },
    BackgroundColor: "#00ff00"
});
```

---

### `entity.remove(componentName)`

Remove a component from this entity.

**Parameters:**
- `componentName`: `string` - Component type to remove

**Returns:** `Promise<void>`

```javascript
await entity.remove("Velocity");
```

---

### `entity.get(componentName)`

Get a component's data.

**Parameters:**
- `componentName`: `string` - Component type name

**Returns:** `Promise<any | null>` - Component data or null

```javascript
const health = await entity.get("Health");
if (health) {
    console.log(`HP: ${health.current}/${health.max}`);
}
```

---

### `entity.has(componentName)`

Check if entity has a component.

**Parameters:**
- `componentName`: `string` - Component type name

**Returns:** `Promise<boolean>`

```javascript
if (await entity.has("Flying")) {
    console.log("Entity can fly!");
}
```

---

### `entity.despawn()`

Despawn this entity.

**Returns:** `Promise<void>`

```javascript
await entity.despawn();
```

---

## Native Components

### Transform

Position, rotation, and scale in world coordinates.

```javascript
await entity.insert("Transform", {
    translation: { x: 100, y: 200, z: 0 },
    rotation: { x: 0, y: 0, z: 0, w: 1 },  // Quaternion
    scale: { x: 1, y: 1, z: 1 }
});
```

### Sprite

2D rendering properties.

```javascript
await entity.insert("Sprite", {
    color: { r: 1, g: 1, b: 1, a: 1 },
    flip_x: false,
    flip_y: false,
    custom_size: { width: 64, height: 64 }
});
```

### Visibility

Control entity visibility.

```javascript
await entity.insert("Visibility", "visible");   // Always visible
await entity.insert("Visibility", "hidden");    // Always hidden
await entity.insert("Visibility", "inherited"); // Follows parent
```

### Node

UI layout (flexbox-like).

```javascript
await entity.insert("Node", {
    width: "100%",           // "auto", "50%", 100 (px), "10vw", "10vh"
    height: 50,
    padding: 10,             // All sides, or { top, right, bottom, left }
    margin: { top: 5 },
    flex_direction: "row",   // "row", "column", "row_reverse", "column_reverse"
    justify_content: "center", // "start", "end", "center", "space_between", etc.
    align_items: "center",   // "start", "end", "center", "stretch", "baseline"
    display: "flex",         // "flex", "grid", "block", "none"
    position_type: "relative" // "relative", "absolute"
});
```

### BackgroundColor

Background color for UI elements.

```javascript
// Hex color
await entity.insert("BackgroundColor", "#3366CC");

// RGBA object
await entity.insert("BackgroundColor", { r: 0.2, g: 0.4, b: 0.8, a: 1.0 });
```

### Text

Display text in UI.

```javascript
await entity.insert("Text", {
    value: "Hello, World!",
    font_size: 24,
    color: "#ffffff",     // or { r, g, b, a }
    font: "my-font-alias" // Optional: loaded via Graphic.loadFont()
});
```

### BorderRadius

Rounded corners for UI elements.

```javascript
// All corners same
await entity.insert("BorderRadius", 10);

// Individual corners
await entity.insert("BorderRadius", {
    top_left: 10,
    top_right: 10,
    bottom_left: 5,
    bottom_right: 5
});
```

### Button

Make entity interactive (clickable). Supports an optional `on_click` handler.

```javascript
// Simple button (handle clicks via event listener)
await entity.insert("Button", {});

// Button with inline click handler
const button = await World.spawn({
    Node: { width: 200, height: 50 },
    Button: {
        on_click: async (event) => {
            console.log("Button clicked at", event.x, event.y);
        }
    },
    BackgroundColor: "#3366CC"
}, window);
```

**Button.on_click callback:**
- `event.entityId` - The clicked entity ID
- `event.x` - Mouse X position
- `event.y` - Mouse Y position

### Interaction

Automatically added when `Button` is present. Tracks interaction state.

---

## Button Event Handlers

There are two ways to handle button events:

### 1. Direct `on_click` Callback (Recommended)

Define the click handler directly in the Button component at spawn time. The callback is stored privately and invoked directly when the button is clicked - no other mod can intercept it.

```javascript
const button = await World.spawn({
    Node: { width: 200, height: 50 },
    Button: {
        on_click: (event) => {
            console.log("Clicked!", event.entityId, event.x, event.y);
            // Your click logic here
        }
    },
    BackgroundColor: "#4a90d9",
    HoverBackgroundColor: "#5ba0e9",
    BorderRadius: 8
}, window);
```

**Benefits of direct callbacks:**
- **Isolation**: Other mods cannot intercept your callbacks
- **Performance**: No global event broadcasting overhead
- **Simplicity**: No need to manage event listeners

**Callback event object:**
- `event.entityId` - The entity ID that was clicked
- `event.eventType` - Always "click" for on_click
- `event.x` - Cursor X position
- `event.y` - Cursor Y position

### 2. Global Event Listener (Legacy)

You can still use global event listeners, but other mods could potentially intercept these:

```javascript
System.registerEvent("graphic:entity:interactionChanged", async (req, res) => {
    if (req.entityId === myButton.id && req.interaction === "pressed") {
        console.log("Button clicked!");
        res.handled = true;
    }
});
```

> **Note**: The `on_click` direct callback is preferred because it provides isolation and cannot be intercepted by other mods.

---

## Button Color States (Pseudo-Components)

When spawning buttons, you can define colors for different interaction states using pseudo-components. These are processed at spawn time and configure automatic color changes.

**Available pseudo-components:**
- `HoverBackgroundColor` - Background color when hovered
- `PressedBackgroundColor` - Background color when pressed
- `DisabledBackgroundColor` - Background color when disabled
- `Disabled` - Whether the button is disabled (boolean)

**Requirements:**
- Entity must have `BackgroundColor` (used as "normal" color)
- Entity must have `Button` component

```javascript
const button = await World.spawn({
    Node: { width: 200, height: 50, justify_content: "center", align_items: "center" },
    Button: {},
    BackgroundColor: "#3366CC",           // Normal state
    HoverBackgroundColor: "#4477DD",      // When hovered
    PressedBackgroundColor: "#2255BB",    // When pressed
    DisabledBackgroundColor: "#666666",   // When disabled
    Disabled: false,                       // Initially enabled
    BorderRadius: 8
}, window);
```

**Color cascade:**
1. **Normal**: `BackgroundColor`
2. **Hovered**: `HoverBackgroundColor` or fallback to `BackgroundColor`
3. **Pressed**: `PressedBackgroundColor` or `HoverBackgroundColor` or `BackgroundColor`
4. **Disabled**: `DisabledBackgroundColor` or `BackgroundColor`

---

## Disabling Buttons

Buttons can be disabled at spawn time or dynamically at runtime.

### At Spawn Time

```javascript
const button = await World.spawn({
    Node: { width: 200, height: 50 },
    Button: {},
    BackgroundColor: "#3366CC",
    DisabledBackgroundColor: "#666666",
    Disabled: true  // Button starts disabled
}, window);
```

### At Runtime

Use `entity.update("Disabled", value)` to toggle the disabled state:

```javascript
// Disable the button
await button.update("Disabled", true);

// Enable the button
await button.update("Disabled", false);
```

When disabled:
- The button displays the `DisabledBackgroundColor` (or `BackgroundColor` if not specified)
- Hover and press color changes are ignored
- The button still receives interaction events, but you should check the disabled state in your handler

### Checking Disabled State in Event Handler

```javascript
// Track disabled state in your mod
let isButtonDisabled = false;

async function setButtonDisabled(disabled) {
    isButtonDisabled = disabled;
    await button.update("Disabled", disabled);
}

System.registerEvent("graphic:entity:interactionChanged", async (req, res) => {
    if (req.entityId === button.id && req.interaction === "pressed") {
        if (isButtonDisabled) {
            // Button is disabled, ignore the click
            return;
        }
        // Handle the click
        console.log("Button clicked!");
        res.handled = true;
    }
});
```

---

## Button Interaction Events

Handle button clicks and interaction changes using the `graphic:entity:interactionChanged` event.

```javascript
System.registerEvent("graphic:entity:interactionChanged", async (req, res) => {
    // req contains: entityId, interaction ("none", "hovered", "pressed"), x, y

    if (req.entityId === myButton.id && req.interaction === "pressed") {
        console.log("Button clicked!");
        res.handled = true;
    }
});
```

---

## Automatic Cursor Change

When hovering over any entity with `Button` and `Interaction` components, the cursor automatically changes to a pointer (hand icon). The cursor returns to the default arrow when no buttons are hovered.

This behavior is automatic and requires no configuration.

---

## Complete Examples

### Interactive Button with Label

```javascript
async function createButton(window, text, onClick) {
    const button = await World.spawn({
        Node: {
            width: 200,
            height: 50,
            justify_content: "center",
            align_items: "center"
        },
        Button: {},
        BackgroundColor: "#4a90d9",
        HoverBackgroundColor: "#5ba0e9",
        PressedBackgroundColor: "#3a80c9",
        BorderRadius: 8
    }, window);

    await World.spawn({
        Node: { width: "auto", height: "auto" },
        Text: { value: text, font_size: 18, color: "#ffffff" }
    }, button);

    // Store callback
    button._onClick = onClick;
    return button;
}

// Create buttons
const startBtn = await createButton(window, "Start Game", async () => {
    console.log("Starting game...");
});

const quitBtn = await createButton(window, "Quit", async () => {
    await System.exit(0);
});

// Handle clicks
System.registerEvent("graphic:entity:interactionChanged", async (req, res) => {
    if (req.interaction !== "pressed") return;

    for (const btn of [startBtn, quitBtn]) {
        if (req.entityId === btn.id && btn._onClick) {
            await btn._onClick();
            res.handled = true;
            break;
        }
    }
});
```

### Progress Bar

```javascript
async function createProgressBar(parent, width) {
    // Container
    const container = await World.spawn({
        Node: {
            width: width,
            height: 20,
            padding: 2
        },
        BackgroundColor: "#333333",
        BorderRadius: 4
    }, { parent });

    // Fill bar
    const fill = await World.spawn({
        Node: {
            width: "0%",
            height: "100%"
        },
        BackgroundColor: "#4a90d9",
        BorderRadius: 2
    }, container);

    return {
        container,
        fill,
        async setProgress(percent) {
            const p = Math.max(0, Math.min(100, percent));
            await fill.update("Node", { width: `${p}%` });
        }
    };
}

// Usage
const progressBar = await createProgressBar(window, 300);
await progressBar.setProgress(50);  // 50%
await progressBar.setProgress(100); // 100%
```

### Vertical Menu with Flexbox

```javascript
async function createMenu(window, items) {
    // Main container
    const menu = await World.spawn({
        Node: {
            width: 300,
            height: "auto",
            flex_direction: "column",
            align_items: "stretch",
            padding: 20
        },
        BackgroundColor: "#1a1a2e",
        BorderRadius: 12
    }, window);

    const buttons = [];

    for (const item of items) {
        const btn = await World.spawn({
            Node: {
                width: "100%",
                height: 50,
                margin: { bottom: 10 },
                justify_content: "center",
                align_items: "center"
            },
            Button: {},
            BackgroundColor: "#3d3d5c",
            HoverBackgroundColor: "#4d4d6c",
            PressedBackgroundColor: "#2d2d4c",
            BorderRadius: 8
        }, menu);

        await World.spawn({
            Node: { width: "auto", height: "auto" },
            Text: { value: item.label, font_size: 18, color: "#ffffff" }
        }, btn);

        btn._action = item.action;
        buttons.push(btn);
    }

    return { menu, buttons };
}

// Usage
const { menu, buttons } = await createMenu(window, [
    { label: "New Game", action: () => console.log("New game") },
    { label: "Load Game", action: () => console.log("Load game") },
    { label: "Settings", action: () => console.log("Settings") },
    { label: "Quit", action: () => System.exit(0) }
]);

// Handle clicks
System.registerEvent("graphic:entity:interactionChanged", async (req, res) => {
    if (req.interaction !== "pressed") return;

    for (const btn of buttons) {
        if (req.entityId === btn.id && btn._action) {
            await btn._action();
            res.handled = true;
            break;
        }
    }
});
```

### Updating Text Dynamically

```javascript
// Create a label
const scoreLabel = await World.spawn({
    Node: { width: "auto", height: "auto" },
    Text: { value: "Score: 0", font_size: 24, color: "#ffff00" }
}, window);

// Update the text (using update to preserve other fields)
let score = 0;

async function addScore(points) {
    score += points;
    await scoreLabel.update("Text", { value: `Score: ${score}` });
}

await addScore(100);  // "Score: 100"
await addScore(50);   // "Score: 150"
```

---

## Error Handling

All async methods can throw errors. Use try/catch:

```javascript
try {
    const entity = await World.spawn({ Invalid: {} });
} catch (error) {
    console.error("Spawn failed:", error.message);
}
```

Common errors:
- `"World.* is not available on the server. This method is client-only."`
- `"Entity X not found"` - Entity was despawned
- `"Entity X does not have Y component"` - Component not present

---

## See Also

- [ECS Architecture](../../../graphic/ecs.md) - Generic ECS documentation (runtime-agnostic)
- [Window API](window.md) - Widget-based UI API
- [System API](../system.md) - System events and mod lifecycle
