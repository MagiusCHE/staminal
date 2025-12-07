# ECS API Documentation

The ECS (Entity-Component-System) API allows mods to create game entities with components, query them efficiently, and declare systems that run automatically.

## Overview

The ECS API is exposed through two global objects:

- **`World`** - Manages the ECS world (spawn, query, register components, declare systems)
- **`Entity`** - Handle returned by `World.spawn()`, provides entity-level operations

## Global Objects

### World

The main entry point for ECS operations.

#### Methods

##### `World.spawn(components?)`

Spawn a new entity with optional initial components.

```javascript
// Spawn empty entity
const entity = await World.spawn();

// Spawn with initial components
const player = await World.spawn({
    Position: { x: 100, y: 200 },
    Velocity: { x: 0, y: 0 },
    Health: { current: 100, max: 100 }
});

console.log(player.id); // Entity ID (number)
```

**Parameters:**
- `components` (optional): Object with component names as keys and component data as values

**Returns:** `Entity` - Handle to the spawned entity

##### `World.despawn(entityId)`

Despawn an entity by ID.

```javascript
await World.despawn(entityId);
// or
await entity.despawn();
```

##### `World.query(options)`

Query entities matching criteria.

```javascript
const entities = await World.query({
    withComponents: ["Position", "Velocity"],  // Must have these
    withoutComponents: ["Frozen"],             // Must NOT have these
    limit: 100                                 // Optional: max results
});

for (const entity of entities) {
    console.log(entity.id);                    // Entity ID
    console.log(entity.components.Position);   // Component data
    console.log(entity.components.Velocity);
}
```

**Parameters:**
- `options.withComponents`: Array of component names the entity must have
- `options.withoutComponents`: Array of component names the entity must NOT have
- `options.limit` (optional): Maximum number of results

**Returns:** Array of query results with `{ id, components }` structure

##### `World.registerComponent(name, schema?)`

Register a custom component type with optional schema for validation.

```javascript
// Register without schema (any data allowed)
await World.registerComponent("Tag");

// Register with schema (validates data)
await World.registerComponent("Player", {
    health: FieldTypes.Number,
    name: FieldTypes.String,
    position: FieldTypes.Vec2
});
```

**Parameters:**
- `name`: Component type name
- `schema` (optional): Object mapping field names to `FieldTypes` values

##### `World.declareSystem(config)`

Declare a system that runs automatically every frame.

```javascript
// System with predefined behavior
await World.declareSystem({
    name: "gravity",
    query: { withComponents: ["Velocity"] },
    behavior: SystemBehaviors.ApplyGravity,
    config: { strength: 9.8, direction: "down" },
    order: 0,       // Lower runs first
    enabled: true
});

// System with formulas
await World.declareSystem({
    name: "age",
    query: { withComponents: ["Age"] },
    formulas: ["Age.value = Age.value + dt"],
    order: 10
});
```

**Parameters:**
- `config.name`: Unique system name
- `config.query`: Query options (`withComponents`, `withoutComponents`)
- `config.behavior` (optional): `SystemBehaviors` enum value
- `config.config` (optional): Behavior configuration object
- `config.formulas` (optional): Array of formula strings
- `config.order` (default: 0): Execution order (lower runs first)
- `config.enabled` (default: true): Whether system is active

##### `World.setSystemEnabled(name, enabled)`

Enable or disable a declared system.

```javascript
await World.setSystemEnabled("gravity", false);  // Pause gravity
await World.setSystemEnabled("gravity", true);   // Resume
```

##### `World.removeSystem(name)`

Remove a declared system.

```javascript
await World.removeSystem("gravity");
```

---

### Entity

Handle to a specific entity, returned by `World.spawn()`.

#### Properties

##### `entity.id`

The entity's unique ID (number).

```javascript
const entity = await World.spawn();
console.log(entity.id);  // e.g., 1
```

#### Methods

##### `entity.insert(componentName, data)` or `entity.insert(components)`

Insert or update component(s) on this entity.

```javascript
// Single component
await entity.insert("Health", { current: 100, max: 100 });
await entity.insert("Position", { x: 0, y: 0 });

// Batch insert multiple components
await entity.insert({
    Health: { current: 100, max: 100 },
    Position: { x: 0, y: 0 }
});
```

##### `entity.update(componentName, data)` or `entity.update(components)`

Update specific fields of component(s), merging with existing data.

```javascript
// Single component - only updates specified fields
await entity.update("Health", { current: 50 });

// Batch update multiple components
await entity.update({
    Health: { current: 50 },
    Position: { x: 100 }
});
```

##### `entity.remove(componentName)`

Remove a component from this entity.

```javascript
await entity.remove("Velocity");
```

##### `entity.get(componentName)`

Get a component's data from this entity.

```javascript
const health = await entity.get("Health");
if (health) {
    console.log(health.current, health.max);
}
```

**Returns:** Component data object, or `null` if not present

##### `entity.has(componentName)`

Check if this entity has a component.

```javascript
if (await entity.has("Velocity")) {
    console.log("Entity can move!");
}
```

**Returns:** `boolean`

##### `entity.despawn()`

Despawn this entity.

```javascript
await entity.despawn();
```

---

## Enums

### SystemBehaviors

Predefined behaviors for declared systems. These behaviors are implemented in Rust and execute efficiently without crossing the JS/Rust boundary every frame.

| Value | Description | Required Components | Config Options |
|-------|-------------|---------------------|----------------|
| `ApplyVelocity` | Add Velocity to Transform every frame | Transform, Velocity | - |
| `ApplyGravity` | Apply gravity force to Velocity | Velocity | `strength` (f32, default 980), `direction` ("down", "up", "left", "right") |
| `ApplyFriction` | Reduce Velocity over time (drag) | Velocity | `factor` (f32, 0-1, how much to retain per second, default 0.98) |
| `RegenerateOverTime` | Increment a numeric field over time | Any with target field | `field` (string), `rate` (f32/sec), `max_field` (optional string) |
| `DecayOverTime` | Decrement a numeric field over time | Any with target field | `field` (string), `rate` (f32/sec), `min_field` (optional string) |
| `DespawnWhenZero` | Despawn entity when a field reaches zero | Any with target field | `field` (string, default "health") |
| `FollowEntity` | Move entity towards another entity | Transform | `speed_field`, `target_field` (TODO) |
| `OrbitAround` | Orbit around a point or entity | Transform | `center`, `radius`, `speed` (TODO) |
| `BounceOnBounds` | Bounce when hitting bounds | Transform, Velocity | `bounds`, `damping` (TODO) |
| `AnimateSprite` | Cycle through sprite animation frames | Sprite | `frames`, `frame_time` (TODO) |

#### Behavior Configuration Examples

```javascript
// ApplyVelocity - no config needed
await World.declareSystem({
    name: "movement",
    query: { withComponents: ["Transform", "Velocity"] },
    behavior: SystemBehaviors.ApplyVelocity,
    order: 0
});

// ApplyGravity with custom strength and direction
await World.declareSystem({
    name: "gravity",
    query: { withComponents: ["Velocity"], withoutComponents: ["Flying"] },
    behavior: SystemBehaviors.ApplyGravity,
    config: { strength: 500, direction: "down" },
    order: 1
});

// ApplyFriction with custom factor
await World.declareSystem({
    name: "friction",
    query: { withComponents: ["Velocity"] },
    behavior: SystemBehaviors.ApplyFriction,
    config: { factor: 0.95 },  // Retain 95% velocity per second
    order: 2
});

// RegenerateOverTime for health regeneration
await World.declareSystem({
    name: "health_regen",
    query: { withComponents: ["Health", "Regenerating"] },
    behavior: SystemBehaviors.RegenerateOverTime,
    config: { field: "current", rate: 5, max_field: "max" },
    order: 10
});

// DecayOverTime for status effects
await World.declareSystem({
    name: "poison_damage",
    query: { withComponents: ["Health", "Poisoned"] },
    behavior: SystemBehaviors.DecayOverTime,
    config: { field: "current", rate: 2 },
    order: 11
});

// DespawnWhenZero to remove dead entities
await World.declareSystem({
    name: "cleanup_dead",
    query: { withComponents: ["Health"] },
    behavior: SystemBehaviors.DespawnWhenZero,
    config: { field: "current" },
    order: 100
});
```

### System Formulas

Formulas provide a declarative way to update component values using mathematical expressions. Unlike behaviors which are predefined, formulas let you write custom expressions that are evaluated every frame.

#### Formula Syntax

Formulas follow the pattern:
```
Component.field = expression
```

**Example:**
```javascript
await World.declareSystem({
    name: "oscillate",
    query: { withComponents: ["Transform", "Oscillator"] },
    formulas: [
        "Transform.x = Oscillator.center_x + sin(time * Oscillator.speed) * Oscillator.amplitude",
        "Transform.y = Oscillator.center_y + cos(time * Oscillator.speed) * Oscillator.amplitude"
    ],
    order: 5
});
```

#### Available Variables

| Variable | Type | Description |
|----------|------|-------------|
| `dt` | `number` | Delta time since last frame (seconds) |
| `time` | `number` | Total elapsed time since start (seconds) |
| `Transform_translation_x` | `number` | Entity's X position |
| `Transform_translation_y` | `number` | Entity's Y position |
| `Transform_translation_z` | `number` | Entity's Z position |
| `Transform_scale_x` | `number` | Entity's X scale |
| `Transform_scale_y` | `number` | Entity's Y scale |
| `Transform_scale_z` | `number` | Entity's Z scale |
| `ComponentName_fieldName` | `number/string/bool` | Custom component field values |

**Note:** In expressions, dot notation is automatically converted to underscore notation. So `Transform.translation.x` becomes `Transform_translation_x`.

#### Available Math Functions

| Function | Arguments | Description |
|----------|-----------|-------------|
| `sin(x)` | 1 | Sine of x (radians) |
| `cos(x)` | 1 | Cosine of x (radians) |
| `tan(x)` | 1 | Tangent of x (radians) |
| `abs(x)` | 1 | Absolute value |
| `sqrt(x)` | 1 | Square root |
| `pow(base, exp)` | 2 | Power (base^exp) |
| `min(a, b)` | 2 | Minimum of two values |
| `max(a, b)` | 2 | Maximum of two values |
| `clamp(val, min, max)` | 3 | Clamp value between min and max |
| `lerp(a, b, t)` | 3 | Linear interpolation: a + (b - a) * t |

#### Target Fields

Formulas can update:
- **Transform fields**: `Transform.x`, `Transform.y`, `Transform.z`, `Transform.translation_x`, `Transform.translation_y`, `Transform.translation_z`, `Transform.scale_x`, `Transform.scale_y`, `Transform.scale_z`, `Transform.scale` (sets all axes)
- **Custom component fields**: Any numeric field on registered components

#### Formula Examples

```javascript
// Sine wave oscillation
await World.declareSystem({
    name: "wave",
    query: { withComponents: ["Transform", "Wave"] },
    formulas: [
        "Transform.y = Wave.base_y + sin(time * Wave.frequency) * Wave.amplitude"
    ]
});

// Circular motion
await World.declareSystem({
    name: "orbit",
    query: { withComponents: ["Transform", "Orbit"] },
    formulas: [
        "Transform.x = Orbit.center_x + cos(time * Orbit.speed) * Orbit.radius",
        "Transform.y = Orbit.center_y + sin(time * Orbit.speed) * Orbit.radius"
    ]
});

// Pulsing scale
await World.declareSystem({
    name: "pulse",
    query: { withComponents: ["Transform", "Pulse"] },
    formulas: [
        "Transform.scale = Pulse.base_scale + sin(time * Pulse.speed) * Pulse.range"
    ]
});

// Countdown timer
await World.declareSystem({
    name: "countdown",
    query: { withComponents: ["Timer"] },
    formulas: [
        "Timer.remaining = max(0, Timer.remaining - dt)"
    ]
});

// Smooth value interpolation
await World.declareSystem({
    name: "smooth_follow",
    query: { withComponents: ["Transform", "Target"] },
    formulas: [
        "Transform.x = lerp(Transform.translation.x, Target.x, dt * Target.speed)",
        "Transform.y = lerp(Transform.translation.y, Target.y, dt * Target.speed)"
    ]
});

// Clamped velocity
await World.declareSystem({
    name: "clamp_speed",
    query: { withComponents: ["Velocity", "SpeedLimit"] },
    formulas: [
        "Velocity.x = clamp(Velocity.x, -SpeedLimit.max, SpeedLimit.max)",
        "Velocity.y = clamp(Velocity.y, -SpeedLimit.max, SpeedLimit.max)"
    ]
});
```

#### Behaviors vs Formulas

| Aspect | Behaviors | Formulas |
|--------|-----------|----------|
| **Performance** | Faster (native Rust) | Slightly slower (expression parsing) |
| **Flexibility** | Predefined operations only | Any mathematical expression |
| **Use cases** | Common game mechanics | Custom animations, effects |
| **Config** | Via `config` object | Via expression variables |

**Recommendation:** Use behaviors for standard operations (velocity, gravity, friction) and formulas for custom mathematical relationships.

---

### FieldTypes

Field types for component schema definitions.

| Value | Description | Example |
|-------|-------------|---------|
| `Number` | Numeric value | `42`, `3.14` |
| `String` | Text value | `"Hello"` |
| `Bool` | Boolean value | `true`, `false` |
| `Vec2` | 2D vector | `{ x: 0, y: 0 }` |
| `Vec3` | 3D vector | `{ x: 0, y: 0, z: 0 }` |
| `Color` | Color value | `"#FF0000"` or `{ r: 1, g: 0, b: 0, a: 1 }` |
| `Entity` | Entity reference | `42` (entity ID) |
| `Any` | Any JSON value | Anything |

---

## Examples

### Basic Entity Management

```javascript
// Spawn a player
const player = await World.spawn({
    Position: { x: 100, y: 100 },
    Velocity: { x: 0, y: 0 },
    Health: { current: 100, max: 100 },
    Player: { name: "Hero" }
});

// Add a component later
await player.insert("Inventory", { items: [] });

// Check and modify
if (await player.has("Health")) {
    const health = await player.get("Health");
    health.current -= 10;
    await player.insert("Health", health);
}

// Remove a component
await player.remove("Velocity");  // Player can't move anymore

// Despawn
await player.despawn();
```

### Querying Entities

```javascript
// Find all moving entities
const movingEntities = await World.query({
    withComponents: ["Position", "Velocity"]
});

for (const entity of movingEntities) {
    const pos = entity.components.Position;
    const vel = entity.components.Velocity;
    console.log(`Entity ${entity.id} at (${pos.x}, ${pos.y}) moving (${vel.x}, ${vel.y})`);
}

// Find all enemies that are NOT frozen
const activeEnemies = await World.query({
    withComponents: ["Enemy", "Position"],
    withoutComponents: ["Frozen", "Dead"]
});
```

### Declaring Systems

```javascript
// Movement system
await World.declareSystem({
    name: "movement",
    query: { withComponents: ["Position", "Velocity"] },
    behavior: SystemBehaviors.ApplyVelocity,
    order: 0
});

// Gravity system
await World.declareSystem({
    name: "gravity",
    query: {
        withComponents: ["Velocity"],
        withoutComponents: ["Flying"]
    },
    behavior: SystemBehaviors.ApplyGravity,
    config: { strength: 980 },  // pixels/sec^2
    order: 1
});

// Friction system
await World.declareSystem({
    name: "friction",
    query: { withComponents: ["Velocity"] },
    behavior: SystemBehaviors.ApplyFriction,
    config: { factor: 0.98 },  // Retain 98% velocity per second
    order: 2
});

// Toggle systems
await World.setSystemEnabled("gravity", false);  // Disable gravity
await World.setSystemEnabled("gravity", true);   // Re-enable

// Remove system
await World.removeSystem("friction");
```

### Component Schemas

```javascript
// Register component types with validation
await World.registerComponent("Transform", {
    position: FieldTypes.Vec2,
    rotation: FieldTypes.Number,
    scale: FieldTypes.Vec2
});

await World.registerComponent("Sprite", {
    texture: FieldTypes.String,
    width: FieldTypes.Number,
    height: FieldTypes.Number,
    color: FieldTypes.Color
});

await World.registerComponent("CharacterStats", {
    health: FieldTypes.Number,
    mana: FieldTypes.Number,
    strength: FieldTypes.Number,
    name: FieldTypes.String
});

// Now spawn with validated components
const character = await World.spawn({
    Transform: { position: { x: 0, y: 0 }, rotation: 0, scale: { x: 1, y: 1 } },
    Sprite: { texture: "hero.png", width: 64, height: 64, color: "#FFFFFF" },
    CharacterStats: { health: 100, mana: 50, strength: 10, name: "Hero" }
});
```

---

## Native Components

The ECS API provides access to native Bevy components in addition to custom script-defined components. Native components are stored efficiently in Bevy's ECS and provide better performance for common game operations.

### Available Native Components

#### Transform

The Transform component controls an entity's position, rotation, and scale in the world.

```javascript
// Spawn entity with Transform
const entity = await World.spawn({
    Transform: {
        translation: { x: 100, y: 200, z: 0 },
        rotation: { x: 0, y: 0, z: 0, w: 1 },  // Quaternion
        scale: { x: 1, y: 1, z: 1 }
    }
});

// Update transform (partial updates supported)
await entity.insert("Transform", {
    translation: { x: 150, y: 250, z: 0 }
});

// Read transform
const transform = await entity.get("Transform");
console.log(transform.translation.x, transform.translation.y);
```

**Fields:**
- `translation`: `{ x, y, z }` - Position in world coordinates
- `rotation`: `{ x, y, z, w }` - Rotation as a quaternion
- `scale`: `{ x, y, z }` - Scale factor on each axis

#### Sprite

The Sprite component controls 2D rendering properties.

```javascript
// Spawn entity with Sprite
const entity = await World.spawn({
    Transform: { translation: { x: 100, y: 100, z: 0 } },
    Sprite: {
        color: { r: 1, g: 1, b: 1, a: 1 },  // White, fully opaque
        flip_x: false,
        flip_y: false,
        custom_size: { width: 64, height: 64 }
    }
});

// Update sprite color
await entity.insert("Sprite", {
    color: { r: 1, g: 0, b: 0, a: 1 }  // Red
});
```

**Fields:**
- `color`: `{ r, g, b, a }` - Color tint (values 0-1)
- `flip_x`: `boolean` - Flip horizontally
- `flip_y`: `boolean` - Flip vertically
- `custom_size`: `{ width, height }` or `null` - Custom render size
- `rect`: `{ min: {x, y}, max: {x, y} }` or `null` - Sprite sheet region

#### Visibility

The Visibility component controls whether an entity is rendered.

```javascript
// Spawn hidden entity
const entity = await World.spawn({
    Transform: { translation: { x: 0, y: 0, z: 0 } },
    Visibility: { value: "hidden" }
});

// Show entity
await entity.insert("Visibility", { value: "visible" });

// Or use inherited visibility (follows parent)
await entity.insert("Visibility", "inherited");
```

**Values:**
- `"inherited"` - Inherits visibility from parent entity
- `"visible"` - Always visible
- `"hidden"` - Always hidden

### UI Components

The ECS API also provides access to Bevy's UI components for building user interfaces.

#### Node

The Node component controls layout properties using a flexbox-like system.

```javascript
// Spawn a UI container
const container = await World.spawn({
    Node: {
        width: "100%",
        height: "auto",
        flex_direction: "column",
        justify_content: "center",
        align_items: "center",
        padding: 10,
        margin: 5
    }
});

// Update layout
await container.insert("Node", {
    flex_direction: "row",
    justify_content: "space_between"
});
```

**Fields:**
- `width`, `height`: Size value (`"auto"`, `"50%"`, `100` (px), `"10vw"`, `"10vh"`)
- `min_width`, `min_height`, `max_width`, `max_height`: Size constraints
- `left`, `right`, `top`, `bottom`: Position offsets
- `padding`, `margin`: Edge spacing (number for all sides, or object with `top`, `right`, `bottom`, `left`)
- `display`: `"flex"`, `"grid"`, `"block"`, `"none"`
- `position_type`: `"relative"`, `"absolute"`
- `flex_direction`: `"row"`, `"column"`, `"row_reverse"`, `"column_reverse"`
- `justify_content`: `"start"`, `"end"`, `"center"`, `"space_between"`, `"space_around"`, `"space_evenly"`
- `align_items`: `"start"`, `"end"`, `"center"`, `"stretch"`, `"baseline"`

#### BackgroundColor

The BackgroundColor component sets the background color of a UI element.

```javascript
// Spawn a colored box
const box = await World.spawn({
    Node: { width: 100, height: 100 },
    BackgroundColor: { r: 0.2, g: 0.4, b: 0.8, a: 1.0 }
});

// Or use hex color
const box2 = await World.spawn({
    Node: { width: 100, height: 100 },
    BackgroundColor: "#3366CC"
});

// Update color
await box.insert("BackgroundColor", { r: 1, g: 0, b: 0, a: 1 });
```

**Formats:**
- Object: `{ r, g, b, a }` - RGBA values (0-1)
- Hex string: `"#RRGGBB"` or `"#RRGGBBAA"`

#### Text

The Text component displays text in the UI.

```javascript
// Spawn text element
const label = await World.spawn({
    Node: { width: "auto", height: "auto" },
    Text: {
        value: "Hello, World!",
        font_size: 24,
        color: { r: 1, g: 1, b: 1, a: 1 }
    }
});

// Update text
await label.insert("Text", {
    value: "Updated text!",
    font_size: 32
});
```

**Fields:**
- `value`: The text content (string)
- `font_size`: Font size in pixels (number)
- `color`: Text color (`{ r, g, b, a }` or hex string)
- `font`: Font asset path (optional)

#### BorderRadius

The BorderRadius component adds rounded corners to UI elements.

```javascript
// Spawn a rounded box
const roundedBox = await World.spawn({
    Node: { width: 100, height: 100 },
    BackgroundColor: "#3366CC",
    BorderRadius: {
        top_left: 10,
        top_right: 10,
        bottom_left: 10,
        bottom_right: 10
    }
});

// Or use a single value for all corners
const pill = await World.spawn({
    Node: { width: 200, height: 50 },
    BackgroundColor: "#3366CC",
    BorderRadius: 25  // All corners
});
```

**Formats:**
- Number: Same radius for all corners (in pixels)
- Object: `{ top_left, top_right, bottom_left, bottom_right }` - Individual corner radii

#### ImageNode

The ImageNode component displays an image in the UI. Images must be loaded first using the `Resource.load()` API before they can be used.

```javascript
// First, load the image resource
Resource.load("@my-mod/assets/images/background.png", "bg-image");
await Resource.whenLoadedAll();

// Spawn an image element
const image = await World.spawn({
    Node: { width: "100%", height: "100%" },
    ImageNode: {
        resource_id: "bg-image",
        image_mode: NodeImageMode.Stretch
    }
});

// Update image properties
await image.insert("ImageNode", {
    resource_id: "bg-image",
    image_mode: NodeImageMode.Tiled,
    flip_x: true
});
```

**Fields:**
- `resource_id` (required): The resource alias used when loading with `Resource.load()`
- `image_mode`: How the image should be rendered:
  - `NodeImageMode.Auto` (0): Uses the image's original dimensions with layout constraints
  - `NodeImageMode.Stretch` (1): Stretch to fill the node (may distort aspect ratio)
  - `NodeImageMode.Sliced` (2): 9-slice scaling (for UI panels)
  - `NodeImageMode.Tiled` (3): Tile the image to fill the node
- `flip_x`: Flip the image horizontally (boolean, default: false)
- `flip_y`: Flip the image vertically (boolean, default: false)
- `color`: Tint color (`{ r, g, b, a }` or hex string, default: white)

**Example with full options:**
```javascript
const decoratedPanel = await World.spawn({
    Node: { width: 300, height: 200 },
    ImageNode: {
        resource_id: "panel-border",
        image_mode: NodeImageMode.Sliced,
        flip_x: false,
        flip_y: false,
        color: "#ffffff"
    }
});
```

**Note:** The `NodeImageMode` enum is available as a global constant with values: `Auto`, `Stretch`, `Sliced`, `Tiled`.

> **Not yet implemented:** CSS-like `Cover` (scale to cover while maintaining aspect ratio) and `Contain` (scale to fit while maintaining aspect ratio) modes are not yet available. Use `Stretch` for full coverage or calculate custom dimensions manually.

### Button Event Handlers

The `Button` component supports event callback fields for direct event handling. These are language-agnostic - each runtime (JavaScript, Lua, C#, etc.) handles the callbacks in its native way.

**Supported event callbacks:**
- `on_click` - Called when the button is pressed (clicked)
- `on_hover` - Called when hover state changes (future)
- `on_enter` - Called when cursor enters the button (future)
- `on_leave` - Called when cursor leaves the button (future)

**Example (JavaScript):**
```javascript
const button = await World.spawn({
    Node: { width: 200, height: 50 },
    Button: {
        on_click: (event) => {
            console.log("Clicked!", event.entityId, event.x, event.y);
        }
    },
    BackgroundColor: "#3366CC"
});
```

**How it works:**
1. When `on_click` (or other `on_*` callbacks) are specified in a component, they are extracted before the entity is created
2. The callback is stored in a runtime-specific registry (e.g., `__ENTITY_EVENT_CALLBACKS__` for JavaScript)
3. The entity is registered with the GraphicEngine for direct event dispatch
4. When the event occurs, the callback is invoked directly without going through the global event system

**Benefits of direct callbacks:**
- **Isolation**: Other mods cannot intercept your callbacks
- **Performance**: No global event broadcasting overhead
- **Simplicity**: No need to manually register/unregister event handlers

The callback receives an event object with:
- `entityId` - The entity ID that triggered the event
- `eventType` - The event type ("click", "hover", etc.)
- `x` - Cursor X position
- `y` - Cursor Y position

### Button Color States (Pseudo-Components)

When creating interactive buttons, you can define different background colors for each interaction state. These are "pseudo-components" - they are processed at spawn time to configure button behavior, but are not stored as separate ECS components.

**Available pseudo-components:**
- `HoverBackgroundColor` - Background color when the button is hovered
- `PressedBackgroundColor` - Background color when the button is pressed
- `DisabledBackgroundColor` - Background color when the button is disabled
- `Disabled` - Boolean flag to set the initial disabled state

**Requirements:**
- The entity must have a `BackgroundColor` component (used as the "normal" state color)
- The entity must have the `Button` component for interaction detection
- Pseudo-components are optional - if not provided, the normal color is used for that state
- If `PressedBackgroundColor` is not specified but `HoverBackgroundColor` is, the hovered color will be used when pressed

**Example - Simple button:**
```
{
    Node: { width: 200, height: 50 },
    Button: {},
    BackgroundColor: "#3366CC",           // Normal state
    HoverBackgroundColor: "#4477DD",      // When hovered
    PressedBackgroundColor: "#2255BB",    // When pressed
    DisabledBackgroundColor: "#666666",   // When disabled
    Disabled: false,                       // Initially enabled
    BorderRadius: 8
}
```

### Disabling Buttons

Buttons can be disabled at spawn time or dynamically at runtime using the `Disabled` pseudo-component.

**At spawn time:**
```
{
    Node: { width: 200, height: 50 },
    Button: {},
    BackgroundColor: "#3366CC",
    DisabledBackgroundColor: "#666666",
    Disabled: true  // Button starts disabled
}
```

**At runtime:**
Use the entity's update method to toggle the disabled state:
```
// Disable the button
entity.update("Disabled", true);

// Enable the button
entity.update("Disabled", false);
```

When disabled:
- The button displays the `DisabledBackgroundColor` (or `BackgroundColor` if not specified)
- Hover and press color changes are ignored
- The button still receives interaction events, but handlers should check the disabled state

**Example - Styled menu button with centered text (spawn as parent, then child):**
```
// Parent: the button container
{
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
}

// Child: the text label (spawn with parent option)
{
    Node: { width: "auto", height: "auto" },
    Text: { value: "Click Me!", font_size: 18, color: "#ffffff" }
}
```

**Color cascade:**
1. **Normal**: Uses `BackgroundColor`
2. **Hovered**: Uses `HoverBackgroundColor` if set, otherwise `BackgroundColor`
3. **Pressed**: Uses `PressedBackgroundColor` if set, otherwise `HoverBackgroundColor` if set, otherwise `BackgroundColor`
4. **Disabled**: Uses `DisabledBackgroundColor` if set, otherwise `BackgroundColor`

### Automatic Cursor Change

When an entity has both the `Button` and `Interaction` components, the cursor automatically changes to a pointer (hand) when hovering over it. This provides visual feedback to users that the element is clickable.

The cursor returns to the default arrow when no buttons are hovered.

This behavior is automatic and requires no additional configuration.

### Mixing Native and Custom Components

You can freely mix native Bevy components with custom script-defined components:

```javascript
// Spawn entity with both native and custom components
const player = await World.spawn({
    // Native components
    Transform: {
        translation: { x: 100, y: 100, z: 0 },
        scale: { x: 1, y: 1, z: 1 }
    },
    Visibility: "visible",

    // Custom components
    Health: { current: 100, max: 100 },
    Player: { name: "Hero", level: 1 }
});

// Query works with both
const visiblePlayers = await World.query({
    withComponents: ["Transform", "Player", "Visibility"]
});
```

---

## Architecture Notes

### Component Storage

Script-defined components are stored as JSON data since Rust cannot dynamically create struct types at runtime. Each component is validated against its schema (if registered) when inserted.

Native Bevy components (Transform, Sprite, Visibility, Node, BackgroundColor, Text, BorderRadius, ImageNode, Button, Interaction) are stored directly in Bevy's ECS as their native types, providing better performance and integration with the rendering and UI pipelines.

### Entity IDs

Entity IDs are managed by a script-facing registry that maps to internal Bevy entities. The IDs exposed to JavaScript are stable and unique during the session.

### Thread Safety

All ECS operations go through the command channel to the Bevy main thread, ensuring thread-safe access to the ECS world from the JavaScript worker thread.

### Client-Only

The ECS API is client-only. Calling any method on the server will throw an error:

```javascript
try {
    await World.spawn({});
} catch (e) {
    // "World.spawn() is not available on the server. This method is client-only."
}
```

---

## Runtime-Specific Documentation

For runtime-specific API documentation with complete code examples:

- **JavaScript**: [JavaScript ECS API](../mods/js/graphic/ecs.md)
