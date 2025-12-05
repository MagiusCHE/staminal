# Plan: Resource Management System (Resource API)

## Objective

Implement a resource loading and caching system for the Staminal client, exposing a new global `Resource` object to JavaScript mods.

### Target Usage Example

```javascript
// Load a resource into cache with an alias
await Resource.load("@bme-assets-01/assets/background/title.jpg", "title-screen-background");

// Create an Image widget that uses the pre-loaded resource
const bkg = await cont.createChild(WidgetTypes.Image, {
    resourceId: "title-screen-background",  // Reference to the alias
    width: "100%",
    height: "100%",
    stretchMode: "cover"
});
```

---

## Proposed Architecture

### 1. ResourceProxy Positioning

The `ResourceProxy` must be **shared across all runtimes** (JavaScript, Lua, future C#, etc.)
because loaded resources must be accessible cross-runtime.

```
┌─────────────────────────────────────────────────────────────────────────┐
│                         ResourceProxy (Rust)                             │
│                    (Shared across all runtimes)                          │
│  ┌───────────────────────────────────────────────────────────────────┐  │
│  │                      Unified Cache                                 │  │
│  │                                                                    │  │
│  │  resource_id → ResourceEntry {                                     │  │
│  │      path: String,              // Original path                   │  │
│  │      resolved_path: String,     // Resolved path                   │  │
│  │      resource_type: ResourceType,                                  │  │
│  │      state: ResourceState,                                         │  │
│  │      engine_handle: Option<EngineHandle>,  // GE Handle            │  │
│  │      data: Option<ResourceData>,           // For non-GE resources │  │
│  │  }                                                                 │  │
│  └───────────────────────────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────────────────────────┘
        │                                           │
        │ Graphic resources                         │ Non-graphic resources
        │ (image, font, shader...)                  │ (json, text, binary...)
        ▼                                           ▼
┌─────────────────────────────┐          ┌─────────────────────────────┐
│     GraphicEngine           │          │     ResourceProxy Cache     │
│  (Bevy AssetServer)         │          │  (In-memory data)           │
│                             │          │                             │
│  - Maintains Handle<T>      │          │  - serde_json::Value        │
│  - Automatic caching        │          │  - String                   │
│  - Reference counting       │          │  - Vec<u8>                  │
└─────────────────────────────┘          └─────────────────────────────┘
```

### 2. Resource Routing Rules

Each GraphicEngine declares which resource types it can handle:

```rust
/// Trait that each GraphicEngine must implement
pub trait GraphicEngineCapabilities {
    /// Returns the resource types this engine can handle
    fn supported_resource_types(&self) -> Vec<ResourceType>;
}

// Example for Bevy:
impl GraphicEngineCapabilities for BevyEngine {
    fn supported_resource_types(&self) -> Vec<ResourceType> {
        vec![
            ResourceType::Image,
            ResourceType::Font,
            ResourceType::Shader,
            // Audio in future
        ]
    }
}
```

**Routing logic in `Resource.load()`:**

1. Determine `ResourceType` from path (file extension)
2. Ask the active GraphicEngine: "Do you support this type?"
3. **If yes** → Send command to GraphicEngine, store `engine_handle`
4. **If no** → Load into memory in ResourceProxy, store in `data`

### 3. EngineHandle Structure

To keep the GraphicEngine handle alive and allow retrieval:

```rust
/// Opaque handle representing a resource in the GraphicEngine
#[derive(Clone)]
pub enum EngineHandle {
    /// Bevy handle (wraps an internal ID that Bevy can resolve)
    Bevy {
        asset_id: u64,  // Unique ID for lookup in Bevy's ResourceRegistry
    },
    // Future: other engines
    // Vulkan { ... },
    // WebGL { ... },
}
```

On the Bevy side, `ResourceRegistry` maintains the mapping:

```rust
#[derive(Resource, Default)]
struct ResourceRegistry {
    /// asset_id → Handle<Image> (keeps handle alive)
    images: HashMap<u64, Handle<Image>>,
    /// asset_id → Handle<Font>
    fonts: HashMap<u64, Handle<Font>>,
    /// asset_id → Handle<Shader>
    shaders: HashMap<u64, Handle<Shader>>,
    /// Counter to generate unique asset_ids
    next_id: AtomicU64,
}
```

### 4. ResourceProxy API for Handle Management

The `ResourceProxy` must expose methods to manage the `resource_id` ↔ `engine_handle` pairing:

```rust
impl ResourceProxy {
    // ========================================================================
    // Methods for resource_id → engine_handle management
    // ========================================================================

    /// Registers a new resource with its engine handle
    /// NOTE: Automatically increments loading_state.requested
    pub fn register(&self, resource_id: &str, entry: ResourceEntry) -> Result<(), String>;

    /// Checks if a resource exists in cache
    pub fn exists(&self, resource_id: &str) -> bool;

    /// Gets the complete entry for a resource
    pub fn get(&self, resource_id: &str) -> Option<ResourceEntry>;

    /// Gets only the engine handle (for widget use)
    pub fn get_engine_handle(&self, resource_id: &str) -> Option<EngineHandle>;

    /// Updates an existing resource (for forceReload)
    pub fn update(&self, resource_id: &str, entry: ResourceEntry) -> Result<(), String>;

    /// Marks a resource as loaded
    /// NOTE: Automatically increments loading_state.loaded
    pub fn mark_loaded(&self, resource_id: &str) -> Result<(), String>;

    /// Marks a resource as failed
    /// NOTE: Does NOT modify loading_state (stays in requested for retry)
    pub fn mark_failed(&self, resource_id: &str, error: String) -> Result<(), String>;

    /// Removes a resource from cache
    /// NOTE: Updates loading_state based on resource state
    /// - If loading: requested -= 1
    /// - If loaded: requested -= 1, loaded -= 1
    /// If has engine_handle, sends command to GraphicEngine to release
    pub async fn remove(&self, resource_id: &str) -> Result<bool, String>;

    /// Removes all resources
    /// NOTE: Resets loading_state to { requested: 0, loaded: 0 }
    pub async fn clear(&self) -> Result<(), String>;

    /// Lists all loaded resources
    pub fn list(&self) -> Vec<String>;

    /// Gets info about a resource (for JS API)
    pub fn get_info(&self, resource_id: &str) -> Option<ResourceInfo>;

    // ========================================================================
    // Loading Progress (pre-calculated, no on-demand computation)
    // ========================================================================

    /// Gets the current loading state
    /// NOTE: Returns pre-calculated data, O(1)
    pub fn get_loading_progress(&self) -> LoadingState;
}

/// Loading state - updated incrementally, never calculated
#[derive(Clone, Copy, Default)]
pub struct LoadingState {
    pub requested: u32,
    pub loaded: u32,
}
```

### 5. Widget Usage Flow

When an Image widget is created with `resourceId`:

```
JavaScript                    ResourceProxy                 Bevy Engine
    │                              │                              │
    │ createChild(Image, {         │                              │
    │   resourceId: "bg-title"     │                              │
    │ })                           │                              │
    │──────────────────────────────>│                              │
    │                              │                              │
    │                              │ get_engine_handle("bg-title")│
    │                              │──────────────────────────────>│
    │                              │                              │
    │                              │<─ EngineHandle::Bevy {       │
    │                              │     asset_id: 42             │
    │                              │   }                          │
    │                              │                              │
    │                              │ CreateWidget command         │
    │                              │   with asset_id: 42          │
    │                              │──────────────────────────────>│
    │                              │                              │
    │                              │                              │ ResourceRegistry
    │                              │                              │   .images.get(42)
    │                              │                              │   → Handle<Image>
    │                              │                              │
    │                              │                              │ spawn(ImageNode {
    │                              │                              │   image: handle
    │                              │                              │ })
    │                              │                              │
```

### 6. Resource Types

| Type | Extensions | Cache Location | Bevy Asset Type |
|------|-----------|----------------|-----------------|
| `image` | .png, .jpg, .jpeg, .gif, .webp, .bmp | GraphicEngine | `Handle<Image>` |
| `audio` | .mp3, .ogg, .wav, .flac | GraphicEngine (future) | `Handle<AudioSource>` |
| `video` | .mp4, .webm | GraphicEngine (future) | Custom |
| `shader` | .wgsl, .glsl | GraphicEngine | `Handle<Shader>` |
| `font` | .ttf, .otf, .woff | GraphicEngine | `Handle<Font>` |
| `json` | .json | RuntimeCache | `serde_json::Value` |
| `text` | .txt, .md, .xml | RuntimeCache | `String` |
| `binary` | * | RuntimeCache | `Vec<u8>` |

### 7. JavaScript API Signature

```typescript
// New global Resource object
declare const Resource: {
    /**
     * Loads a resource into cache
     * @param path - Resource path (supports @mod-id/path syntax)
     * @param alias - Unique alias to reference the resource
     * @param options - Optional options
     * @returns Promise that resolves with info about the loaded resource
     */
    load(path: string, alias: string, options?: LoadOptions): Promise<ResourceInfo>;

    /**
     * Checks if a resource is in cache and fully loaded
     * @param alias - Resource alias
     * @returns true if the resource is in cache and loaded
     */
    isLoaded(alias: string): boolean;

    /**
     * Gets info about a loaded resource
     * @param alias - Resource alias
     * @returns Resource info or null if not found
     */
    getInfo(alias: string): ResourceInfo | null;

    /**
     * Gets the global loading progress
     * NOTE: Returns pre-calculated data, does not compute from lists
     * @returns Current loading state
     */
    getLoadingProgress(): LoadingProgress;

    /**
     * Unloads a resource from cache
     * @param alias - Alias of resource to remove
     * @returns true if the resource was removed
     */
    unload(alias: string): Promise<boolean>;

    /**
     * Unloads all resources from cache
     */
    unloadAll(): Promise<void>;
};

interface LoadingProgress {
    /** Number of resources requested (passed to load()) */
    requested: number;
    /** Number of resources successfully loaded */
    loaded: number;
}

interface LoadOptions {
    /** Force reload even if already in cache */
    forceReload?: boolean;
    /** Explicit type (if not deducible from extension) */
    type?: ResourceType;
}

interface ResourceInfo {
    /** Resource alias */
    alias: string;
    /** Original path */
    path: string;
    /** Resolved path (absolute) */
    resolvedPath: string;
    /** Resource type */
    type: ResourceType;
    /** Loading state */
    state: "loading" | "loaded" | "error";
    /** Size in bytes (if available) */
    size?: number;
    /** Error (if state === "error") */
    error?: string;
}

type ResourceType = "image" | "audio" | "video" | "shader" | "font" | "json" | "text" | "binary";
```

### 8. ImageConfig Modification for WidgetConfig

```rust
// In widget.rs - Modified ImageConfig
pub struct ImageConfig {
    /// Asset path (relative to mod or asset folder) - DEPRECATED, use resource_id
    #[serde(default)]
    pub path: Option<String>,

    /// Resource ID (alias from Resource.load) - NEW
    #[serde(default)]
    pub resource_id: Option<String>,

    // ... rest unchanged
    pub scale_mode: ImageScaleMode,
    pub tint: Option<ColorValue>,
    pub opacity: Option<f32>,
    pub flip_x: bool,
    pub flip_y: bool,
    pub source_rect: Option<RectValue>,
}
```

---

## Files to Modify/Create

### New Files

| File | Description |
|------|-------------|
| `apps/shared/stam_mod_runtimes/src/api/resource.rs` | ResourceApi and ResourceProxy |
| `apps/shared/stam_mod_runtimes/src/adapters/js/glue/resource.js` | JS glue code (optional) |

### Files to Modify

| File | Changes |
|------|---------|
| `apps/shared/stam_mod_runtimes/src/api/mod.rs` | Export ResourceApi |
| `apps/shared/stam_mod_runtimes/src/api/graphic/commands.rs` | Add LoadResource, UnloadResource |
| `apps/shared/stam_mod_runtimes/src/api/graphic/proxy.rs` | Add resource_cache to GraphicProxy |
| `apps/shared/stam_mod_runtimes/src/api/graphic/widget.rs` | Modify ImageConfig for resource_id |
| `apps/shared/stam_mod_runtimes/src/adapters/js/bindings.rs` | JavaScript bindings for Resource |
| `apps/shared/stam_mod_runtimes/src/adapters/js/runtime.rs` | Setup Resource API |
| `apps/stam_client/src/engines/bevy.rs` | LoadResource handler, ResourceRegistry, real Image widget |

---

## Step-by-Step Implementation

### Phase 1: Base Data Structures

1. **Create `resource.rs`** with:
   - `ResourceType` enum
   - `ResourceState` enum (Loading, Loaded, Error)
   - `ResourceInfo` struct
   - `ResourceApi` struct (runtime cache metadata)

2. **Add graphic commands** in `commands.rs`:
   ```rust
   LoadResource {
       path: String,
       alias: String,
       resource_type: ResourceType,
       force_reload: bool,
       response_tx: oneshot::Sender<Result<ResourceInfo, String>>,
   },

   UnloadResource {
       alias: String,
       response_tx: oneshot::Sender<Result<(), String>>,
   },

   UnloadAllResources {
       response_tx: oneshot::Sender<Result<(), String>>,
   },
   ```

### Phase 2: ResourceProxy and Runtime Cache

3. **Extend GraphicProxy** in `proxy.rs`:
   ```rust
   pub struct GraphicProxy {
       // ... existing fields ...

       /// Resource cache for metadata
       resources: Arc<RwLock<HashMap<String, ResourceInfo>>>,
   }
   ```

4. **Implement ResourceApi methods**:
   - `load()` - resolves path, determines type, sends command to engine if graphic
   - `is_loaded()` - checks cache
   - `get_info()` - returns metadata
   - `unload()` / `unload_all()` - removes from cache(s)

### Phase 3: Bevy Engine Integration

5. **Add ResourceRegistry** in `bevy.rs`:
   ```rust
   #[derive(Resource, Default)]
   struct ResourceRegistry {
       /// Alias -> Handle mapping for images
       images: HashMap<String, Handle<Image>>,
       /// Alias -> Handle mapping for fonts (already exists as FontRegistry)
       // fonts: HashMap<String, Handle<Font>>,
       /// Alias -> ResourceInfo for tracking
       info: HashMap<String, ResourceInfo>,
   }
   ```

6. **Handler for LoadResource**:
   - Uses `asset_server.load()` to load
   - Stores Handle in registry
   - Returns ResourceInfo

7. **Modify Image widget creation**:
   - If `resource_id` present, look up Handle in ResourceRegistry
   - Use Bevy's `ImageNode` with the Handle
   - Apply scale_mode, tint, opacity

### Phase 4: JavaScript Bindings

8. **Create bindings in `bindings.rs`**:
   ```rust
   #[qjs(rename = "Resource")]
   pub struct JsResource { ... }

   #[qjs]
   impl JsResource {
       #[qjs(rename = "load")]
       pub async fn load(...) -> Result<JsResourceInfo, ...>;

       #[qjs(rename = "isLoaded")]
       pub fn is_loaded(...) -> bool;
       // ...
   }
   ```

9. **Register in runtime.rs** in `setup_global_apis()`

### Phase 5: Testing and Documentation

10. **Update docs/** with Resource API documentation

---

## Security Considerations

- **Path Validation**: All paths must go through `path_security` module
- **Cross-mod access**: The `@mod-id/path` syntax must validate that the referenced mod is in the current mod list

---

## Design Decisions

### 1. No Cache Limit + System Memory API

There are no limits on the resource cache. Instead, a method to get free RAM is exposed:

```typescript
// New method in Process or System
Process.getAvailableMemory(): number  // Bytes of free RAM
```

This allows MODs to decide autonomously when and how to manage memory.

### 2. Manual Deallocation (No Garbage Collection)

Resources stay in cache **until the user explicitly removes them**:

- `Resource.unload(resourceId)` - Removes a specific resource
- `Resource.unloadAll()` - Removes all resources

When a resource is removed from ResourceProxy:
1. If has `engine_handle` → sends command to GraphicEngine to release the Handle
2. If has `data` → deallocates in-memory data
3. Updates state counters (see below)

### 3. Loading Progress API

Instead of callbacks for individual resources, ResourceProxy maintains a **pre-calculated loading state**:

```rust
/// Loading state - updated on each operation, not calculated on-demand
#[derive(Clone, Default)]
pub struct LoadingState {
    /// Number of resources requested (passed to load())
    pub requested: u32,
    /// Number of resources actually loaded
    pub loaded: u32,
}
```

**Update rules:**

| Event | Action on `requested` | Action on `loaded` |
|-------|----------------------|-------------------|
| `load()` called | +1 | - |
| Resource loaded successfully | - | +1 |
| Resource failed (error) | - | - (stays in requested) |
| `unload()` on loading resource | -1 | - |
| `unload()` on loaded resource | -1 | -1 |
| `unloadAll()` | = 0 | = 0 |

**JavaScript API:**

```typescript
interface LoadingProgress {
    requested: number;  // Resources passed to load()
    loaded: number;     // Resources loaded
}

Resource.getLoadingProgress(): LoadingProgress;
```

**Typical MOD usage:**

```javascript
// Start loading multiple resources without await
Resource.load("@assets/bg1.png", "bg1");
Resource.load("@assets/bg2.png", "bg2");
Resource.load("@assets/bg3.png", "bg3");

// Progress polling
function checkProgress() {
    const progress = Resource.getLoadingProgress();
    console.log(`Loading: ${progress.loaded}/${progress.requested}`);

    if (progress.loaded === progress.requested) {
        // All loaded, proceed
        startGame();
    } else {
        // Check again later
        setTimeout(checkProgress, 100);
    }
}
checkProgress();
```

### 4. Resource Mapping for GraphicEngine

Each GraphicEngine declares which file extensions it can handle. Unsupported resources are loaded by ResourceProxy.

#### Bevy 0.17 - Supported Resources

References: [Bevy Cargo Features](https://github.com/bevyengine/bevy/blob/main/docs/cargo_features.md), [Bevy Cheat Book](https://bevy-cheatbook.github.io/builtins.html)

```rust
/// Extension → ResourceType mapping for Bevy
pub fn bevy_supported_extensions() -> HashMap<&'static str, ResourceType> {
    let mut map = HashMap::new();

    // ========================================================================
    // IMAGES (Handle<Image>)
    // ========================================================================
    // Default enabled
    map.insert("png", ResourceType::Image);
    map.insert("hdr", ResourceType::Image);
    map.insert("ktx2", ResourceType::Image);
    // Optional features (enable in Cargo.toml if needed)
    map.insert("jpg", ResourceType::Image);
    map.insert("jpeg", ResourceType::Image);
    map.insert("bmp", ResourceType::Image);
    map.insert("dds", ResourceType::Image);
    map.insert("tga", ResourceType::Image);
    map.insert("tiff", ResourceType::Image);
    map.insert("webp", ResourceType::Image);
    map.insert("gif", ResourceType::Image);
    map.insert("ico", ResourceType::Image);
    map.insert("exr", ResourceType::Image);  // OpenEXR
    map.insert("pnm", ResourceType::Image);
    map.insert("pam", ResourceType::Image);
    map.insert("pbm", ResourceType::Image);
    map.insert("pgm", ResourceType::Image);
    map.insert("ppm", ResourceType::Image);
    map.insert("qoi", ResourceType::Image);
    map.insert("basis", ResourceType::Image);  // Basis Universal

    // ========================================================================
    // FONTS (Handle<Font>)
    // ========================================================================
    map.insert("ttf", ResourceType::Font);
    map.insert("otf", ResourceType::Font);

    // ========================================================================
    // AUDIO (Handle<AudioSource>) - Requires audio feature
    // ========================================================================
    map.insert("ogg", ResourceType::Audio);   // vorbis feature (default)
    map.insert("wav", ResourceType::Audio);   // wav feature
    map.insert("mp3", ResourceType::Audio);   // mp3 feature
    map.insert("flac", ResourceType::Audio);  // flac feature

    // ========================================================================
    // SHADERS (Handle<Shader>)
    // ========================================================================
    map.insert("wgsl", ResourceType::Shader);

    // ========================================================================
    // 3D MODELS (Handle<Gltf>) - bevy_gltf feature
    // ========================================================================
    map.insert("gltf", ResourceType::Model3D);
    map.insert("glb", ResourceType::Model3D);

    map
}
```

#### Resources NOT Supported by Bevy → ResourceProxy

These are loaded into memory by ResourceProxy:

| Extension | ResourceType | Data in ResourceData |
|-----------|-------------|---------------------|
| `.json` | `Json` | `serde_json::Value` |
| `.txt`, `.md`, `.xml`, `.csv` | `Text` | `String` |
| `.mp4`, `.webm`, `.avi` | `Video` | `Vec<u8>` (future: external player) |
| `*` (other) | `Binary` | `Vec<u8>` |

#### GraphicEngine Trait

```rust
pub trait GraphicEngineResourceSupport {
    /// Returns the extension → ResourceType map for this engine
    fn supported_extensions(&self) -> &HashMap<&'static str, ResourceType>;

    /// Checks if an extension is supported
    fn supports_extension(&self, ext: &str) -> bool {
        self.supported_extensions().contains_key(ext.to_lowercase().as_str())
    }

    /// Gets the ResourceType for an extension
    fn get_resource_type(&self, ext: &str) -> Option<ResourceType> {
        self.supported_extensions().get(ext.to_lowercase().as_str()).copied()
    }
}
```

---

## Estimated Timeline

| Phase | Effort |
|-------|--------|
| Phase 1: Base structures | Low |
| Phase 2: ResourceProxy | Medium |
| Phase 3: Bevy integration | High (requires Bevy UI knowledge) |
| Phase 4: JS bindings | Medium |
| Phase 5: Tests and docs | Low |

---

## Technical Notes

### Bevy 0.17 Asset System - Built-in Caching

Bevy already has an **automatic caching system** built into the AssetServer:

> "As long as at least one handle has not been dropped, calling `AssetServer::load` on the same path will return the same handle."

**Implications for our design:**

1. **No separate Handle cache needed** - Bevy handles this already
2. **Our layer only needs to:**
   - Map alias → path
   - Keep Handles "alive" (prevent them from being dropped)
   - Track metadata (type, state, etc.)

### Relevant Bevy Methods (from [AssetServer docs](https://docs.rs/bevy/latest/bevy/asset/struct.AssetServer.html))

| Method | Description |
|--------|-------------|
| `load(path)` | Loads asset, returns Handle. **Returns existing handle if already loaded** |
| `get_handle(path)` | Gets existing Handle without reloading |
| `is_loaded(id)` | Checks if asset is loaded |
| `is_loaded_with_dependencies(id)` | Checks asset + dependencies |
| `reload(path)` | Forces reload (for `forceReload: true`) |
| `get_load_state(id)` | Gets state: Loading, Loaded, Failed, etc. |

### Bevy ImageNode (Bevy 0.17)

Reference: [ImageNode docs](https://docs.rs/bevy_ui/latest/bevy_ui/widget/struct.ImageNode.html)

```rust
pub struct ImageNode {
    pub color: Color,                        // Tint color (default: white)
    pub image: Handle<Image>,                // Texture handle
    pub texture_atlas: Option<TextureAtlas>, // For sprite sheets
    pub flip_x: bool,
    pub flip_y: bool,
    pub rect: Option<Rect>,                  // Source rect
    pub image_mode: NodeImageMode,           // Scaling mode
}
```

**Creation:**
```rust
commands.spawn((
    ImageNode::new(asset_server.load("path/to/image.png"))
        .with_color(Color::srgba(1.0, 1.0, 1.0, 0.8))  // Opacity via alpha
        .with_flip_x()
        .with_mode(NodeImageMode::Stretch),
    Node {
        width: Val::Percent(100.0),
        height: Val::Percent(100.0),
        ..default()
    },
));
```

### NodeImageMode (Scaling)

Reference: [NodeImageMode docs](https://docs.rs/bevy/latest/bevy/ui/widget/enum.NodeImageMode.html)

| Bevy Mode | Our `stretchMode` | Description |
|-----------|------------------|-------------|
| `Auto` | `"auto"` | Uses original image dimensions |
| `Stretch` | `"stretch"` | Ignores aspect ratio, fills node |
| `Sliced(slicer)` | `"nine-slice"` | 9-slice for UI elements |
| `Tiled { tile_x, tile_y, stretch_value }` | `"tile"` | Repeats texture |

**NOTE:** Missing `cover` and `contain`! We should:
1. Implement them manually by calculating dimensions
2. Or use `Auto` + JS dimension calculation

### Final Architecture

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                              MOD RUNTIMES                                    │
│  ┌─────────────────┐  ┌─────────────────┐  ┌─────────────────┐              │
│  │ JavaScript (JS) │  │   Lua (future)  │  │  C# (future)    │              │
│  └────────┬────────┘  └────────┬────────┘  └────────┬────────┘              │
│           │                    │                    │                       │
│           └────────────────────┼────────────────────┘                       │
│                                │                                            │
│                                ▼                                            │
│  ┌──────────────────────────────────────────────────────────────────────┐  │
│  │                      ResourceProxy (Shared)                           │  │
│  │  ┌────────────────────────────────────────────────────────────────┐  │  │
│  │  │  resources: HashMap<String, ResourceEntry>                     │  │  │
│  │  │                                                                │  │  │
│  │  │  ResourceEntry {                                               │  │  │
│  │  │      resource_id: String,                                      │  │  │
│  │  │      path: String,                                             │  │  │
│  │  │      resolved_path: String,                                    │  │  │
│  │  │      resource_type: ResourceType,                              │  │  │
│  │  │      state: ResourceState,                                     │  │  │
│  │  │      engine_handle: Option<EngineHandle>,  // For GE resources │  │  │
│  │  │      data: Option<ResourceData>,           // For non-GE       │  │  │
│  │  │  }                                                             │  │  │
│  │  └────────────────────────────────────────────────────────────────┘  │  │
│  │                                                                       │  │
│  │  Methods: register(), exists(), get(), get_engine_handle(),          │  │
│  │           update(), remove(), clear(), list(), get_info()            │  │
│  └──────────────────────────────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────────────────────────────┘
                                     │
           ┌─────────────────────────┴─────────────────────────┐
           │                                                   │
           ▼                                                   ▼
┌──────────────────────────────────┐        ┌──────────────────────────────────┐
│  GraphicEngine (Bevy)            │        │  ResourceProxy Data Cache        │
│  ┌────────────────────────────┐  │        │  ┌────────────────────────────┐  │
│  │    ResourceRegistry        │  │        │  │  ResourceData enum         │  │
│  │                            │  │        │  │                            │  │
│  │  images: HashMap<          │  │        │  │  Json(serde_json::Value)   │  │
│  │    u64, Handle<Image>>     │  │        │  │  Text(String)              │  │
│  │                            │  │        │  │  Binary(Vec<u8>)           │  │
│  │  fonts: HashMap<           │  │        │  │                            │  │
│  │    u64, Handle<Font>>      │  │        │  └────────────────────────────┘  │
│  │                            │  │        │                                  │
│  │  shaders: HashMap<         │  │        │  For: .json, .txt, .xml, etc.    │
│  │    u64, Handle<Shader>>    │  │        │                                  │
│  └────────────────────────────┘  │        └──────────────────────────────────┘
│                                  │
│  Bevy AssetServer does caching  │
│  automatically per path         │
└──────────────────────────────────┘
```

**Benefits of this architecture:**

1. **Cross-runtime sharing**: A resource loaded from JS is accessible from Lua
2. **Handles always alive**: ResourceProxy maintains references to engine handles
3. **Separation of concerns**: GE manages graphic resources, RP manages metadata and data
4. **Efficient lookup**: Widgets use `get_engine_handle()` to get asset_id

### Path Resolution

Path resolution for `@mod-id/path` already uses `SystemApi::get_assets_path()`:
1. `@other-mod/icons/icon.png` → `mods/other-mod/assets/icons/icon.png`
2. `icons/icon.png` → First looks in `mods/current-mod/assets/`, then in `assets/`

### Thread Safety

- `ResourceApi` lives in the worker thread (async runtime)
- Commands travel via `mpsc` to Bevy's main thread
- Responses return via `oneshot` channel

---

## Sources

- [Bevy AssetServer Documentation](https://docs.rs/bevy/latest/bevy/asset/struct.AssetServer.html)
- [Bevy Assets Collection](https://docs.rs/bevy/latest/bevy/asset/struct.Assets.html)
- [ImageNode Documentation](https://docs.rs/bevy_ui/latest/bevy_ui/widget/struct.ImageNode.html)
- [NodeImageMode Documentation](https://docs.rs/bevy/latest/bevy/ui/widget/enum.NodeImageMode.html)
- [Bevy 0.17 Release Notes](https://thisweekinbevy.com/issue/2025-09-29-bevy-017-is-out)
