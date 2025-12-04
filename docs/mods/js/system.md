# System API (JavaScript)

The `System` global object provides access to mod information, game context, and utility functions for file path resolution.

## Methods Overview

| Method | Availability | Description |
|--------|--------------|-------------|
| `getMods()` | Client & Server | Get information about all registered mods |
| `getGameInfo()` | Client only | Get current game context information |
| `getGameConfigPath(relativePath)` | Client only | Get full path for a config file |
| `getAssetsPath(relativePath)` | Client only | Resolve an asset path |
| `registerEvent(event, handler, priority, ...)` | Client & Server | Register an event handler |
| `removeEvent(handlerId)` | Client & Server | Remove an event handler |
| `sendEvent(eventName, ...args)` | Client & Server | Send a custom event |
| `getModPackages(side)` | Server only | Get mod packages for a side |
| `installModFromPath(archivePath, modId)` | Client & Server | Install a mod from archive |
| `attachMod(modId)` | Client & Server | Attach a previously installed mod |
| `exit(code)` | Client & Server | Request graceful shutdown |
| `terminate(code)` | Client & Server | Immediate process termination |

---

## getMods()

Get information about all registered mods.

**Returns:** `Array<ModInfo>` - Array of mod information objects

**ModInfo Properties:**
- `id: string` - Unique mod identifier
- `version: string` - Mod version
- `name: string` - Human-readable name
- `description: string` - Mod description
- `mod_type: string | null` - "bootstrap" or "library"
- `priority: number` - Load priority (lower = earlier)
- `bootstrapped: boolean` - Whether onBootstrap was called
- `loaded: boolean` - Whether mod is loaded in runtime
- `exists: boolean` - Whether mod exists locally
- `download_url: string | null` - URL to download the mod
- `archive_sha512: string | null` - SHA512 hash of archive
- `archive_bytes: number | null` - Archive size in bytes
- `uncompressed_bytes: number | null` - Uncompressed size

**Example:**
```javascript
const mods = System.getMods();
for (const mod of mods) {
    console.log(`${mod.name} v${mod.version} - ${mod.loaded ? 'loaded' : 'not loaded'}`);
}
```

---

## getGameInfo() (Client Only)

Get information about the current game context.

**Returns:** `Object` with properties:
- `id: string` - Game identifier
- `name: string` - Game display name
- `version: string` - Game version

**Throws:** Error if called on the server

**Example:**
```javascript
const game = System.getGameInfo();
console.log(`Playing ${game.name} v${game.version}`);
```

---

## getGameConfigPath(relativePath) (Client Only)

Get the full absolute path for a config file within the game's config directory.

This method validates the path to prevent directory traversal attacks - paths containing `../` that would escape the config directory are rejected.

**Arguments:**
- `relativePath: string` - Relative path within config directory (e.g., "settings.json", "saves/slot1.json")

**Returns:** `string` - Full absolute path to the config file

**Throws:**
- Error if called on the server
- Error if path attempts directory traversal
- Error if path is absolute

**Security:**
- Absolute paths are rejected
- Paths with `..` that escape the config directory are rejected
- The file does not need to exist (useful for creating new files)

**Example:**
```javascript
// Get path for a config file
const settingsPath = System.getGameConfigPath("settings.json");
// Returns: "/home/user/.config/staminal/demo/settings.json"

// Get path for a nested config file
const savePath = System.getGameConfigPath("saves/slot1.json");
// Returns: "/home/user/.config/staminal/demo/saves/slot1.json"

// This will throw an error (path traversal attempt)
try {
    System.getGameConfigPath("../../../etc/passwd");
} catch (e) {
    console.error("Access denied:", e.message);
}
```

---

## getAssetsPath(relativePath) (Client Only)

Resolve a relative asset path to an actual file path.

**Path Resolution Order:**
1. If path starts with `@modid/`, looks in that mod's assets folder
2. Otherwise, checks current mod's assets folder first
3. Falls back to client's global assets directory

**Arguments:**
- `relativePath: string` - Relative asset path (e.g., "fonts/MyFont.ttf" or "@other-mod/icons/icon.png")

**Returns:** `string` - Resolved path relative to data root (e.g., "mods/my-mod/assets/fonts/MyFont.ttf")

**Throws:** Error if asset not found or referenced mod doesn't exist

**Example:**
```javascript
// Look in current mod's assets folder, fallback to global assets
const fontPath = System.getAssetsPath("fonts/PerfectDOSVGA437.ttf");

// Look in another mod's assets folder
const iconPath = System.getAssetsPath("@ui-toolkit/icons/close.png");
```

---

## registerEvent(event, handler, priority, protocol?, route?)

Register an event handler for system or custom events.

**Arguments:**
- `event: number | string` - SystemEvents enum value or custom event name
- `handler: Function` - Callback function
- `priority: number` - Handler priority (lower = first)
- `protocol?: string` - (RequestUri only) Protocol filter ("stam://", "http://", or "" for all)
- `route?: string` - (RequestUri only) Route prefix filter

**Returns:** `number` - Unique handler ID for removal

**Example:**
```javascript
// Register for system event
const handlerId = System.registerEvent(
    SystemEvents.TerminalKeyPressed,
    (request, response) => {
        if (request.combo === "Ctrl+Q") {
            response.setHandled(true);
            System.exit(0);
        }
    },
    100
);

// Register for custom event
System.registerEvent("mymod:player_joined", (request, response) => {
    console.log("Player joined:", request.args[0]);
}, 50);
```

---

## removeEvent(handlerId)

Remove a previously registered event handler.

**Arguments:**
- `handlerId: number` - Handler ID returned from registerEvent

**Returns:** `boolean` - True if handler was found and removed

**Example:**
```javascript
const id = System.registerEvent(SystemEvents.RequestUri, myHandler, 100);
// Later...
System.removeEvent(id);
```

---

## sendEvent(eventName, ...args)

Send a custom event to all registered handlers.

**Arguments:**
- `eventName: string` - Event name (e.g., "mymod:player_ready")
- `...args: any[]` - Arguments to pass to handlers

**Returns:** `Promise<Object>` with:
- `handled: boolean` - Whether any handler marked it as handled
- Additional properties set by handlers

**Example:**
```javascript
const result = await System.sendEvent("game:score_update", { player: "p1", score: 100 });
if (result.handled) {
    console.log("Score update was processed");
}
```

---

## exit(code)

Request a graceful shutdown of the application.

**Arguments:**
- `code: number` - Exit code (0 = success, non-zero = error)

**Example:**
```javascript
System.exit(0); // Graceful shutdown with success
```

---

## terminate(code)

Immediately terminate the process without cleanup.

**Arguments:**
- `code: number` - Exit code

**Note:** Use `exit()` for graceful shutdown. Only use `terminate()` for fatal errors.

---

## SystemEvents Enum

Available system events for `registerEvent`:

| Event | Value | Description |
|-------|-------|-------------|
| `RequestUri` | 0 | URI request (stam:// or http://) |
| `TerminalKeyPressed` | 1 | Terminal key input |
| `GraphicEngineReady` | 2 | Graphic engine initialized |
| `GraphicEngineWindowClosed` | 3 | Window closed |

**Example:**
```javascript
System.registerEvent(SystemEvents.GraphicEngineReady, (req, res) => {
    console.log("Graphics ready!");
}, 100);
```

---

## ModSides Enum

For filtering mod packages:

| Side | Value |
|------|-------|
| `Client` | 0 |
| `Server` | 1 |

**Example:**
```javascript
const clientMods = System.getModPackages(ModSides.Client);
```
