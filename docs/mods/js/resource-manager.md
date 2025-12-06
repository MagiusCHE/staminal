# Resource API (JavaScript)

The `Resource` global object provides methods to load, cache, and manage game assets. This is a **client-only** API.

## Methods Overview

| Method | Description |
|--------|-------------|
| `load(path, alias, options?)` | Queue a resource for loading (synchronous) |
| `whenLoaded(alias)` | Wait for a specific resource to finish loading |
| `isLoaded(alias)` | Check if a resource is loaded |
| `getInfo(alias)` | Get information about a resource |
| `getLoadingProgress()` | Get global loading progress |
| `isLoadingCompleted()` | Check if all resources are loaded |
| `unload(alias)` | Remove a resource from cache |
| `unloadAll()` | Remove all resources from cache |

---

## load(path, alias, options?)

Queue a resource for loading. This method is **synchronous** - it returns immediately without waiting for the resource to load.

**Arguments:**
- `path: string` - Resource path (supports `@mod-id/path` syntax)
- `alias: string` - Unique alias to reference the resource
- `options?: LoadOptions` - Optional loading options

**Returns:**
- `ResourceInfo` - If the resource is already loaded
- `undefined` - If the resource was queued for loading

**LoadOptions:**
- `forceReload?: boolean` - Force reload even if already in cache
- `type?: ResourceType` - Explicit type (if not deducible from extension)

**Example:**
```javascript
// Queue resources for loading (synchronous, returns immediately)
Resource.load("@bme-assets/background/title.jpg", "title-bg");
Resource.load("@bme-assets/images/logo.png", "logo");
Resource.load("@bme-assets/fonts/main.ttf", "main-font");

// The requested counter is immediately updated
const progress = Resource.getLoadingProgress();
console.log(`Queued: ${progress.requested} resources`);

// Wait for a specific resource
await Resource.whenLoaded("title-bg");

// Or wait for all resources
while (!Resource.isLoadingCompleted()) {
    await System.sleep(50);
}
```

---

## whenLoaded(alias)

Wait for a specific resource to finish loading. This is the async counterpart to `load()`.

**Arguments:**
- `alias: string` - The alias of the resource to wait for

**Returns:** `Promise<ResourceInfo>` - Resolves when the resource is loaded, rejects on error

**Example:**
```javascript
// Queue a resource
Resource.load("@bme-assets/background/title.jpg", "title-bg");

// Wait for it to load
try {
    const info = await Resource.whenLoaded("title-bg");
    console.log(`Loaded: ${info.resolvedPath}`);
} catch (error) {
    console.error("Failed to load:", error);
}
```

**Note:** You must call `load()` before `whenLoaded()`. Calling `whenLoaded()` on a non-existent alias will throw an error.

---

## isLoaded(alias)

Check if a resource is in cache and fully loaded.

**Arguments:**
- `alias: string` - Resource alias

**Returns:** `boolean` - True if the resource is loaded

**Example:**
```javascript
if (Resource.isLoaded("title-background")) {
    showTitleScreen();
} else {
    showLoadingScreen();
}
```

---

## getInfo(alias)

Get detailed information about a resource.

**Arguments:**
- `alias: string` - Resource alias

**Returns:** `ResourceInfo | null` - Resource info or null if not found

**ResourceInfo Properties:**
- `alias: string` - Resource alias
- `path: string` - Original path
- `resolvedPath: string` - Resolved absolute path
- `type: ResourceType` - Resource type
- `state: "loading" | "loaded" | "error"` - Loading state
- `size?: number` - Size in bytes (if available)
- `error?: string` - Error message (if state is "error")

**Example:**
```javascript
const info = Resource.getInfo("background-image");
if (info) {
    console.log(`Resource: ${info.alias}`);
    console.log(`Type: ${info.type}`);
    console.log(`State: ${info.state}`);
    if (info.size) {
        console.log(`Size: ${info.size} bytes`);
    }
}
```

---

## getLoadingProgress()

Get the global loading progress. Returns pre-calculated data (O(1) operation).

**Returns:** `LoadingProgress` with:
- `requested: number` - Number of resources queued via `load()`
- `loaded: number` - Number of resources successfully loaded

**Example:**
```javascript
const progress = Resource.getLoadingProgress();
const percent = progress.requested > 0
    ? Math.floor((progress.loaded / progress.requested) * 100)
    : 100;
console.log(`Loading: ${progress.loaded}/${progress.requested} (${percent}%)`);
```

---

## isLoadingCompleted()

Check if all requested resources have been loaded. Returns pre-calculated data (O(1) operation).

**Returns:** `boolean` - True if all resources are loaded (or if no resources were requested)

**Example:**
```javascript
if (Resource.isLoadingCompleted()) {
    startGame();
} else {
    updateLoadingBar();
}
```

---

## unload(alias)

Remove a resource from cache and free associated memory.

**Arguments:**
- `alias: string` - Alias of resource to remove

**Returns:** `Promise<void>` - Resolves when unloaded

**Example:**
```javascript
// Unload a specific resource
await Resource.unload("title-background");
console.log("Resource unloaded successfully");
```

---

## unloadAll()

Remove all resources from cache and free all associated memory.

**Returns:** `Promise<void>`

**Example:**
```javascript
// Unload all resources when changing scenes
await Resource.unloadAll();
console.log("All resources cleared");
```

---

## ResourceType Enum

Available resource types:

| Type | Extensions |
|------|------------|
| `image` | .png, .jpg, .jpeg, .gif, .webp, .bmp, .hdr, .ktx2, .dds, .tga, .tiff, .exr, .qoi, .basis |
| `font` | .ttf, .otf |
| `audio` | .mp3, .ogg, .wav, .flac |
| `shader` | .wgsl |
| `model3d` | .gltf, .glb |
| `json` | .json |
| `text` | .txt, .md, .xml, .csv |
| `binary` | * (other extensions) |

---

## Path Resolution

The `path` argument in `Resource.load()` supports the `@mod-id/path` syntax for loading resources from other mods:

### Examples

```javascript
// Load from another mod using @mod-id syntax
Resource.load("@bme-assets-01/assets/background/title.jpg", "title-bg");
await Resource.whenLoaded("title-bg");

// Load from any path relative to game data directory
Resource.load("mods/my-mod/assets/icon.png", "icon");

// The resolved path will be: mods/bme-assets-01/assets/background/title.jpg
```

### Resolution Rules

| Path | Resolves To |
|------|-------------|
| `@other-mod/assets/file.png` | `mods/other-mod/assets/file.png` |
| `assets/file.png` | `assets/file.png` |

### Mod Validation

When using `@mod-id` syntax, the system verifies that the mod exists. If not:

```javascript
Resource.load("@nonexistent-mod/assets/file.png", "file");
// Throws: "Mod 'nonexistent-mod' not found. Cannot resolve path '@nonexistent-mod/assets/file.png'"
```

---

## TypeScript Definitions

```typescript
declare const Resource: {
    load(path: string, alias: string, options?: LoadOptions): ResourceInfo | undefined;
    whenLoaded(alias: string): Promise<ResourceInfo>;
    isLoaded(alias: string): boolean;
    getInfo(alias: string): ResourceInfo | null;
    getLoadingProgress(): LoadingProgress;
    isLoadingCompleted(): boolean;
    unload(alias: string): Promise<void>;
    unloadAll(): Promise<void>;
};

interface LoadOptions {
    forceReload?: boolean;
    type?: ResourceType;
}

interface LoadingProgress {
    requested: number;
    loaded: number;
}

interface ResourceInfo {
    alias: string;
    path: string;
    resolvedPath: string;
    type: ResourceType;
    state: "loading" | "loaded" | "error";
    size?: number;
    error?: string;
}

type ResourceType = "image" | "audio" | "video" | "shader" | "font" | "model3d" | "json" | "text" | "binary";
```

---

## Usage Patterns

### Preloading with Progress Bar

```javascript
// Queue all resources (synchronous - returns immediately)
Resource.load("@assets/bg1.png", "bg1");
Resource.load("@assets/bg2.png", "bg2");
Resource.load("@assets/music.ogg", "bgm");
Resource.load("@assets/font.ttf", "main-font");

// Poll for progress
async function updateProgress() {
    while (!Resource.isLoadingCompleted()) {
        const progress = Resource.getLoadingProgress();
        updateProgressBar(progress.loaded, progress.requested);
        await System.sleep(50);
    }
    hideLoadingScreen();
    startGame();
}
updateProgress();
```

### Wait for Specific Resources

```javascript
// Queue resources
Resource.load("@assets/critical.png", "critical");
Resource.load("@assets/optional.png", "optional");

// Wait only for critical resource
await Resource.whenLoaded("critical");
showUI();

// Optional resource loads in background
```

### Using with Image Widgets

```javascript
// Queue the image
Resource.load("@bme-assets/background/title.jpg", "title-bg");

// Wait for it to load
await Resource.whenLoaded("title-bg");

// Create widget using resourceId
const background = await container.createChild(WidgetTypes.Image, {
    width: "100%",
    height: "100%",
    image: {
        resourceId: "title-bg",
        scaleMode: ImageScaleModes.Cover
    }
});
```

### Scene Transition with Resource Cleanup

```javascript
async function transitionToLevel(levelId) {
    // Unload previous level resources
    await Resource.unloadAll();

    // Queue new level resources
    Resource.load(`@levels/${levelId}/background.png`, "level-bg");
    Resource.load(`@levels/${levelId}/tileset.png`, "tileset");
    Resource.load(`@levels/${levelId}/config.json`, "level-config");

    // Wait for all to load
    while (!Resource.isLoadingCompleted()) {
        await System.sleep(50);
    }

    // Start level
    initializeLevel();
}
```

---

## Error Handling

```javascript
// Queue and wait with error handling
Resource.load("@assets/missing.png", "missing");
try {
    await Resource.whenLoaded("missing");
} catch (error) {
    console.error("Failed to load resource:", error);
}

// Or check state after loading
const info = Resource.getInfo("my-resource");
if (info?.state === "error") {
    console.error("Resource error:", info.error);
}
```

---

## Notes

- **Client-only**: All Resource methods throw errors when called on the server
- **Synchronous load()**: `load()` returns immediately - use `whenLoaded()` to wait
- **Manual memory management**: Resources are never automatically unloaded
- **Unique aliases**: Each alias must be unique; loading with an existing alias requires `forceReload: true`
- **Cross-mod access**: Use `@mod-id/path` syntax to load assets from other mods
- **Background loading**: Resources are loaded by the engine's main event loop, not blocking JavaScript execution
