# Resource Manager

The Resource Manager is the Staminal engine's system for loading, caching, and managing game assets such as images, fonts, audio, shaders, and data files.

## Overview

The Resource Manager allows mods to:
- **Preload assets** before they are needed, avoiding loading delays during gameplay
- **Reference assets by alias** using a simple string identifier
- **Track loading progress** for loading screens and progress bars
- **Manually manage memory** by unloading resources when no longer needed

## Architecture

The Resource Manager uses a **ResourceProxy** shared across all scripting runtimes (JavaScript, Lua, future C#). This ensures resources loaded from one runtime are accessible from others.

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                              MOD RUNTIMES                                    │
│  ┌─────────────────┐  ┌─────────────────┐  ┌─────────────────┐              │
│  │ JavaScript (JS) │  │   Lua (future)  │  │  C# (future)    │              │
│  └────────┬────────┘  └────────┬────────┘  └────────┬────────┘              │
│           └────────────────────┼────────────────────┘                       │
│                                ▼                                            │
│  ┌──────────────────────────────────────────────────────────────────────┐  │
│  │                      ResourceProxy (Shared)                           │  │
│  │                                                                       │  │
│  │  resources: HashMap<alias, ResourceEntry>                             │  │
│  │  load_queue: VecDeque<QueuedLoadRequest>                              │  │
│  │  resource_waiters: HashMap<alias, Notify>                             │  │
│  │                                                                       │  │
│  │  ResourceEntry {                                                      │  │
│  │      alias, path, resolved_path, resource_type,                       │  │
│  │      state, engine_handle, data                                       │  │
│  │  }                                                                    │  │
│  └──────────────────────────────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────────────────────────────┘
                                     │
           ┌─────────────────────────┴─────────────────────────┐
           │                                                   │
           ▼                                                   ▼
┌──────────────────────────────────┐        ┌──────────────────────────────────┐
│  GraphicEngine (Bevy)            │        │  ResourceProxy Data Cache        │
│                                  │        │                                  │
│  Handles: Image, Font, Shader,   │        │  Stores: JSON, Text, Binary      │
│  Audio, 3D Models                │        │                                  │
│                                  │        │  For formats not supported       │
│  Uses Bevy's AssetServer with    │        │  by the GraphicEngine            │
│  automatic caching               │        │                                  │
└──────────────────────────────────┘        └──────────────────────────────────┘
```

## Loading Flow

The Resource Manager uses a **queue-based loading system**:

1. **`load()` is synchronous**: When a mod calls `Resource.load()`, the resource is added to a queue and the method returns immediately. The `requested` counter is incremented synchronously.

2. **Background processing**: The main event loop processes the queue, loading resources via the GraphicEngine or internal cache.

3. **`whenLoaded()` is async**: Mods can wait for specific resources using `Resource.whenLoaded(alias)`, which returns a Promise that resolves when the resource is loaded.

```
┌─────────────────────┐
│  Resource.load()    │ ──────► Queue + increment requested
│  (synchronous)      │         └──► returns undefined or ResourceInfo
└─────────────────────┘

┌─────────────────────┐
│  Main Event Loop    │ ◄────── Notified when queue has items
│  (async processor)  │ ──────► Process queue items
└─────────────────────┘         └──► Load via GraphicEngine or cache
                                └──► Mark loaded + notify waiters

┌─────────────────────┐
│ Resource.whenLoaded │ ◄────── Waits for notification
│  (async)            │ ──────► Returns ResourceInfo when loaded
└─────────────────────┘
```

## Resource Types

| Type | Extensions | Cache Location | Description |
|------|-----------|----------------|-------------|
| `image` | .png, .jpg, .jpeg, .gif, .webp, .bmp, .hdr, .ktx2, .dds, .tga, .tiff, .exr, .qoi, .basis | GraphicEngine | Textures and images |
| `font` | .ttf, .otf | GraphicEngine | Font files |
| `audio` | .mp3, .ogg, .wav, .flac | GraphicEngine | Sound effects and music |
| `shader` | .wgsl | GraphicEngine | GPU shaders |
| `model3d` | .gltf, .glb | GraphicEngine | 3D models |
| `json` | .json | ResourceProxy | JSON data files |
| `text` | .txt, .md, .xml, .csv | ResourceProxy | Text files |
| `binary` | * (other) | ResourceProxy | Binary data |

## Two-Level Caching

Resources are cached at two levels depending on their type:

1. **GraphicEngine Cache** (Bevy): For graphic resources (images, fonts, shaders, audio, 3D models). Bevy's AssetServer handles caching and reference counting automatically.

2. **ResourceProxy Cache**: For non-graphic resources (JSON, text, binary). Data is stored directly in memory.

## Path Resolution

The Resource Manager uses a centralized path resolution system that works consistently across both client and server.

### @mod-id Syntax

Paths starting with `@mod-id/` are resolved to the specified mod's root directory:

| Path Pattern | Resolution |
|--------------|------------|
| `@other-mod/assets/icon.png` | `mods/other-mod/assets/icon.png` |
| `@bme-assets-01/assets/background/title.jpg` | `mods/bme-assets-01/assets/background/title.jpg` |
| `assets/icon.png` | `assets/icon.png` (relative to home_dir) |

### Resolution Rules

1. **@mod-id paths**: `@mod-id/path` → `mods/<mod-id>/path`
2. **Regular paths**: Resolved relative to the home directory (game data directory)

### Mod Validation

When using `@mod-id` syntax:
- The system verifies that the referenced mod exists in the current mod registry
- If the mod doesn't exist, an error is thrown: `"Mod 'mod-id' not found. Cannot resolve path '...'"`

## Loading Progress

The ResourceProxy maintains pre-calculated loading counters for efficient progress tracking:

```
LoadingState {
    requested: u32,  // Number of resources queued via load()
    loaded: u32,     // Number of resources successfully loaded
}
```

| Event | `requested` | `loaded` |
|-------|-------------|----------|
| `load()` called | +1 | — |
| Resource loaded | — | +1 |
| Resource failed | — | — |
| `unload()` on loading | -1 | — |
| `unload()` on loaded | -1 | -1 |
| `unloadAll()` | = 0 | = 0 |

## Memory Management

Resources use **manual deallocation** only. There is no automatic garbage collection:

- Resources stay in cache until explicitly removed with `unload()` or `unloadAll()`
- When a resource is unloaded:
  - If it has an engine handle → GraphicEngine releases the asset
  - If it has data → Memory is deallocated
  - Loading counters are updated

The system exposes available memory information, allowing mods to make autonomous decisions about when to free resources.

## Security

All resource paths are validated through the path security module:

- **Path traversal attacks** (`../`) are blocked by canonicalization
- **Symlinks** are followed and the real path is validated
- **Absolute paths** outside permitted directories are rejected
- **Cross-mod access** validates that the referenced mod is in the current mod list

## Availability

The Resource API is **client-only**. Calling Resource methods on the server will throw descriptive errors.

## Widget Integration

Loaded resources can be referenced by widgets using their alias:

```javascript
// Queue a resource for loading
Resource.load("@assets/background.jpg", "main-bg");

// Wait for it to be ready
await Resource.whenLoaded("main-bg");

// Create widget using the loaded resource
const bgWidget = await container.createChild(WidgetTypes.Image, {
    resourceId: "main-bg",  // Reference by alias
    width: "100%",
    height: "100%"
});
```

## Internal Implementation: Async Asset Loading

The GraphicEngine (Bevy) loads assets **asynchronously**. When a resource is queued via `load()`, the following happens:

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                          ASYNC LOADING FLOW                                  │
│                                                                              │
│  1. load() called                                                            │
│     └──► ResourceProxy adds entry with state = "Loading"                     │
│     └──► GraphicCommand::LoadResource sent to Bevy                           │
│                                                                              │
│  2. Bevy's process_commands system                                           │
│     └──► asset_server.load(&path) returns Handle immediately                 │
│     └──► Handle is registered in PendingAssetRegistry                        │
│     └──► Returns ResourceInfo with state = "Loading"                         │
│                                                                              │
│  3. Bevy loads asset in background (IO thread)                               │
│     └──► Asset loading happens asynchronously                                │
│                                                                              │
│  4. check_pending_assets system (runs every frame)                           │
│     └──► Polls asset_server.get_load_state(handle_id)                        │
│     └──► When LoadState::Loaded detected:                                    │
│         └──► Sends GraphicEvent::ResourceLoaded { alias, asset_id }          │
│     └──► When LoadState::Failed detected:                                    │
│         └──► Sends GraphicEvent::ResourceFailed { alias, asset_id, error }   │
│                                                                              │
│  5. Event processor (main loop)                                              │
│     └──► Receives ResourceLoaded event                                       │
│     └──► Calls resource_proxy.mark_loaded(&alias)                            │
│     └──► Notifies any waiters via resource_waiters[alias].notify_waiters()   │
│                                                                              │
│  6. whenLoaded() resolves                                                    │
│     └──► Returns ResourceInfo with state = "Loaded"                          │
└─────────────────────────────────────────────────────────────────────────────┘
```

### Key Components

| Component | Location | Purpose |
|-----------|----------|---------|
| `ResourceProxy` | `stam_mod_runtimes/api/resource.rs` | Shared resource cache and load queue |
| `PendingAssetRegistry` | `stam_client/engines/bevy.rs` | Tracks assets waiting for Bevy to load |
| `check_pending_assets` | `stam_client/engines/bevy.rs` | Bevy system that polls asset load state |
| `GraphicEvent::ResourceLoaded` | `stam_mod_runtimes/api/graphic/events.rs` | Event sent when asset loading completes |

### Why This Architecture?

1. **Non-blocking**: `load()` returns immediately, allowing mods to queue multiple resources without blocking
2. **Event-driven**: Uses Bevy's native async asset loading instead of polling from Rust
3. **Cross-thread safe**: Events are sent via channels, avoiding direct access to Bevy resources from other threads
4. **Proper state tracking**: Resources only transition to "Loaded" after Bevy confirms the asset is ready

## Language-Specific Documentation

- [JavaScript Resource API](js/resource-manager.md)
